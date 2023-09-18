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

use cumulus_client_consensus_common::{
	ParachainBlockImport, ParachainCandidate, ParachainConsensus,
};
use cumulus_primitives_core::{relay_chain::Hash as PHash, ParaId, PersistedValidationData};
pub use import_queue::import_queue;
use log::{debug, info, warn};
use nimbus_primitives::{
	CompatibleDigestItem, DigestsProvider, NimbusApi, NimbusId, NIMBUS_KEY_ID,
};
use parking_lot::Mutex;
use sc_client_api::backend::Backend;
use sc_consensus::{BlockImport, BlockImportParams};
use sp_api::ProvideRuntimeApi;
use sp_application_crypto::ByteArray;
use sp_consensus::{
	BlockOrigin, EnableProofRecording, Environment, ProofRecording, Proposal, Proposer,
};
use sp_core::{crypto::CryptoTypeId, sr25519};
use sp_inherents::{CreateInherentDataProviders, InherentData, InherentDataProvider};
use sp_keystore::{Keystore, KeystorePtr};
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	DigestItem,
};
use std::convert::TryInto;
use std::{marker::PhantomData, sync::Arc, time::Duration};
use tracing::error;
mod import_queue;
mod manual_seal;
pub use manual_seal::NimbusManualSealConsensusDataProvider;

const LOG_TARGET: &str = "filtering-consensus";

/// The implementation of the relay-chain provided consensus for parachains.
pub struct NimbusConsensus<B: BlockT, PF, BI, BE, ParaClient, CIDP, DP = ()> {
	para_id: ParaId,
	proposer_factory: Arc<Mutex<PF>>,
	create_inherent_data_providers: Arc<CIDP>,
	block_import: Arc<futures::lock::Mutex<ParachainBlockImport<B, BI, BE>>>,
	parachain_client: Arc<ParaClient>,
	keystore: KeystorePtr,
	skip_prediction: bool,
	additional_digests_provider: Arc<DP>,
	_phantom: PhantomData<B>,
}

impl<B: BlockT, PF, BI, BE, ParaClient, CIDP, DP> Clone
	for NimbusConsensus<B, PF, BI, BE, ParaClient, CIDP, DP>
{
	fn clone(&self) -> Self {
		Self {
			para_id: self.para_id,
			proposer_factory: self.proposer_factory.clone(),
			create_inherent_data_providers: self.create_inherent_data_providers.clone(),
			block_import: self.block_import.clone(),
			parachain_client: self.parachain_client.clone(),
			keystore: self.keystore.clone(),
			skip_prediction: self.skip_prediction,
			additional_digests_provider: self.additional_digests_provider.clone(),
			_phantom: PhantomData,
		}
	}
}

