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

//! Stock, pure Nimbus collators.
//!
//! This includes the [`basic`] collator, which only builds on top of the most recently
//! included parachain block, as well as the [`lookahead`] collator, which prospectively
//! builds on parachain blocks which have not yet been included in the relay chain.

pub mod basic;
pub mod lookahead;
pub mod slot_based;

use crate::{collator::SlotClaim, *};
use async_backing_primitives::Slot;
use async_backing_primitives::UnincludedSegmentApi;
use cumulus_client_collator::service::ServiceInterface as CollatorServiceInterface;
use cumulus_client_consensus_common::{
	ParachainBlockImportMarker, ParachainCandidate, ParentSearchParams, PotentialParent,
};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::{
	relay_chain::{Header as RelayHeader, OccupiedCoreAssumption, ValidationCodeHash},
	ClaimQueueOffset, ParachainBlockData,
};
use cumulus_primitives_parachain_inherent::ParachainInherentData;
use futures::prelude::*;
use log::{debug, info};
use nimbus_primitives::{CompatibleDigestItem, DigestsProvider, NimbusId, NIMBUS_KEY_ID};
use parity_scale_codec::Codec;
use polkadot_node_primitives::{Collation, MaybeCompressedPoV};
use polkadot_node_subsystem::messages::RuntimeApiRequest;
use polkadot_node_subsystem_util::runtime::ClaimQueueSnapshot;
use polkadot_primitives::{CoreIndex, Hash as RelayHash};
use sc_consensus::{BlockImport, BlockImportParams};
use sp_api::{ApiExt, RuntimeApiInfo};
use sp_application_crypto::ByteArray;
use sp_consensus::{BlockOrigin, Proposal};
use sp_core::{crypto::CryptoTypeId, sr25519};
use sp_inherents::InherentData;
use sp_keystore::Keystore;
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	DigestItem,
};
use sp_timestamp::Timestamp;
use std::convert::TryInto;
use std::error::Error;
use std::time::Duration;

// This is an arbitrary value which is likely guaranteed to exceed any reasonable
// limit, as it would correspond to 30 non-included blocks.
//
// Since we only search for parent blocks which have already been imported,
// we can guarantee that all imported blocks respect the unincluded segment
// rules specified by the parachain's runtime and thus will never be too deep. This is just an extra
// sanity check.
const PARENT_SEARCH_DEPTH: usize = 30;

