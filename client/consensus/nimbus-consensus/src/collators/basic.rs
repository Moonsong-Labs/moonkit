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

use crate::*;
use cumulus_client_collator::service::ServiceInterface as CollatorServiceInterface;
use cumulus_client_consensus_common::{self as consensus_common, ParachainBlockImportMarker};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::{
	relay_chain::{BlockId as RBlockId, Hash as PHash, ValidationCode},
	CollectCollationInfo, ParaId, PersistedValidationData,
};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use futures::prelude::*;
use nimbus_primitives::{DigestsProvider, NimbusApi, NimbusId};
use polkadot_node_primitives::CollationResult;
use polkadot_primitives::CollatorPair;
use sc_client_api::{AuxStore, BlockBackend, BlockOf, StateBackend};
use sp_api::{CallApiAt, ProvideRuntimeApi};
use sp_blockchain::HeaderBackend;
use sp_consensus_slots::{Slot, SlotDuration};
use sp_core::Decode;
use sp_inherents::CreateInherentDataProviders;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
use std::{sync::Arc, time::Duration};

/// Parameters for [`run`].
pub struct Params<Proposer, BI, ParaClient, RClient, CIDP, CS, ADP = ()> {
	/// Additional digest provider
	pub additional_digests_provider: ADP,
	/// Parachain id
	pub para_id: ParaId,
	/// A handle to the relay-chain client's "Overseer" or task orchestrator.
	pub overseer_handle: OverseerHandle,
	/// The length of slots in the relay chain.
	pub relay_chain_slot_duration: Duration,
	/// The length of slots in this parachain.
	/// If the parachain doesn't have slot and rely only on relay slots, set it to None.
	pub slot_duration: Option<SlotDuration>,
	/// The underlying block proposer this should call into.
	pub proposer: Proposer,
	/// The block import handle.
	pub block_import: BI,
	/// The underlying para client.
	pub para_client: Arc<ParaClient>,
	/// An interface to the relay-chain client.
	pub relay_client: RClient,
	/// The underlying keystore, which should contain Nimbus consensus keys.
	pub keystore: KeystorePtr,
	/// The collator key used to sign collations before submitting to validators.
	pub collator_key: CollatorPair,
	/// Force production of the block even if the collator is not eligible
	pub force_authoring: bool,
	/// Maximum percentage of POV size to use (0-85)
	pub max_pov_percentage: u8,
	/// A builder for inherent data builders.
	pub create_inherent_data_providers: CIDP,
	/// The collator service used for bundling proposals into collations and announcing
	/// to the network.
	pub collator_service: CS,
	/// Additional relay keys to add in the storage proof
	pub additional_relay_keys: Vec<Vec<u8>>,
}