impl<B, PF, BI, BE, ParaClient, CIDP, DP> NimbusConsensus<B, PF, BI, BE, ParaClient, CIDP, DP>
where
	B: BlockT,
	PF: 'static,
	BI: 'static,
	BE: Backend<B> + 'static,
	ParaClient: ProvideRuntimeApi<B> + 'static,
	CIDP: CreateInherentDataProviders<B, (PHash, PersistedValidationData, NimbusId)> + 'static,
	DP: DigestsProvider<NimbusId, <B as BlockT>::Hash> + 'static,
{
	/// Create a new instance of nimbus consensus.
	pub fn build(
		BuildNimbusConsensusParams {
			para_id,
			proposer_factory,
			create_inherent_data_providers,
			block_import,
			backend,
			parachain_client,
			keystore,
			skip_prediction,
			additional_digests_provider,
		}: BuildNimbusConsensusParams<PF, BI, BE, ParaClient, CIDP, DP>,
	) -> Box<dyn ParachainConsensus<B>>
	where
		Self: ParachainConsensus<B>,
	{
		Box::new(Self {
			para_id,
			proposer_factory: Arc::new(Mutex::new(proposer_factory)),
			create_inherent_data_providers: Arc::new(create_inherent_data_providers),
			block_import: Arc::new(futures::lock::Mutex::new(ParachainBlockImport::new(
				block_import,
				backend,
			))),
			parachain_client,
			keystore,
			skip_prediction,
			additional_digests_provider: Arc::new(additional_digests_provider),
			_phantom: PhantomData,
		})
	}

	//TODO Could this be a provided implementation now that we have this async inherent stuff?
	/// Create the data.
	async fn inherent_data(
		&self,
		parent: B::Hash,
		validation_data: &PersistedValidationData,
		relay_parent: PHash,
		author_id: NimbusId,
	) -> Option<InherentData> {
		let inherent_data_providers = self
			.create_inherent_data_providers
			.create_inherent_data_providers(
				parent,
				(relay_parent, validation_data.clone(), author_id),
			)
			.await
			.map_err(|e| {
				tracing::error!(
					target: LOG_TARGET,
					error = ?e,
					"Failed to create inherent data providers.",
				)
			})
			.ok()?;

		inherent_data_providers
			.create_inherent_data()
			.await
			.map_err(|e| {
				tracing::error!(
					target: LOG_TARGET,
					error = ?e,
					"Failed to create inherent data.",
				)
			})
			.ok()
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
pub(crate) fn first_eligible_key<B: BlockT, C>(
	client: Arc<C>,
	keystore: &dyn Keystore,
	parent: &B::Header,
	slot_number: u32,
) -> Option<Vec<u8>>
where
	C: ProvideRuntimeApi<B>,
	C::Api: NimbusApi<B>,
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

pub(crate) fn seal_header<B>(
	header: &B::Header,
	keystore: &dyn Keystore,
	public_pair: &Vec<u8>,
	crypto_id: &CryptoTypeId,
) -> DigestItem
where
	B: BlockT,
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

#[async_trait::async_trait]
impl<B, PF, BI, BE, ParaClient, CIDP, DP> ParachainConsensus<B>
	for NimbusConsensus<B, PF, BI, BE, ParaClient, CIDP, DP>
where
	B: BlockT,
	BI: BlockImport<B> + Send + Sync + 'static,
	BE: Backend<B> + Send + Sync + 'static,
	PF: Environment<B> + Send + Sync + 'static,
	PF::Proposer: Proposer<
		B,
		ProofRecording = EnableProofRecording,
		Proof = <EnableProofRecording as ProofRecording>::Proof,
	>,
	ParaClient: ProvideRuntimeApi<B> + Send + Sync + 'static,
	ParaClient::Api: NimbusApi<B>,
	CIDP: CreateInherentDataProviders<B, (PHash, PersistedValidationData, NimbusId)> + 'static,
	DP: DigestsProvider<NimbusId, <B as BlockT>::Hash> + 'static + Send + Sync,
{
	async fn produce_candidate(
		&mut self,
		parent: &B::Header,
		relay_parent: PHash,
		validation_data: &PersistedValidationData,
	) -> Option<ParachainCandidate<B>> {
		// Determine if runtime change
		let runtime_upgraded = if *parent.number() > sp_runtime::traits::Zero::zero() {
			use sp_api::Core as _;
			let previous_runtime_version: sp_api::RuntimeVersion = self
				.parachain_client
				.runtime_api()
				.version(parent.hash())
				.ok()?;
			let runtime_version: sp_api::RuntimeVersion = self
				.parachain_client
				.runtime_api()
				.version(parent.hash())
				.ok()?;

			previous_runtime_version != runtime_version
		} else {
			false
		};

		let maybe_key = if self.skip_prediction || runtime_upgraded {
			first_available_key(&*self.keystore)
		} else {
			first_eligible_key::<B, ParaClient>(
				self.parachain_client.clone(),
				&*self.keystore,
				parent,
				validation_data.relay_parent_number,
			)
		};

		// If there are no eligible keys, print the log, and exit early.
		let type_public_pair = match maybe_key {
			Some(p) => p,
			None => {
				return None;
			}
		};

		let proposer_future = self.proposer_factory.lock().init(&parent);

		let proposer = proposer_future
			.await
			.map_err(|e| error!(target: LOG_TARGET, error = ?e, "Could not create proposer."))
			.ok()?;

		let nimbus_id = NimbusId::from_slice(&type_public_pair)
			.map_err(
				|e| error!(target: LOG_TARGET, error = ?e, "Invalid Nimbus ID (wrong length)."),
			)
			.ok()?;

		let inherent_data = self
			.inherent_data(
				parent.hash(),
				&validation_data,
				relay_parent,
				nimbus_id.clone(),
			)
			.await?;

		let mut logs = vec![CompatibleDigestItem::nimbus_pre_digest(nimbus_id.clone())];
		logs.extend(
			self.additional_digests_provider
				.provide_digests(nimbus_id, parent.hash()),
		);
		let inherent_digests = sp_runtime::generic::Digest { logs };

		let Proposal {
			block,
			storage_changes,
			proof,
		} = proposer
			.propose(
				inherent_data,
				inherent_digests,
				//TODO: Fix this.
				Duration::from_millis(500),
				// Set the block limit to 50% of the maximum PoV size.
				//
				// TODO: If we got benchmarking that includes that encapsulates the proof size,
				// we should be able to use the maximum pov size.
				Some((validation_data.max_pov_size / 2) as usize),
			)
			.await
			.map_err(|e| error!(target: LOG_TARGET, error = ?e, "Proposing failed."))
			.ok()?;

		let (header, extrinsics) = block.clone().deconstruct();

		let sig_digest = seal_header::<B>(
			&header,
			&*self.keystore,
			&type_public_pair,
			&sr25519::CRYPTO_ID,
		);

		let mut block_import_params = BlockImportParams::new(BlockOrigin::Own, header.clone());
		block_import_params.post_digests.push(sig_digest.clone());
		block_import_params.body = Some(extrinsics.clone());
		block_import_params.state_action = sc_consensus::StateAction::ApplyChanges(
			sc_consensus::StorageChanges::Changes(storage_changes),
		);

		// Print the same log line as slots (aura and babe)
		info!(
			"🔖 Sealed block for proposal at {}. Hash now {:?}, previously {:?}.",
			*header.number(),
			block_import_params.post_hash(),
			header.hash(),
		);

		if let Err(err) = self
			.block_import
			.lock()
			.await
			.import_block(block_import_params)
			.await
		{
			error!(
				target: LOG_TARGET,
				at = ?parent.hash(),
				error = ?err,
				"Error importing built block.",
			);

			return None;
		}

		// Compute info about the block after the digest is added
		let mut post_header = header.clone();
		post_header.digest_mut().logs.push(sig_digest.clone());
		let post_block = B::new(post_header, extrinsics);

		// Returning the block WITH the seal for distribution around the network.
		Some(ParachainCandidate {
			block: post_block,
			proof,
		})
	}
}

/// Paramaters of [`build_relay_chain_consensus`].
///
/// I briefly tried the async keystore approach, but decided to go sync so I can copy
/// code from Aura. Maybe after it is working, Jeremy can help me go async.
pub struct BuildNimbusConsensusParams<PF, BI, BE, ParaClient, CIDP, DP> {
	pub para_id: ParaId,
	pub proposer_factory: PF,
	pub create_inherent_data_providers: CIDP,
	pub block_import: BI,
	pub backend: Arc<BE>,
	pub parachain_client: Arc<ParaClient>,
	pub keystore: KeystorePtr,
	pub skip_prediction: bool,
	pub additional_digests_provider: DP,
}
