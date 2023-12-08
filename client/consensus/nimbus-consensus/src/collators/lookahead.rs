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
use async_backing_primitives::UnincludedSegmentApi;
use cumulus_client_collator::service::ServiceInterface as CollatorServiceInterface;
use cumulus_client_consensus_common::{
	self as consensus_common, load_abridged_host_configuration, ParachainBlockImportMarker,
	ParentSearchParams,
};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::{relay_chain::Hash as PHash, CollectCollationInfo, ParaId};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use futures::{channel::oneshot, prelude::*};
use nimbus_primitives::{DigestsProvider, NimbusApi, NimbusId};
use polkadot_node_primitives::SubmitCollationParams;
use polkadot_node_subsystem::messages::{
	CollationGenerationMessage, RuntimeApiMessage, RuntimeApiRequest,
};
use polkadot_primitives::{CollatorPair, OccupiedCoreAssumption};
use sc_client_api::{BlockBackend, BlockOf};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_consensus::SyncOracle;
use sp_consensus_slots::{Slot, SlotDuration};
use sp_core::Encode;
use sp_inherents::CreateInherentDataProviders;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
use std::{sync::Arc, time::Duration};

/// Parameters for [`run`].
pub struct Params<BI, CIDP, Client, Backend, RClient, CHP, SO, Proposer, CS, DP = ()> {
	/// Additional digest provider
	pub additional_digests_provider: DP,
	/// The amount of time to spend authoring each block.
	pub authoring_duration: Duration,
	/// Used to actually import blocks.
	pub block_import: BI,
	/// A validation code hash provider, used to get the current validation code hash.
	pub code_hash_provider: CHP,
	/// The collator key used to sign collations before submitting to validators.
	pub collator_key: CollatorPair,
	/// The generic collator service used to plug into this consensus engine.
	pub collator_service: CS,
	/// Inherent data providers. Only non-consensus inherent data should be provided, i.e.
	/// the timestamp, slot, and paras inherents should be omitted, as they are set by this
	/// collator.
	pub create_inherent_data_providers: CIDP,
	/// The underlying keystore, which should contain Aura consensus keys.
	pub keystore: KeystorePtr,
	/// A handle to the relay-chain client's "Overseer" or task orchestrator.
	pub overseer_handle: OverseerHandle,
	/// The underlying para client.
	pub para_client: Arc<Client>,
	/// The para's ID.
	pub para_id: ParaId,
	/// The para client's backend, used to access the database.
	pub para_backend: Arc<Backend>,
	/// The underlying block proposer this should call into.
	pub proposer: Proposer,
	/// A handle to the relay-chain client.
	pub relay_client: RClient,
	/// A chain synchronization oracle.
	pub sync_oracle: SO,
	/// The length of slots in this parachain.
	/// If the parachain doesn't have slot and rely only on relay slots, set it to None.
	pub slot_duration: Option<SlotDuration>,
	/// The length of slots in the relay chain.
	pub relay_chain_slot_duration: Duration,
}

