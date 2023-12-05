// Copyright Moonsong Labs
// This file is part of Moonkit.

// Moonkit is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonkit is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonkit.  If not, see <http://www.gnu.org/licenses/>.

//! The nimbus consensus client-side worker
//!
//! It queries the in-runtime filter to determine whether any keys
//! stored in its keystore are eligible to author at this slot. If it has an eligible
//! key it authors.

pub use import_queue::import_queue;
pub use manual_seal::NimbusManualSealConsensusDataProvider;

use cumulus_client_collator::service::ServiceInterface as CollatorServiceInterface;
use cumulus_client_consensus_common::{ParachainBlockImportMarker, ParachainCandidate};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::{
	relay_chain::{BlockId as RBlockId, Hash as PHash, Header as PHeader},
	CollectCollationInfo, ParaId, PersistedValidationData,
};
use cumulus_primitives_parachain_inherent::ParachainInherentData;
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use futures::prelude::*;
use log::{debug, info, warn};
use nimbus_primitives::{
	CompatibleDigestItem, DigestsProvider, NimbusApi, NimbusId, NIMBUS_KEY_ID,
};
use polkadot_node_primitives::{Collation, CollationResult, MaybeCompressedPoV};
use polkadot_primitives::CollatorPair;
use sc_client_api::{BlockBackend, BlockOf};
use sc_consensus::{BlockImport, BlockImportParams};
use sp_api::ProvideRuntimeApi;
use sp_application_crypto::ByteArray;
use sp_blockchain::HeaderBackend;
use sp_consensus::{BlockOrigin, Proposal};
use sp_core::{crypto::CryptoTypeId, sr25519, Decode, Encode};
use sp_inherents::{CreateInherentDataProviders, InherentData, InherentDataProvider};
use sp_keystore::{Keystore, KeystorePtr};
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	DigestItem,
};
use std::convert::TryInto;
use std::error::Error;
use std::{marker::PhantomData, sync::Arc, time::Duration};

mod import_queue;
mod manual_seal;

const LOG_TARGET: &str = "filtering-consensus";

/// Nimbus Consensus parameters
pub struct NimbusConsensusParams<Proposer, BI, ParaClient, RClient, CIDP, CS, DP = ()> {
	pub para_id: ParaId,
	/// A handle to the relay-chain client's "Overseer" or task orchestrator.
	pub overseer_handle: OverseerHandle,
	pub proposer: Proposer,
	/// The block import handle.
	pub block_import: BI,
	pub para_client: Arc<ParaClient>,
	/// An interface to the relay-chain client.
	pub relay_client: RClient,
	/// The underlying keystore, which should contain Nimbus consensus keys.
	pub keystore: KeystorePtr,
	/// The collator key used to sign collations before submitting to validators.
	pub collator_key: CollatorPair,
	pub skip_prediction: bool,
	/// A builder for inherent data builders.
	pub create_inherent_data_providers: CIDP,
	/// The collator service used for bundling proposals into collations and announcing
	/// to the network.
	pub collator_service: CS,
	pub additional_digests_provider: DP,
}

