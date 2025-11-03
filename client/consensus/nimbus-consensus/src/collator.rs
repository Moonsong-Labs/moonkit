use cumulus_client_collator::service::ServiceInterface as CollatorServiceInterface;
use cumulus_client_consensus_common::{ParachainBlockImportMarker, ParachainCandidate};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_client_parachain_inherent::{ParachainInherentData, ParachainInherentDataProvider};
use cumulus_primitives_core::{
	relay_chain::Hash as PHash, DigestItem, ParachainBlockData, PersistedValidationData,
};
use cumulus_relay_chain_interface::RelayChainInterface;
use parity_scale_codec::Codec;

use polkadot_node_primitives::{Collation, MaybeCompressedPoV};
use polkadot_primitives::Id as ParaId;

use crate::collators::RelayParentData;
use futures::prelude::*;
use nimbus_primitives::{CompatibleDigestItem, NimbusId};
use sc_consensus::{BlockImport, BlockImportParams, ForkChoiceStrategy, StateAction};
use sp_application_crypto::AppPublic;
use sp_consensus::BlockOrigin;
use sp_core::{crypto::Pair, ByteArray};
use sp_inherents::{CreateInherentDataProviders, InherentData, InherentDataProvider};
use sp_keystore::KeystorePtr;
use sp_runtime::{
	generic::Digest,
	traits::{Block as BlockT, HashingFor, Header as HeaderT, Member},
};
use sp_state_machine::StorageChanges;
use sp_timestamp::Timestamp;
use std::{error::Error, time::Duration};

/// Parameters for instantiating a [`Collator`].
pub struct Params<BI, CIDP, RClient, Proposer, CS> {
	/// A builder for inherent data builders.
	pub create_inherent_data_providers: CIDP,
	/// The block import handle.
	pub block_import: BI,
	/// An interface to the relay-chain client.
	pub relay_client: RClient,
	/// The keystore handle used for accessing parachain key material.
	pub keystore: KeystorePtr,
	/// The identifier of the parachain within the relay-chain.
	pub para_id: ParaId,
	/// The block proposer used for building blocks.
	pub proposer: Proposer,
	/// The collator service used for bundling proposals into collations and announcing
	/// to the network.
	pub collator_service: CS,
}