/// Run async-backing-friendly collator.
pub fn run<Block, P, BI, CIDP, Client, Backend, RClient, CHP, SO, Proposer, CS, DP>(
	mut params: Params<BI, CIDP, Client, Backend, RClient, CHP, SO, Proposer, CS, DP>,
) -> impl Future<Output = ()> + Send + 'static
where
	Block: BlockT,
	Client: ProvideRuntimeApi<Block>
		+ BlockOf
		+ HeaderBackend<Block>
		+ BlockBackend<Block>
		+ Send
		+ Sync
		+ 'static,
	Client::Api: NimbusApi<Block> + CollectCollationInfo<Block> + UnincludedSegmentApi<Block>,
	Backend: sc_client_api::Backend<Block> + 'static,
	RClient: RelayChainInterface + Clone + 'static,
	CIDP: CreateInherentDataProviders<Block, (PHash, PersistedValidationData, NimbusId)> + 'static,
	CIDP::InherentDataProviders: Send,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	SO: SyncOracle + Send + Sync + Clone + 'static,
	Proposer: ProposerInterface<Block> + Send + Sync + 'static,
	CS: CollatorServiceInterface<Block> + Send + Sync + 'static,
	CHP: consensus_common::ValidationCodeHashProvider<Block::Hash> + Send + 'static,
	DP: DigestsProvider<NimbusId, <Block as BlockT>::Hash> + Send + Sync + 'static,
{
	// This is an arbitrary value which is likely guaranteed to exceed any reasonable
	// limit, as it would correspond to 10 non-included blocks.
	//
	// Since we only search for parent blocks which have already been imported,
	// we can guarantee that all imported blocks respect the unincluded segment
	// rules specified by the parachain's runtime and thus will never be too deep.
	const PARENT_SEARCH_DEPTH: usize = 10;

	async move {
		cumulus_client_collator::initialize_collator_subsystems(
			&mut params.overseer_handle,
			params.collator_key,
			params.para_id,
		)
		.await;

		let mut import_notifications = match params.relay_client.import_notification_stream().await
		{
			Ok(s) => s,
			Err(err) => {
				tracing::error!(
					target: crate::LOG_TARGET,
					?err,
					"Failed to initialize consensus: no relay chain import notification stream"
				);

				return;
			}
		};

		// React to each new relmy block
		while let Some(relay_parent_header) = import_notifications.next().await {
			let relay_parent = relay_parent_header.hash();

			// First, verify if the parachain is active (have a core available on the relay)
			if !is_para_scheduled(relay_parent, params.para_id, &mut params.overseer_handle).await {
				tracing::trace!(
					target: crate::LOG_TARGET,
					?relay_parent,
					?params.para_id,
					"Para is not scheduled on any core, skipping import notification",
				);

				continue;
			}

			// Get the PoV size limit dynamically
			let max_pov_size = match params
				.relay_client
				.persisted_validation_data(
					relay_parent,
					params.para_id,
					OccupiedCoreAssumption::Included,
				)
				.await
			{
				Ok(None) => continue,
				Ok(Some(pvd)) => pvd.max_pov_size,
				Err(err) => {
					tracing::error!(
						target: crate::LOG_TARGET,
						?err,
						"Failed to gather information from relay-client"
					);
					continue;
				}
			};

			// Determine which is the current slot
			let slot_now = match consensus_common::relay_slot_and_timestamp(
				&relay_parent_header,
				params.relay_chain_slot_duration,
			) {
				None => continue,
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
					our_slot
				}
			};

			// Search potential parents to build upon
			let mut potential_parents =
				match cumulus_client_consensus_common::find_potential_parents::<Block>(
					ParentSearchParams {
						relay_parent,
						para_id: params.para_id,
						ancestry_lookback: max_ancestry_lookback(
							relay_parent,
							&params.relay_client,
						)
						.await,
						max_depth: PARENT_SEARCH_DEPTH,
						ignore_alternative_branches: true,
					},
					&*params.para_backend,
					&params.relay_client,
				)
				.await
				{
					Err(e) => {
						tracing::error!(
							target: crate::LOG_TARGET,
							?relay_parent,
							err = ?e,
							"Could not fetch potential parents to build upon"
						);

						continue;
					}
					Ok(potential_parents) => potential_parents,
				};

			// Search the first potential parent parablock that is already included in the relay
			let included_block = match potential_parents.iter().find(|x| x.depth == 0) {
				None => continue, // also serves as an `is_empty` check.
				Some(b) => b.hash,
			};

			// At this point, we found a potential parent parablock that is already included in the relay.
			//
			// Sort by depth, ascending, to choose the longest chain. If the longest chain has space,
			// build upon that. Otherwise, don't build at all.
			potential_parents.sort_by_key(|a| a.depth);
			let initial_parent = match potential_parents.pop() {
				None => continue,
				Some(initial_parent) => initial_parent,
			};

			// Build in a loop until not allowed. Note that the selected collators can change
			// at any block, so we need to re-claim our slot every time.
			// This needs to change to support elastic scaling, but for continuously
			// scheduled chains this ensures that the backlog will grow steadily.
			let mut parent_hash = initial_parent.hash;
			let mut parent_header = initial_parent.header;
			let overseer_handle = &mut params.overseer_handle;
			for n_built in 0..2 {
				// Ask to the runtime if we are authorized to create a new parablock on top of this parent.
				// (This will claim the slot internally)
				let para_client = &*params.para_client;
				let keystore = &params.keystore;
				let author_id = match can_build_upon::<_, _>(
					slot_now,
					&parent_header,
					&relay_parent_header,
					included_block,
					para_client,
					&keystore,
				)
				.await
				{
					None => break,
					Some(author_id) => author_id,
				};

				tracing::debug!(
					target: crate::LOG_TARGET,
					?relay_parent,
					unincluded_segment_len = initial_parent.depth + n_built,
					"Slot claimed. Building"
				);

				//
				// Build and announce collations recursively until
				// `can_build_upon` fails or building a collation fails.
				//

				// Create inherents data for the next parablock
				let (parachain_inherent_data, other_inherent_data) =
					match crate::create_inherent_data(
						&params.create_inherent_data_providers,
						params.para_id,
						parent_hash,
						&PersistedValidationData {
							parent_head: parent_header.encode().into(),
							relay_parent_number: *relay_parent_header.number(),
							relay_parent_storage_root: *relay_parent_header.state_root(),
							max_pov_size,
						},
						&params.relay_client,
						relay_parent,
						author_id.clone(),
					)
					.await
					{
						Err(err) => {
							tracing::error!(target: crate::LOG_TARGET, ?err);
							break;
						}
						Ok(x) => x,
					};

				// Compute the hash of the parachain runtime bytecode that we using to build the block.
				// The hash will be send to relay validators alongside the candidate.
				let validation_code_hash = match params.code_hash_provider.code_hash_at(parent_hash)
				{
					None => {
						tracing::error!(
							target: crate::LOG_TARGET,
							?parent_hash,
							"Could not fetch validation code hash"
						);
						break;
					}
					Some(validation_code_hash) => validation_code_hash,
				};

				match super::collate(
					&params.additional_digests_provider,
					author_id,
					&mut params.block_import,
					&params.collator_service,
					keystore,
					&parent_header,
					&mut params.proposer,
					(parachain_inherent_data, other_inherent_data),
					params.authoring_duration,
					// Set the block limit to 50% of the maximum PoV size.
					//
					// TODO: If we got benchmarking that includes the proof size,
					// we should be able to use the maximum pov size.
					(max_pov_size / 2) as usize,
				)
				.await
				{
					Ok((collation, block_data, new_block_hash)) => {
						// Here we are assuming that the import logic protects against equivocations
						// and provides sybil-resistance, as it should.
						params.collator_service.announce_block(new_block_hash, None);

						// Send a submit-collation message to the collation generation subsystem,
						// which then distributes this to validators.
						//
						// Here we are assuming that the leaf is imported, as we've gotten an
						// import notification.
						overseer_handle
							.send_msg(
								CollationGenerationMessage::SubmitCollation(
									SubmitCollationParams {
										relay_parent,
										collation,
										parent_head: parent_header.encode().into(),
										validation_code_hash,
										result_sender: None,
									},
								),
								"SubmitCollation",
							)
							.await;

						parent_hash = new_block_hash;
						parent_header = block_data.into_header();
					}
					Err(err) => {
						tracing::error!(target: crate::LOG_TARGET, ?err);
						break;
					}
				}
			}
		}
	}
}

