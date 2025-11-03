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
//! included parachain block, the [`lookahead`] collator, which prospectively builds on
//! parachain blocks which have not yet been included in the relay chain, and the
//! [`slot_based`] collator, which uses a slot-based block production approach with a
//! separate block builder task that is timed by the parachain's slot duration.

pub mod basic;
pub mod lookahead;
pub mod slot_based;

use crate::{collator::SlotClaim, *};
use async_backing_primitives::Slot;
use async_backing_primitives::UnincludedSegmentApi;
use cumulus_client_consensus_common::{ParentSearchParams, PotentialParent};
use cumulus_primitives_core::{
	relay_chain::{Header as RelayHeader, OccupiedCoreAssumption, ValidationCodeHash},
	ParaId,
};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use log::debug;
use nimbus_primitives::{CompatibleDigestItem, NimbusId, NIMBUS_KEY_ID};
use parity_scale_codec::Codec;
use polkadot_node_subsystem::messages::{CollatorProtocolMessage, RuntimeApiRequest};
use polkadot_node_subsystem_util::runtime::ClaimQueueSnapshot;
use polkadot_primitives::Hash as RelayHash;
use sp_api::{ApiExt, RuntimeApiInfo};
use sp_core::crypto::CryptoTypeId;
use sp_keystore::{Keystore, KeystorePtr};
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	DigestItem,
};
use sp_timestamp::Timestamp;
use std::convert::TryInto;

// This is an arbitrary value which is likely guaranteed to exceed any reasonable
// limit, as it would correspond to 30 non-included blocks.
//
// Since we only search for parent blocks which have already been imported,
// we can guarantee that all imported blocks respect the unincluded segment
// rules specified by the parachain's runtime and thus will never be too deep. This is just an extra
// sanity check.
const PARENT_SEARCH_DEPTH: usize = 30;

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

/// Helper for managing pre-connections to backing groups.
///
/// This ensures collators establish connections to backing groups proactively,
/// even in single-collator scenarios where the collator always owns the slot.
///
/// For Nimbus consensus, since there's no round-robin authority rotation like Aura,
/// we simply check if we have any Nimbus keys and pre-connect if so.
pub(crate) struct BackingGroupConnectionHelper {
	keystore: KeystorePtr,
	overseer_handle: OverseerHandle,
	/// Track the last slot we connected for to avoid sending duplicate messages.
	last_slot: Option<Slot>,
	/// Whether we are currently connected.
	connected: bool,
}

impl BackingGroupConnectionHelper {
	/// Create a new [`BackingGroupConnectionHelper`].
	pub fn new(keystore: KeystorePtr, overseer_handle: OverseerHandle) -> Self {
		Self {
			keystore,
			overseer_handle,
			last_slot: None,
			connected: false,
		}
	}

	/// Update the connection helper with the current slot.
	///
	/// For Nimbus, we check if we have any Nimbus keys and pre-connect if so.
	/// This is simpler than Aura since Nimbus doesn't have round-robin slot assignment.
	pub async fn update(&mut self, current_slot: Slot) {
		// If we already processed this slot, skip
		if self.last_slot == Some(current_slot) {
			return;
		}
		self.last_slot = Some(current_slot);

		// Check if we have any Nimbus keys
		let has_keys = self
			.keystore
			.keys(NIMBUS_KEY_ID)
			.map(|keys| !keys.is_empty())
			.unwrap_or(false);

		if has_keys && !self.connected {
			tracing::debug!(
				target: crate::LOG_TARGET,
				?current_slot,
				"Pre-connecting to backing groups",
			);
			self.overseer_handle
				.send_msg(
					CollatorProtocolMessage::ConnectToBackingGroups,
					"BackingGroupConnectionHelper",
				)
				.await;
			self.connected = true;
		} else if !has_keys && self.connected {
			tracing::debug!(
				target: crate::LOG_TARGET,
				?current_slot,
				"Disconnecting from backing groups - no keys",
			);
			self.overseer_handle
				.send_msg(
					CollatorProtocolMessage::DisconnectFromBackingGroups,
					"BackingGroupConnectionHelper",
				)
				.await;
			self.connected = false;
		}
	}
}

// Returns the claim queue at the given relay parent.
async fn claim_queue_at(
	relay_parent: RelayHash,
	relay_client: &impl RelayChainInterface,
) -> ClaimQueueSnapshot {
	// Get `ClaimQueue` from runtime
	match relay_client.claim_queue(relay_parent).await {
		Ok(claim_queue) => claim_queue.into(),
		Err(error) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?error,
				?relay_parent,
				"Failed to query claim queue runtime API",
			);
			Default::default()
		}
	}
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