/// A utility struct for writing collation logic that makes use of Aura entirely
/// or in part. See module docs for more details.
pub struct Collator<Block, P, BI, CIDP, RClient, Proposer, CS> {
	create_inherent_data_providers: CIDP,
	block_import: BI,
	relay_client: RClient,
	keystore: KeystorePtr,
	para_id: ParaId,
	proposer: Proposer,
	collator_service: CS,
	_marker: std::marker::PhantomData<(Block, Box<dyn Fn(P) + Send + Sync + 'static>)>,
}

impl<Block, P, BI, CIDP, RClient, Proposer, CS> Collator<Block, P, BI, CIDP, RClient, Proposer, CS>
where
	Block: BlockT,
	RClient: RelayChainInterface,
	CIDP: CreateInherentDataProviders<Block, ()> + 'static,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	Proposer: ProposerInterface<Block>,
	CS: CollatorServiceInterface<Block>,
	P: Pair<Public = NimbusId>,
	P::Public: AppPublic + Member,
	P::Signature: TryFrom<Vec<u8>> + Member + Codec,
{
	/// Instantiate a new instance of the `Aura` manager.
	pub fn new(params: Params<BI, CIDP, RClient, Proposer, CS>) -> Self {
		Collator {
			create_inherent_data_providers: params.create_inherent_data_providers,
			block_import: params.block_import,
			relay_client: params.relay_client,
			keystore: params.keystore,
			para_id: params.para_id,
			proposer: params.proposer,
			collator_service: params.collator_service,
			_marker: std::marker::PhantomData,
		}
	}

	/// Explicitly creates the inherent data for parachain block authoring and overrides
	/// the timestamp inherent data with the one provided, if any. Additionally allows to specify
	/// relay parent descendants that can be used to prevent authoring at the tip of the relay
	/// chain.
	pub async fn create_inherent_data_with_rp_offset(
		&self,
		relay_parent: PHash,
		validation_data: &PersistedValidationData,
		parent_hash: Block::Hash,
		timestamp: impl Into<Option<Timestamp>>,
		relay_parent_descendants: Option<RelayParentData>,
		additional_relay_state_keys: Vec<Vec<u8>>,
	) -> Result<(ParachainInherentData, InherentData), Box<dyn Error + Send + Sync + 'static>> {
		let paras_inherent_data = ParachainInherentDataProvider::create_at(
			relay_parent,
			&self.relay_client,
			validation_data,
			self.para_id,
			relay_parent_descendants
				.map(RelayParentData::into_inherent_descendant_list)
				.unwrap_or_default(),
			additional_relay_state_keys,
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

		let mut other_inherent_data = self
			.create_inherent_data_providers
			.create_inherent_data_providers(parent_hash, ())
			.map_err(|e| e as Box<dyn Error + Send + Sync + 'static>)
			.await?
			.create_inherent_data()
			.await
			.map_err(Box::new)?;

		if let Some(timestamp) = timestamp.into() {
			other_inherent_data.replace_data(sp_timestamp::INHERENT_IDENTIFIER, &timestamp);
		}

		Ok((paras_inherent_data, other_inherent_data))
	}

	/// Explicitly creates the inherent data for parachain block authoring and overrides
	/// the timestamp inherent data with the one provided, if any.
	pub async fn create_inherent_data(
		&self,
		relay_parent: PHash,
		validation_data: &PersistedValidationData,
		parent_hash: Block::Hash,
		timestamp: impl Into<Option<Timestamp>>,
		additional_relay_state_keys: Vec<Vec<u8>>,
	) -> Result<(ParachainInherentData, InherentData), Box<dyn Error + Send + Sync + 'static>> {
		self.create_inherent_data_with_rp_offset(
			relay_parent,
			validation_data,
			parent_hash,
			timestamp,
			None,
			additional_relay_state_keys,
		)
		.await
	}

	/// Build and import a parachain block on the given parent header, using the given slot claim.
	pub async fn build_block_and_import(
		&mut self,
		parent_header: &Block::Header,
		slot_claim: &SlotClaim,
		additional_pre_digest: impl Into<Option<Vec<DigestItem>>>,
		inherent_data: (ParachainInherentData, InherentData),
		proposal_duration: Duration,
		max_pov_size: usize,
	) -> Result<Option<ParachainCandidate<Block>>, Box<dyn Error + Send + 'static>> {
		//let mut digest = additional_pre_digest.into().unwrap_or_default();
		//digest.push(slot_claim.pre_digest.clone());
		//log::error!("digest: {:?}", digest);
		let mut logs = vec![CompatibleDigestItem::nimbus_pre_digest(
			slot_claim.author_id.clone(),
		)];
		logs.extend(additional_pre_digest.into().unwrap_or_default());

		let maybe_proposal = self
			.proposer
			.propose(
				&parent_header,
				&inherent_data.0,
				inherent_data.1,
				Digest { logs },
				proposal_duration,
				Some(max_pov_size),
			)
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

		let proposal = match maybe_proposal {
			None => return Ok(None),
			Some(p) => p,
		};

		let sealed_importable = seal::<_, P>(
			proposal.block,
			proposal.storage_changes,
			&slot_claim.author_id,
			&self.keystore,
		)
		.map_err(|e| e as Box<dyn Error + Send>)?;

		let block = Block::new(
			sealed_importable.post_header(),
			sealed_importable
				.body
				.as_ref()
				.expect("body always created with this `propose` fn; qed")
				.clone(),
		);

		self.block_import
			.import_block(sealed_importable)
			.map_err(|e| Box::new(e) as Box<dyn Error + Send>)
			.await?;

		Ok(Some(ParachainCandidate {
			block,
			proof: proposal.proof,
		}))
	}

	/// Propose, seal, import a block and packaging it into a collation.
	///
	/// Provide the slot to build at as well as any other necessary pre-digest logs,
	/// the inherent data, and the proposal duration and PoV size limits.
	///
	/// The Nimbust pre-digest should not be explicitly provided and is set internally.
	///
	/// This does not announce the collation to the parachain network or the relay chain.
	pub async fn collate(
		&mut self,
		parent_header: &Block::Header,
		slot_claim: &SlotClaim,
		additional_pre_digest: impl Into<Option<Vec<DigestItem>>>,
		inherent_data: (ParachainInherentData, InherentData),
		proposal_duration: Duration,
		max_pov_size: usize,
	) -> Result<Option<(Collation, ParachainBlockData<Block>)>, Box<dyn Error + Send + 'static>> {
		let maybe_candidate = self
			.build_block_and_import(
				parent_header,
				slot_claim,
				additional_pre_digest,
				inherent_data,
				proposal_duration,
				max_pov_size,
			)
			.await?;

		let Some(candidate) = maybe_candidate else {
			return Ok(None);
		};

		let hash = candidate.block.header().hash();
		if let Some((collation, block_data)) =
			self.collator_service
				.build_collation(parent_header, hash, candidate)
		{
			block_data.log_size_info();

			if let MaybeCompressedPoV::Compressed(ref pov) = collation.proof_of_validity {
				tracing::info!(
					target: crate::LOG_TARGET,
					"Compressed PoV size: {}kb",
					pov.block_data.0.len() as f64 / 1024f64,
				);
			}

			Ok(Some((collation, block_data)))
		} else {
			Err(Box::<dyn Error + Send + Sync>::from(
				"Unable to produce collation",
			))
		}
	}

	/// Get the underlying collator service.
	pub fn collator_service(&self) -> &CS {
		&self.collator_service
	}
}