/// Run bare Nimbus consensus as a relay-chain-driven collator.
pub fn run_relay_driven_collator<Block, BI, CIDP, Backend, Client, RClient, Proposer, CS, DP>(
	params: NimbusConsensusParams<Proposer, BI, Client, RClient, CIDP, CS, DP>,
) -> impl Future<Output = ()> + Send + 'static
where
	Block: BlockT + Send,
	CIDP: CreateInherentDataProviders<Block, (PHash, PersistedValidationData, NimbusId)> + 'static,
	CIDP::InherentDataProviders: Send,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	Client: ProvideRuntimeApi<Block>
		+ BlockOf
		+ HeaderBackend<Block>
		+ BlockBackend<Block>
		+ Send
		+ Sync
		+ 'static,
	Client::Api: NimbusApi<Block> + CollectCollationInfo<Block>,
	RClient: RelayChainInterface + Send + Clone + 'static,
	Proposer: ProposerInterface<Block> + Send + Sync + 'static,
	CS: CollatorServiceInterface<Block> + Send + Sync + 'static,
	DP: DigestsProvider<NimbusId, <Block as BlockT>::Hash> + Send + Sync + 'static,
{
	async move {
		let mut collation_requests = cumulus_client_collator::relay_chain_driven::init(
			params.collator_key,
			params.para_id,
			params.overseer_handle,
		)
		.await;

		let mut nimbus_consensus = {
			let params = BuildNimbusConsensusParams {
				para_id: params.para_id,
				proposer: params.proposer,
				create_inherent_data_providers: params.create_inherent_data_providers,
				block_import: params.block_import,
				para_client: params.para_client.clone(),
				relay_client: params.relay_client.clone(),
				keystore: params.keystore.clone(),
				skip_prediction: params.skip_prediction,
				additional_digests_provider: params.additional_digests_provider,
				collator_service: params.collator_service,
			};

			NimbusConsensus::<Block, _, _, _, _, _, _, _>::build(params)
		};

		while let Some(request) = collation_requests.next().await {
			macro_rules! reject_with_error {
				($err:expr) => {{
					request.complete(None);
					tracing::error!(target: crate::LOG_TARGET, err = ?{ $err });
					continue;
				}};
			}

			macro_rules! try_request {
				($x:expr) => {{
					match $x {
						Ok(x) => x,
						Err(e) => reject_with_error!(e),
					}
				}};
			}

			let validation_data = request.persisted_validation_data();

			let parent_header = try_request!(Block::Header::decode(
				&mut &validation_data.parent_head.0[..]
			));

			let parent_hash = parent_header.hash();

			if !nimbus_consensus
				.collator_service()
				.check_block_status(parent_hash, &parent_header)
			{
				continue;
			}

			let relay_parent_header = match params
				.relay_client
				.header(RBlockId::hash(*request.relay_parent()))
				.await
			{
				Err(e) => reject_with_error!(e),
				Ok(None) => continue, // sanity: would be inconsistent to get `None` here
				Ok(Some(h)) => h,
			};

			let nimbus_id = match nimbus_consensus
				.claim_slot(&parent_header, &relay_parent_header)
				.await
			{
				Ok(None) => continue,
				Ok(Some(nimbus_id)) => nimbus_id,
				Err(e) => reject_with_error!(e),
			};

			let inherent_data = try_request!(
				nimbus_consensus
					.create_inherent_data(
						parent_header.hash(),
						&validation_data,
						*request.relay_parent(),
						nimbus_id.clone(),
					)
					.await
			);

			let (collation, post_hash) = try_request!(
				nimbus_consensus
					.collate(
						&parent_header,
						nimbus_id,
						inherent_data,
						Duration::from_millis(500), //params.authoring_duration,
						// Set the block limit to 50% of the maximum PoV size.
						//
						// TODO: If we got benchmarking that includes the proof size,
						// we should be able to use the maximum pov size.
						(validation_data.max_pov_size / 2) as usize,
					)
					.await
			);

			let result_sender = Some(
				nimbus_consensus
					.collator_service()
					.announce_with_barrier(post_hash),
			);
			request.complete(Some(CollationResult {
				collation,
				result_sender,
			}));
		}
	}
}

/// The implementation of the relay-chain provided consensus for parachains.
struct NimbusConsensus<Block, BI, CIDP, Client, RClient, Proposer, CS, DP = ()> {
	/// Inherent data providers. Only non-consensus inherent data should be provided, i.e.
	/// the timestamp, slot, and paras inherents should be omitted, as they are set by this
	/// collator.
	create_inherent_data_providers: Arc<CIDP>,
	/// Used to actually import blocks.
	block_import: BI,
	/// The underlying para client.
	para_client: Arc<Client>,
	/// An interface to the relay-chain client.
	relay_client: RClient,
	/// The underlying keystore, which should contain Nimbus consensus keys.
	keystore: KeystorePtr,
	/// The para's ID.
	para_id: ParaId,
	/// The underlying block proposer this should call into.
	proposer: Proposer,
	skip_prediction: bool,
	additional_digests_provider: Arc<DP>,
	/// The generic collator service used to plug into this consensus engine.
	collator_service: CS,
	_phantom: PhantomData<Block>,
}