/// Run bare Nimbus consensus as a relay-chain-driven collator.
pub fn run<Block, BI, CIDP, Backend, Client, RClient, Proposer, CS, ADP>(
	params: Params<Proposer, BI, Client, RClient, CIDP, CS, ADP>,
) -> impl Future<Output = ()> + Send + 'static
where
	Block: BlockT + Send,
	CIDP: CreateInherentDataProviders<Block, (PHash, PersistedValidationData, NimbusId)> + 'static,
	CIDP::InherentDataProviders: Send,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	Client: ProvideRuntimeApi<Block>
		+ BlockOf
		+ AuxStore
		+ HeaderBackend<Block>
		+ BlockBackend<Block>
		+ CallApiAt<Block>
		+ Send
		+ Sync
		+ 'static,
	Client::Api: NimbusApi<Block> + CollectCollationInfo<Block>,
	RClient: RelayChainInterface + Send + Clone + 'static,
	Proposer: ProposerInterface<Block> + Send + Sync + 'static,
	CS: CollatorServiceInterface<Block> + Send + Sync + 'static,
	ADP: DigestsProvider<NimbusId, <Block as BlockT>::Hash> + Send + Sync + 'static,
{
	async move {
		let mut collation_requests = cumulus_client_collator::relay_chain_driven::init(
			params.collator_key,
			params.para_id,
			params.overseer_handle,
		)
		.await;

		let Params {
			additional_digests_provider,
			mut block_import,
			collator_service,
			create_inherent_data_providers,
			keystore,
			para_id,
			mut proposer,
			para_client,
			relay_client,
			force_authoring,
			max_pov_percentage,
			..
		} = params;

		let mut last_processed_slot = 0;
		let mut last_relay_chain_block = Default::default();

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

			if !collator_service.check_block_status(parent_hash, &parent_header) {
				continue;
			}

			let Ok(Some(code)) = para_client
				.state_at(parent_hash)
				.map_err(drop)
				.and_then(|s| {
					s.storage(&sp_core::storage::well_known_keys::CODE)
						.map_err(drop)
				})
			else {
				continue;
			};

			super::check_validation_code_or_log(
				&ValidationCode::from(code).hash(),
				para_id,
				&relay_client,
				*request.relay_parent(),
			)
			.await;

			let relay_parent_header = match relay_client
				.header(RBlockId::hash(*request.relay_parent()))
				.await
			{
				Err(e) => reject_with_error!(e),
				Ok(None) => continue, // sanity: would be inconsistent to get `None` here
				Ok(Some(h)) => h,
			};

			let nimbus_id = match claim_slot::<Block, Client>(
				&keystore,
				&para_client,
				&parent_header,
				&relay_parent_header,
				force_authoring,
			)
			.await
			{
				Ok(None) => continue,
				Ok(Some(nimbus_id)) => nimbus_id,
				Err(e) => reject_with_error!(e),
			};

			// Determine which is the current slot
			let (slot_now, timestamp) = match consensus_common::relay_slot_and_timestamp(
				&relay_parent_header,
				params.relay_chain_slot_duration,
			) {
				None => {
					tracing::trace!(
						target: crate::LOG_TARGET,
						relay_parent = ?relay_parent_header,
						relay_chain_slot_duration = ?params.relay_chain_slot_duration,
						"Fail to get the relay slot for this relay block!"
					);
					continue;
				}
				Some((relay_slot, relay_timestamp)) => {
					let our_slot = if let Some(slot_duration) = params.slot_duration {
						Slot::from_timestamp(relay_timestamp, slot_duration)
					} else {
						// If there is no slot duration, we assume that the parachain use the relay slot directly
						relay_slot
					};
					tracing::debug!(
						target: crate::LOG_TARGET,
						?relay_slot,
						para_slot = ?our_slot,
						?relay_timestamp,
						slot_duration = ?params.slot_duration,
						relay_chain_slot_duration = ?params.relay_chain_slot_duration,
						"Adjusted relay-chain slot to parachain slot"
					);
					(our_slot, relay_timestamp)
				}
			};

			// With async backing this function will be called every relay chain block.
			//
			// Most parachains currently run with 12 seconds slots and thus, they would try to
			// produce multiple blocks per slot which very likely would fail on chain. Thus, we have
			// this "hack" to only produce one block per slot per relay chain fork.
			//
			// With https://github.com/paritytech/polkadot-sdk/issues/3168 this implementation will be
			// obsolete and also the underlying issue will be fixed.
			if last_processed_slot >= *slot_now
				&& last_relay_chain_block < *relay_parent_header.number()
			{
				continue;
			}

			let inherent_data = try_request!(
				create_inherent_data(
					&create_inherent_data_providers,
					para_id,
					parent_header.hash(),
					validation_data,
					&relay_client,
					*request.relay_parent(),
					nimbus_id.clone(),
					Some(timestamp),
					params.additional_relay_keys.clone(),
				)
				.await
			);

			let allowed_pov_size = {
				// Cap the percentage at 85% (see https://github.com/paritytech/polkadot-sdk/issues/6020)
				let capped_percentage = max_pov_percentage.min(85);
				// Calculate the allowed POV size based on the percentage
				(validation_data.max_pov_size as u128)
					.saturating_mul(capped_percentage as u128)
					.saturating_div(100) as usize
			};

			let maybe_collation = try_request!(
				super::collate::<ADP, Block, BI, CS, Proposer>(
					&additional_digests_provider,
					nimbus_id,
					&mut block_import,
					&collator_service,
					&*keystore,
					&parent_header,
					&mut proposer,
					inherent_data,
					Duration::from_millis(500), //params.authoring_duration,
					allowed_pov_size,
				)
				.await
			);

			if let Some((collation, _, post_hash)) = maybe_collation {
				let result_sender = Some(collator_service.announce_with_barrier(post_hash));
				request.complete(Some(CollationResult {
					collation,
					result_sender,
				}));
			} else {
				request.complete(None);
				tracing::debug!(target: crate::LOG_TARGET, "No block proposal");
			}

			last_processed_slot = *slot_now;
			last_relay_chain_block = *relay_parent_header.number();
		}
	}
}
