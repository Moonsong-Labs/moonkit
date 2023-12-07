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
use cumulus_client_consensus_common::ParachainBlockImportMarker;
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::{
	relay_chain::{BlockId as RBlockId, Hash as PHash},
	CollectCollationInfo, ParaId, PersistedValidationData,
};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use futures::prelude::*;
use nimbus_primitives::{DigestsProvider, NimbusApi, NimbusId};
use polkadot_node_primitives::CollationResult;
use polkadot_primitives::CollatorPair;
use sc_client_api::{BlockBackend, BlockOf};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
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
		+ HeaderBackend<Block>
		+ BlockBackend<Block>
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
			skip_prediction,
			..
		} = params;

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
				skip_prediction,
			)
			.await
			{
				Ok(None) => continue,
				Ok(Some(nimbus_id)) => nimbus_id,
				Err(e) => reject_with_error!(e),
			};

			let inherent_data = try_request!(
				create_inherent_data(
					&create_inherent_data_providers,
					para_id,
					parent_header.hash(),
					&validation_data,
					&relay_client,
					*request.relay_parent(),
					nimbus_id.clone(),
				)
				.await
			);

			let (collation, _, post_hash) = try_request!(
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
					// Set the block limit to 50% of the maximum PoV size.
					//
					// TODO: If we got benchmarking that includes the proof size,
					// we should be able to use the maximum pov size.
					(validation_data.max_pov_size / 2) as usize,
				)
				.await
			);

			let result_sender = Some(collator_service.announce_with_barrier(post_hash));
			request.complete(Some(CollationResult {
				collation,
				result_sender,
			}));
		}
	}
}