/// Propose, seal, and import a block, packaging it into a collation.
///
/// Provide the slot to build at as well as any other necessary pre-digest logs,
/// the inherent data, and the proposal duration and PoV size limits.
///
/// The Aura pre-digest should not be explicitly provided and is set internally.
///
/// This does not announce the collation to the parachain network or the relay chain.
pub(crate) async fn collate<ADP, Block, BI, CS, Proposer>(
	additional_digests_provider: &ADP,
	author_id: NimbusId,
	block_import: &mut BI,
	collator_service: &CS,
	keystore: &dyn Keystore,
	parent_header: &Block::Header,
	proposer: &mut Proposer,
	inherent_data: (ParachainInherentData, InherentData),
	proposal_duration: Duration,
	max_pov_size: usize,
) -> Result<Option<(Collation, ParachainBlockData<Block>)>, Box<dyn Error + Send + 'static>>
where
	ADP: DigestsProvider<NimbusId, <Block as BlockT>::Hash> + 'static,
	Block: BlockT,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	CS: CollatorServiceInterface<Block>,
	Proposer: ProposerInterface<Block> + Send + Sync + 'static,
{
	let mut logs = vec![CompatibleDigestItem::nimbus_pre_digest(author_id.clone())];
	logs.extend(
		additional_digests_provider.provide_digests(author_id.clone(), parent_header.hash()),
	);

	let maybe_proposal = proposer
		.propose(
			parent_header,
			&inherent_data.0,
			inherent_data.1,
			sp_runtime::generic::Digest { logs },
			proposal_duration,
			Some(max_pov_size),
		)
		.await
		.map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

	let Proposal {
		block,
		storage_changes,
		proof,
	} = match maybe_proposal {
		None => return Ok(None),
		Some(p) => p,
	};

	let (header, extrinsics) = block.clone().deconstruct();

	let sig_digest = seal_header::<Block>(
		&header,
		keystore,
		&author_id.to_raw_vec(),
		&sr25519::CRYPTO_ID,
	);

	let mut block_import_params = BlockImportParams::new(BlockOrigin::Own, header.clone());
	block_import_params.post_digests.push(sig_digest.clone());
	block_import_params.body = Some(extrinsics.clone());
	block_import_params.state_action = sc_consensus::StateAction::ApplyChanges(
		sc_consensus::StorageChanges::Changes(storage_changes),
	);
	// The collator should follow the longest chain
	block_import_params.fork_choice = Some(sc_consensus::ForkChoiceStrategy::LongestChain);

	let post_hash = block_import_params.post_hash();

	// Print the same log line as slots (aura and babe)
	info!(
		"🔖 Sealed block for proposal at {}. Hash now {:?}, previously {:?}.",
		*header.number(),
		&post_hash,
		header.hash(),
	);

	block_import
		.import_block(block_import_params)
		.map_err(|e| Box::new(e) as Box<dyn Error + Send>)
		.await?;

	// Compute info about the block after the digest is added
	let mut post_header = header.clone();
	post_header.digest_mut().logs.push(sig_digest.clone());
	let post_block = Block::new(post_header, extrinsics);

	if let Some((collation, block_data)) = collator_service.build_collation(
		parent_header,
		post_hash,
		ParachainCandidate {
			block: post_block,
			proof,
		},
	) {
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
		Err(
			Box::<dyn Error + Send + Sync>::from("Unable to produce collation")
				as Box<dyn Error + Send>,
		)
	}
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

/// Check the `local_validation_code_hash` against the validation code hash in the relay chain
/// state.
///
/// If the code hashes do not match, it prints a warning.
async fn check_validation_code_or_log(
	local_validation_code_hash: &ValidationCodeHash,
	para_id: ParaId,
	relay_client: &impl RelayChainInterface,
	relay_parent: RelayHash,
) {
	let state_validation_code_hash = match relay_client
		.validation_code_hash(relay_parent, para_id, OccupiedCoreAssumption::Included)
		.await
	{
		Ok(hash) => hash,
		Err(error) => {
			tracing::debug!(
				target: super::LOG_TARGET,
				%error,
				?relay_parent,
				%para_id,
				"Failed to fetch validation code hash",
			);
			return;
		}
	};

	match state_validation_code_hash {
		Some(state) => {
			if state != *local_validation_code_hash {
				tracing::warn!(
					target: super::LOG_TARGET,
					%para_id,
					?relay_parent,
					?local_validation_code_hash,
					relay_validation_code_hash = ?state,
					"Parachain code doesn't match validation code stored in the relay chain state.",
				);
			}
		}
		None => {
			tracing::warn!(
				target: super::LOG_TARGET,
				%para_id,
				?relay_parent,
				"Could not find validation code for parachain in the relay chain state.",
			);
		}
	}
}

/// Holds a relay parent and its descendants.
pub struct RelayParentData {
	/// The relay parent block header
	relay_parent: RelayHeader,
	/// Ordered collection of descendant block headers, from oldest to newest
	descendants: Vec<RelayHeader>,
}

impl RelayParentData {
	/// Creates a new instance with the given relay parent and no descendants.
	pub fn new(relay_parent: RelayHeader) -> Self {
		Self {
			relay_parent,
			descendants: Default::default(),
		}
	}

	/// Creates a new instance with the given relay parent and descendants.
	pub fn new_with_descendants(relay_parent: RelayHeader, descendants: Vec<RelayHeader>) -> Self {
		Self {
			relay_parent,
			descendants,
		}
	}

	/// Returns a reference to the relay parent header.
	pub fn relay_parent(&self) -> &RelayHeader {
		&self.relay_parent
	}

	/// Returns the number of descendants.
	#[cfg(test)]
	pub fn descendants_len(&self) -> usize {
		self.descendants.len()
	}

	/// Consumes the structure and returns a vector containing the relay parent followed by its
	/// descendants in chronological order. The resulting list should be provided to the parachain
	/// inherent data.
	pub fn into_inherent_descendant_list(self) -> Vec<RelayHeader> {
		let Self {
			relay_parent,
			mut descendants,
		} = self;

		if descendants.is_empty() {
			return Default::default();
		}

		let mut result = vec![relay_parent];
		result.append(&mut descendants);
		result
	}
}

// Return all the cores assigned to the para at the provided relay parent, using the claim queue
// offset.
// Will return an empty vec if the provided offset is higher than the claim queue length (which
// corresponds to the scheduling_lookahead on the relay chain).
async fn cores_scheduled_for_para(
	relay_parent: RelayHash,
	para_id: ParaId,
	relay_client: &impl RelayChainInterface,
	claim_queue_offset: ClaimQueueOffset,
) -> Vec<CoreIndex> {
	// Get `ClaimQueue` from runtime
	let claim_queue: ClaimQueueSnapshot = match relay_client.claim_queue(relay_parent).await {
		Ok(claim_queue) => claim_queue.into(),
		Err(error) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?error,
				?relay_parent,
				"Failed to query claim queue runtime API",
			);
			return Vec::new();
		}
	};

	claim_queue
		.iter_claims_at_depth(claim_queue_offset.0 as usize)
		.filter_map(|(core_index, core_para_id)| (core_para_id == para_id).then_some(core_index))
		.collect()
}