// Checks if we own the slot at the given block and whether there
// is space in the unincluded segment.
async fn can_build_upon<Block, Client>(
	slot: Slot,
	parent: &Block::Header,
	relay_parent: &PHeader,
	included_block: Block::Hash,
	client: &Client,
	keystore: &KeystorePtr,
) -> Option<NimbusId>
where
	Block: BlockT,
	Client: ProvideRuntimeApi<Block>,
	Client::Api: NimbusApi<Block> + UnincludedSegmentApi<Block>,
{
	let runtime_api = client.runtime_api();
	match crate::claim_slot::<Block, Client>(keystore, client, parent, relay_parent, false).await {
		Ok(Some(nimbus_id)) => {
			// Here we lean on the property that building on an empty unincluded segment must always
			// be legal. Skipping the runtime API query here allows us to seamlessly run this
			// collator against chains which have not yet upgraded their runtime.
			if parent.hash() != included_block {
				match runtime_api.can_build_upon(parent.hash(), included_block, slot) {
					Ok(true) => Some(nimbus_id),
					Ok(false) => None,
					Err(err) => {
						tracing::error!(
							target: crate::LOG_TARGET,
							?err,
							?parent,
							?relay_parent,
							?included_block,
							"Failed to call runtime api UnincludedSegmentApi::can_build_upon",
						);
						None
					}
				}
			} else {
				Some(nimbus_id)
			}
		}
		Ok(None) => None,
		Err(err) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?err,
				?parent,
				?relay_parent,
				?included_block,
				"Failed to claim slot",
			);
			None
		}
	}
}

/// Reads allowed ancestry length parameter from the relay chain storage at the given relay parent.
///
/// Falls back to 0 in case of an error.
async fn max_ancestry_lookback(
	relay_parent: PHash,
	relay_client: &impl RelayChainInterface,
) -> usize {
	match load_abridged_host_configuration(relay_parent, relay_client).await {
		Ok(Some(config)) => config.async_backing_params.allowed_ancestry_len as usize,
		Ok(None) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				"Active config is missing in relay chain storage",
			);
			0
		}
		Err(err) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?err,
				?relay_parent,
				"Failed to read active config from relay chain client",
			);
			0
		}
	}
}

// Checks if there exists a scheduled core for the para at the provided relay parent.
//
// Falls back to `false` in case of an error.
async fn is_para_scheduled(
	relay_parent: PHash,
	para_id: ParaId,
	overseer_handle: &mut OverseerHandle,
) -> bool {
	let (tx, rx) = oneshot::channel();
	let request = RuntimeApiRequest::AvailabilityCores(tx);
	overseer_handle
		.send_msg(
			RuntimeApiMessage::Request(relay_parent, request),
			"LookaheadCollator",
		)
		.await;

	let cores = match rx.await {
		Ok(Ok(cores)) => cores,
		Ok(Err(error)) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?error,
				?relay_parent,
				"Failed to query availability cores runtime API",
			);
			return false;
		}
		Err(oneshot::Canceled) => {
			tracing::error!(
				target: crate::LOG_TARGET,
				?relay_parent,
				"Sender for availability cores runtime request dropped",
			);
			return false;
		}
	};

	cores.iter().any(|core| core.para_id() == Some(para_id))
}