impl<Block, BI, CIDP, Client, RClient, Proposer, CS, DP>
	NimbusConsensus<Block, BI, CIDP, Client, RClient, Proposer, CS, DP>
where
	Block: BlockT,
	Proposer: ProposerInterface<Block> + Send + Sync + 'static,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	Client: ProvideRuntimeApi<Block> + 'static,
	Client::Api: sp_api::Core<Block> + NimbusApi<Block>,
	RClient: RelayChainInterface + Send + Clone + 'static,
	CIDP: CreateInherentDataProviders<Block, (PHash, PersistedValidationData, NimbusId)> + 'static,
	CS: CollatorServiceInterface<Block>,
	DP: DigestsProvider<NimbusId, <Block as BlockT>::Hash> + 'static,
{
	/// Create a new instance of nimbus consensus.
	pub fn build(
		BuildNimbusConsensusParams {
			para_id,
			proposer,
			create_inherent_data_providers,
			block_import,
			para_client,
			relay_client,
			keystore,
			skip_prediction,
			additional_digests_provider,
			collator_service,
			..
		}: BuildNimbusConsensusParams<Proposer, BI, Client, RClient, CIDP, CS, DP>,
	) -> Self {
		Self {
			para_id,
			proposer,
			create_inherent_data_providers: Arc::new(create_inherent_data_providers),
			block_import,
			para_client,
			relay_client,
			keystore,
			skip_prediction,
			additional_digests_provider: Arc::new(additional_digests_provider),
			collator_service,
			_phantom: PhantomData,
		}
	}

	/// Explicitly creates the inherent data for parachain block authoring.
	async fn create_inherent_data(
		&self,
		parent: Block::Hash,
		validation_data: &PersistedValidationData,
		relay_parent: PHash,
		author_id: NimbusId,
	) -> Result<(ParachainInherentData, InherentData), Box<dyn Error + Send + Sync + 'static>> {
		let paras_inherent_data = ParachainInherentData::create_at(
			relay_parent,
			&self.relay_client,
			validation_data,
			self.para_id,
		)
		.await;

		let paras_inherent_data = match paras_inherent_data {
			Some(p) => p,
			None => {
				return Err(
					format!("Could not create paras inherent data at {:?}", relay_parent).into(),
				)
			}
		};

		let other_inherent_data = self
			.create_inherent_data_providers
			.create_inherent_data_providers(
				parent,
				(relay_parent, validation_data.clone(), author_id),
			)
			.map_err(|e| e as Box<dyn Error + Send + Sync + 'static>)
			.await?
			.create_inherent_data()
			.await
			.map_err(Box::new)?;

		Ok((paras_inherent_data, other_inherent_data))
	}

	/// Attempt to claim a slot derived from the given relay-parent header's slot.
	pub async fn claim_slot(
		&self,
		parent: &Block::Header,
		relay_parent_header: &PHeader,
	) -> Result<Option<NimbusId>, Box<dyn Error>> {
		// Determine if runtime change
		let runtime_upgraded = if *parent.number() > sp_runtime::traits::Zero::zero() {
			use sp_api::Core as _;
			let previous_runtime_version: sp_api::RuntimeVersion = self
				.para_client
				.runtime_api()
				.version(parent.hash())
				.map_err(Box::new)?;
			let runtime_version: sp_api::RuntimeVersion = self
				.para_client
				.runtime_api()
				.version(parent.hash())
				.map_err(Box::new)?;

			previous_runtime_version != runtime_version
		} else {
			false
		};

		let maybe_key = if self.skip_prediction || runtime_upgraded {
			first_available_key(&*self.keystore)
		} else {
			first_eligible_key::<Block, Client>(
				self.para_client.clone(),
				&*self.keystore,
				parent,
				*relay_parent_header.number(),
			)
		};

		if let Some(key) = maybe_key {
			Ok(Some(
				NimbusId::from_slice(&key).map_err(|_| "invalid nimbus id (wrong length)")?,
			))
		} else {
			Ok(None)
		}
	}

	/// Propose, seal, and import a block, packaging it into a collation.
	///
	/// Provide the slot to build at as well as any other necessary pre-digest logs,
	/// the inherent data, and the proposal duration and PoV size limits.
	///
	/// The Aura pre-digest should not be explicitly provided and is set internally.
	///
	/// This does not announce the collation to the parachain network or the relay chain.
	pub async fn collate(
		&mut self,
		parent_header: &Block::Header,
		nimbus_id: NimbusId,
		inherent_data: (ParachainInherentData, InherentData),
		proposal_duration: Duration,
		max_pov_size: usize,
	) -> Result<(Collation, Block::Hash), Box<dyn Error + Send + 'static>> {
		let mut logs = vec![CompatibleDigestItem::nimbus_pre_digest(nimbus_id.clone())];
		logs.extend(
			self.additional_digests_provider
				.provide_digests(nimbus_id.clone(), parent_header.hash()),
		);

		let Proposal {
			block,
			storage_changes,
			proof,
		} = self
			.proposer
			.propose(
				&parent_header,
				&inherent_data.0,
				inherent_data.1,
				sp_runtime::generic::Digest { logs },
				proposal_duration,
				// Set the block limit to 50% of the maximum PoV size.
				//
				// TODO: If we got benchmarking that includes that encapsulates the proof size,
				// we should be able to use the maximum pov size.
				Some((max_pov_size / 2) as usize),
			)
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

		let (header, extrinsics) = block.clone().deconstruct();

		let sig_digest = seal_header::<Block>(
			&header,
			&*self.keystore,
			&nimbus_id.to_raw_vec(),
			&sr25519::CRYPTO_ID,
		);

		let mut block_import_params = BlockImportParams::new(BlockOrigin::Own, header.clone());
		block_import_params.post_digests.push(sig_digest.clone());
		block_import_params.body = Some(extrinsics.clone());
		block_import_params.state_action = sc_consensus::StateAction::ApplyChanges(
			sc_consensus::StorageChanges::Changes(storage_changes),
		);

		let post_hash = block_import_params.post_hash();

		// Print the same log line as slots (aura and babe)
		info!(
			"🔖 Sealed block for proposal at {}. Hash now {:?}, previously {:?}.",
			*header.number(),
			block_import_params.post_hash(),
			header.hash(),
		);

		self.block_import
			.import_block(block_import_params)
			.map_err(|e| Box::new(e) as Box<dyn Error + Send>)
			.await?;

		// Compute info about the block after the digest is added
		let mut post_header = header.clone();
		post_header.digest_mut().logs.push(sig_digest.clone());
		let post_block = Block::new(post_header, extrinsics);

		if let Some((collation, block_data)) = self.collator_service.build_collation(
			parent_header,
			post_hash,
			ParachainCandidate {
				block: post_block,
				proof: proof,
			},
		) {
			tracing::info!(
				target: crate::LOG_TARGET,
				"PoV size {{ header: {}kb, extrinsics: {}kb, storage_proof: {}kb }}",
				block_data.header().encode().len() as f64 / 1024f64,
				block_data.extrinsics().encode().len() as f64 / 1024f64,
				block_data.storage_proof().encode().len() as f64 / 1024f64,
			);

			if let MaybeCompressedPoV::Compressed(ref pov) = collation.proof_of_validity {
				tracing::info!(
					target: crate::LOG_TARGET,
					"Compressed PoV size: {}kb",
					pov.block_data.0.len() as f64 / 1024f64,
				);
			}

			Ok((collation, post_hash))
		} else {
			Err(
				Box::<dyn Error + Send + Sync>::from("Unable to produce collation")
					as Box<dyn Error + Send>,
			)
		}
	}

	/// Get the underlying collator service.
	pub fn collator_service(&self) -> &CS {
		&self.collator_service
	}
}

