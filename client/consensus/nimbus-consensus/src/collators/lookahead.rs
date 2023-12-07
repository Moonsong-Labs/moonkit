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
};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::{relay_chain::Hash as PHash, CollectCollationInfo, ParaId};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use futures::{channel::oneshot, prelude::*};
use nimbus_primitives::{DigestsProvider, NimbusApi, NimbusId};
use polkadot_node_subsystem::messages::{RuntimeApiMessage, RuntimeApiRequest};
use polkadot_primitives::CollatorPair;
use sc_client_api::{BlockBackend, BlockOf};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_consensus::SyncOracle;
use sp_consensus_slots::{Slot, SlotDuration};
use sp_inherents::CreateInherentDataProviders;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
use std::{sync::Arc, time::Duration};

/// Parameters for [`run`].
pub struct Params<BI, CIDP, Client, Backend, RClient, CHP, SO, Proposer, CS, DP = ()> {
	/// Inherent data providers. Only non-consensus inherent data should be provided, i.e.
	/// the timestamp, slot, and paras inherents should be omitted, as they are set by this
	/// collator.
	pub create_inherent_data_providers: CIDP,
	/// Used to actually import blocks.
	pub block_import: BI,
	/// The underlying para client.
	pub para_client: Arc<Client>,
	/// The para client's backend, used to access the database.
	pub para_backend: Arc<Backend>,
	/// A handle to the relay-chain client.
	pub relay_client: RClient,
	/// A validation code hash provider, used to get the current validation code hash.
	pub code_hash_provider: CHP,
	/// A chain synchronization oracle.
	pub sync_oracle: SO,
	/// The underlying keystore, which should contain Aura consensus keys.
	pub keystore: KeystorePtr,
	/// The collator key used to sign collations before submitting to validators.
	pub collator_key: CollatorPair,
	/// The para's ID.
	pub para_id: ParaId,
	/// A handle to the relay-chain client's "Overseer" or task orchestrator.
	pub overseer_handle: OverseerHandle,
	/// The length of slots in this parachain.
	/// If the parachain doesn't have slot and rely only on relay slots, set it to None.
	pub slot_duration: Option<SlotDuration>,
	/// The length of slots in the relay chain.
	pub relay_chain_slot_duration: Duration,
	/// The underlying block proposer this should call into.
	pub proposer: Proposer,
	/// The generic collator service used to plug into this consensus engine.
	pub collator_service: CS,
	/// The amount of time to spend authoring each block.
	pub authoring_duration: Duration,
	pub additional_digests_provider: DP,
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
	CIDP: CreateInherentDataProviders<Block, ()> + 'static,
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
	//const PARENT_SEARCH_DEPTH: usize = 10;

	async move {
		cumulus_client_collator::initialize_collator_subsystems(
			&mut params.overseer_handle,
			params.collator_key,
			params.para_id,
		)
		.await;

		let mut _import_notifications = match params.relay_client.import_notification_stream().await
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

		// TODO
		()
	}
}

// Checks if we own the slot at the given block and whether there
// is space in the unincluded segment.
async fn _can_build_upon<Block, Client, P>(
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
async fn _max_ancestry_lookback(
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
async fn _is_para_scheduled(
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