/// A claim on an Aura slot.
pub struct SlotClaim {
	author_id: NimbusId,
	pre_digest: DigestItem,
	timestamp: Timestamp,
}

impl SlotClaim {
	/// Create a slot-claim from the given author public key, slot, and timestamp.
	///
	/// This does not check whether the author actually owns the slot or the timestamp
	/// falls within the slot.
	pub fn unchecked<P>(author_id: NimbusId, timestamp: Timestamp) -> Self
	where
		P: Pair<Public = NimbusId>,
		P::Public: Codec,
		P::Signature: Codec,
	{
		let pre_digest = CompatibleDigestItem::nimbus_pre_digest(author_id.clone());
		SlotClaim {
			author_id,
			timestamp,
			pre_digest,
		}
	}

	/// Get the timestamp corresponding to the relay-chain slot this claim was
	/// generated against.
	pub fn timestamp(&self) -> Timestamp {
		self.timestamp
	}

	/// Get the timestamp corresponding to the relay-chain slot this claim was
	/// generated against.
	pub fn author_id(&self) -> NimbusId {
		self.author_id.clone()
	}
}

/*

/// Attempt to claim a slot derived from the given relay-parent header's slot.
pub async fn claim_slot<B, C, P>(
	client: &C,
	parent_hash: B::Hash,
	relay_parent_header: &PHeader,
	slot_duration: SlotDuration,
	relay_chain_slot_duration: Duration,
	keystore: &KeystorePtr,
) -> Result<Option<SlotClaim<P::Public>>, Box<dyn Error>>
where
	B: BlockT,
	C: ProvideRuntimeApi<B> + Send + Sync + 'static,
	C::Api: NimbusApi<B>,
	P: Pair,
	P::Public: Codec,
	P::Signature: Codec,
{
	// load authorities
	let authorities = client.runtime_api().authorities(parent_hash).map_err(Box::new)?;

	// Determine the current slot and timestamp based on the relay-parent's.
	let (slot_now, timestamp) = match consensus_common::relay_slot_and_timestamp(
		relay_parent_header,
		relay_chain_slot_duration,
	) {
		Some((r_s, t)) => {
			let our_slot = Slot::from_timestamp(t, slot_duration);
			tracing::debug!(
				target: crate::LOG_TARGET,
				relay_slot = ?r_s,
				para_slot = ?our_slot,
				timestamp = ?t,
				?slot_duration,
				?relay_chain_slot_duration,
				"Adjusted relay-chain slot to parachain slot"
			);
			(our_slot, t)
		},
		None => return Ok(None),
	};

	// Try to claim the slot locally.
	let author_pub = {
		let res = aura_internal::claim_slot::<P>(slot_now, &authorities, keystore).await;
		match res {
			Some(p) => p,
			None => return Ok(None),
		}
	};

	Ok(Some(SlotClaim::unchecked::<P>(author_pub, slot_now, timestamp)))
}

*/

/// Seal a block with a signature in the header.
pub fn seal<B: BlockT, P>(
	pre_sealed: B,
	storage_changes: StorageChanges<HashingFor<B>>,
	author_pub: &P::Public,
	keystore: &KeystorePtr,
) -> Result<BlockImportParams<B>, Box<dyn Error + Send + Sync + 'static>>
where
	P: Pair<Public = NimbusId>,
	P::Signature: Codec + TryFrom<Vec<u8>>,
	P::Public: AppPublic,
{
	let (pre_header, body) = pre_sealed.deconstruct();
	let pre_hash = pre_header.hash();
	let block_number = *pre_header.number();

	// seal the block.
	let block_import_params = {
		//let seal_digest = aura_internal::seal::<_, P>(&pre_hash, &author_pub, keystore).map_err(Box::new)?;
		let seal_digest = crate::collators::seal_header::<B>(
			&pre_header,
			keystore,
			&author_pub.to_raw_vec(),
			&sp_core::sr25519::CRYPTO_ID,
		);
		let mut block_import_params = BlockImportParams::new(BlockOrigin::Own, pre_header);
		block_import_params.post_digests.push(seal_digest);
		block_import_params.body = Some(body);
		block_import_params.state_action =
			StateAction::ApplyChanges(sc_consensus::StorageChanges::Changes(storage_changes));
		block_import_params.fork_choice = Some(ForkChoiceStrategy::LongestChain);
		block_import_params
	};
	let post_hash = block_import_params.post_hash();

	tracing::info!(
		target: crate::LOG_TARGET,
		"🔖 Pre-sealed block for proposal at {}. Hash now {:?}, previously {:?}.",
		block_number,
		post_hash,
		pre_hash,
	);

	Ok(block_import_params)
}