/// Grabs any available nimbus key from the keystore.
/// This may be useful in situations where you expect exactly one key
/// and intend to perform an operation with it regardless of whether it is
/// expected to be eligible. Concretely, this is used in the consensus worker
/// to implement the `skip_prediction` feature.
pub(crate) fn first_available_key(keystore: &dyn Keystore) -> Option<Vec<u8>> {
	// Get all the available keys
	match Keystore::keys(keystore, NIMBUS_KEY_ID) {
		Ok(available_keys) => {
			if available_keys.is_empty() {
				warn!(
					target: LOG_TARGET,
					"🔏 No Nimbus keys available. We will not be able to author."
				);
				None
			} else {
				Some(available_keys[0].clone())
			}
		}
		_ => None,
	}
}

/// Grab the first eligible nimbus key from the keystore
/// If multiple keys are eligible this function still only returns one
/// and makes no guarantees which one as that depends on the keystore's iterator behavior.
/// This is the standard way of determining which key to author with.
pub(crate) fn first_eligible_key<Block: BlockT, C>(
	client: Arc<C>,
	keystore: &dyn Keystore,
	parent: &Block::Header,
	slot_number: u32,
) -> Option<Vec<u8>>
where
	C: ProvideRuntimeApi<Block>,
	C::Api: NimbusApi<Block>,
{
	// Get all the available keys
	let available_keys = Keystore::keys(keystore, NIMBUS_KEY_ID).ok()?;

	// Print a more helpful message than "not eligible" when there are no keys at all.
	if available_keys.is_empty() {
		warn!(
			target: LOG_TARGET,
			"🔏 No Nimbus keys available. We will not be able to author."
		);
		return None;
	}

	// Iterate keys until we find an eligible one, or run out of candidates.
	// If we are skipping prediction, then we author with the first key we find.
	// prediction skipping only really makes sense when there is a single key in the keystore.
	let maybe_key = available_keys.into_iter().find(|type_public_pair| {
		// Have to convert to a typed NimbusId to pass to the runtime API. Maybe this is a clue
		// That I should be passing Vec<u8> across the wasm boundary?
		if let Ok(nimbus_id) = NimbusId::from_slice(&type_public_pair) {
			NimbusApi::can_author(
				&*client.runtime_api(),
				parent.hash(),
				nimbus_id,
				slot_number,
				parent,
			)
			.unwrap_or_default()
		} else {
			false
		}
	});

	// If there are no eligible keys, print the log, and exit early.
	if maybe_key.is_none() {
		info!(
			target: LOG_TARGET,
			"🔮 Skipping candidate production because we are not eligible for slot {}", slot_number
		);
	}

	maybe_key
}