/// Fetch scheduling lookahead at given relay parent.
async fn scheduling_lookahead(
	relay_parent: RelayHash,
	relay_client: &impl RelayChainInterface,
) -> Option<u32> {
	let runtime_api_version = relay_client
		.version(relay_parent)
		.await
		.map_err(|e| {
			tracing::error!(
				target: super::LOG_TARGET,
				error = ?e,
				"Failed to fetch relay chain runtime version.",
			)
		})
		.ok()?;

	let parachain_host_runtime_api_version = runtime_api_version
		.api_version(
			&<dyn polkadot_primitives::runtime_api::ParachainHost<polkadot_primitives::Block>>::ID,
		)
		.unwrap_or_default();

	if parachain_host_runtime_api_version
		< RuntimeApiRequest::SCHEDULING_LOOKAHEAD_RUNTIME_REQUIREMENT
	{
		return None;
	}

	match relay_client.scheduling_lookahead(relay_parent).await {
		Ok(scheduling_lookahead) => Some(scheduling_lookahead),
		Err(err) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?err,
				?relay_parent,
				"Failed to fetch scheduling lookahead from relay chain",
			);
			None
		}
	}
}

/// Use [`cumulus_client_consensus_common::find_potential_parents`] to find parachain blocks that
/// we can build on. Once a list of potential parents is retrieved, return the last one of the
/// longest chain.
async fn find_parent<Block>(
	relay_parent: RelayHash,
	para_id: ParaId,
	para_backend: &impl sc_client_api::Backend<Block>,
	relay_client: &impl RelayChainInterface,
) -> Option<(<Block as BlockT>::Header, PotentialParent<Block>)>
where
	Block: BlockT,
{
	let parent_search_params = ParentSearchParams {
		relay_parent,
		para_id,
		ancestry_lookback: scheduling_lookahead(relay_parent, relay_client)
			.await
			.unwrap_or(polkadot_primitives::DEFAULT_SCHEDULING_LOOKAHEAD)
			.saturating_sub(1) as usize,
		max_depth: PARENT_SEARCH_DEPTH,
		ignore_alternative_branches: true,
	};

	let potential_parents = cumulus_client_consensus_common::find_potential_parents::<Block>(
		parent_search_params,
		para_backend,
		relay_client,
	)
	.await;

	let potential_parents = match potential_parents {
		Err(e) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?relay_parent,
				err = ?e,
				"Could not fetch potential parents to build upon"
			);

			return None;
		}
		Ok(x) => x,
	};

	let included_block = potential_parents
		.iter()
		.find(|x| x.depth == 0)?
		.header
		.clone();
	potential_parents
		.into_iter()
		.max_by_key(|a| a.depth)
		.map(|parent| (included_block, parent))
}

// Checks if we own the slot at the given block and whether there
// is space in the unincluded segment.
async fn can_build_upon<Block: BlockT, Client, P>(
	para_slot: Slot,
	relay_slot: Slot,
	timestamp: Timestamp,
	relay_parent: PHeader,
	parent_header: Block::Header,
	included_block: Block::Hash,
	client: &Client,
	keystore: &KeystorePtr,
	force_authoring: bool,
) -> Option<SlotClaim>
where
	Client: ProvideRuntimeApi<Block>,
	Client::Api: NimbusApi<Block> + UnincludedSegmentApi<Block> + ApiExt<Block>,
	P: sp_core::Pair<Public = NimbusId>,
	P::Public: Codec,
	P::Signature: Codec,
{
	let runtime_api = client.runtime_api();
	let author_pub = crate::claim_slot::<Block, Client>(
		keystore,
		client,
		&parent_header,
		&relay_parent,
		force_authoring,
	)
	.await
	.ok()
	.flatten()?;

	let parent_hash = parent_header.hash();

	// This function is typically called when we want to build block N. At that point, the
	// unincluded segment in the runtime is unaware of the hash of block N-1. If the unincluded
	// segment in the runtime is full, but block N-1 is the included block, the unincluded segment
	// should have length 0 and we can build. Since the hash is not available to the runtime
	// however, we need this extra check here.
	if parent_hash == included_block {
		return Some(SlotClaim::unchecked::<P>(author_pub, timestamp));
	}

	let api_version = runtime_api
		.api_version::<dyn UnincludedSegmentApi<Block>>(parent_hash)
		.ok()
		.flatten()?;

	let slot = if api_version > 1 {
		relay_slot
	} else {
		para_slot
	};

	runtime_api
		.can_build_upon(parent_hash, included_block, slot)
		.ok()?
		.then(|| SlotClaim::unchecked::<P>(author_pub, timestamp))
}