pub(crate) fn seal_header<Block>(
	header: &Block::Header,
	keystore: &dyn Keystore,
	public_pair: &Vec<u8>,
	crypto_id: &CryptoTypeId,
) -> DigestItem
where
	Block: BlockT,
{
	let pre_hash = header.hash();

	let raw_sig = Keystore::sign_with(
		keystore,
		NIMBUS_KEY_ID,
		*crypto_id,
		public_pair,
		pre_hash.as_ref(),
	)
	.expect("Keystore should be able to sign")
	.expect("We already checked that the key was present");

	debug!(target: LOG_TARGET, "The signature is \n{:?}", raw_sig);

	let signature = raw_sig
		.clone()
		.try_into()
		.expect("signature bytes produced by keystore should be right length");

	<DigestItem as CompatibleDigestItem>::nimbus_seal(signature)
}

struct BuildNimbusConsensusParams<Proposer, BI, ParaClient, RClient, CIDP, CS, DP = ()> {
	para_id: ParaId,
	proposer: Proposer,
	/// A builder for inherent data builders.
	create_inherent_data_providers: CIDP,
	/// The block import handle.
	block_import: BI,
	para_client: Arc<ParaClient>,
	/// An interface to the relay-chain client.
	relay_client: RClient,
	/// The underlying keystore, which should contain Nimbus consensus keys.
	keystore: KeystorePtr,
	skip_prediction: bool,
	additional_digests_provider: DP,
	/// The collator service used for bundling proposals into collations and announcing
	/// to the network.
	collator_service: CS,
}
