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

pub mod collators;

mod import_queue;
mod manual_seal;

pub use import_queue::import_queue;
pub use manual_seal::NimbusManualSealConsensusDataProvider;

use cumulus_client_parachain_inherent::ParachainInherentDataProvider;
use cumulus_primitives_core::{
	relay_chain::{Hash as PHash, Header as PHeader},
	ParaId, PersistedValidationData,
};
use cumulus_primitives_parachain_inherent::ParachainInherentData;
use cumulus_relay_chain_interface::RelayChainInterface;
use futures::prelude::*;
use log::{info, warn};
use nimbus_primitives::{NimbusApi, NimbusId, NIMBUS_KEY_ID};
use sc_consensus::BlockImport;
use sp_api::ProvideRuntimeApi;
use sp_application_crypto::ByteArray;
use sp_inherents::{CreateInherentDataProviders, InherentData, InherentDataProvider};
use sp_keystore::{Keystore, KeystorePtr};
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
use std::error::Error;

const LOG_TARGET: &str = "filtering-consensus";

/*impl<Block, BI, Client, RClient, Proposer, CS, DP>
	NimbusConsensus<Block, BI, Client, RClient, Proposer, CS, DP>
where
	Block: BlockT,
	BI: BlockImport<Block> + ParachainBlockImportMarker + Send + Sync + 'static,
	CS: CollatorServiceInterface<Block>,
	Client: ProvideRuntimeApi<Block> + 'static,
	Client::Api: sp_api::Core<Block> + NimbusApi<Block>,
	DP: DigestsProvider<NimbusId, <Block as BlockT>::Hash> + 'static,
	Proposer: ProposerInterface<Block> + Send + Sync + 'static,
	RClient: RelayChainInterface + Send + Clone + 'static,
{

}*/

/// Attempt to claim a slot derived from the given relay-parent header's slot.
pub(crate) async fn claim_slot<Block, Client>(
	keystore: &KeystorePtr,
	para_client: &Client,
	parent: &Block::Header,
	relay_parent_header: &PHeader,
	skip_prediction: bool,
) -> Result<Option<NimbusId>, Box<dyn Error>>
where
	Block: BlockT,
	Client: ProvideRuntimeApi<Block>,
	Client::Api: NimbusApi<Block>,
{
	// Determine if runtime change
	let runtime_upgraded = if *parent.number() > sp_runtime::traits::Zero::zero() {
		use sp_api::Core as _;
		let previous_runtime_version: sp_version::RuntimeVersion = para_client
			.runtime_api()
			.version(parent.hash())
			.map_err(Box::new)?;
		let runtime_version: sp_version::RuntimeVersion = para_client
			.runtime_api()
			.version(parent.hash())
			.map_err(Box::new)?;

		previous_runtime_version != runtime_version
	} else {
		false
	};

	let maybe_key = if skip_prediction || runtime_upgraded {
		first_available_key(keystore)
	} else {
		first_eligible_key::<Block, Client>(
			para_client,
			keystore,
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

/// Explicitly creates the inherent data for parachain block authoring.
pub(crate) async fn create_inherent_data<Block, CIDP, RClient>(
	create_inherent_data_providers: &CIDP,
	para_id: ParaId,
	parent: Block::Hash,
	validation_data: &PersistedValidationData,
	relay_client: &RClient,
	relay_parent: PHash,
	author_id: NimbusId,
) -> Result<(ParachainInherentData, InherentData), Box<dyn Error + Send + Sync + 'static>>
where
	Block: BlockT,
	CIDP: CreateInherentDataProviders<Block, (PHash, PersistedValidationData, NimbusId)> + 'static,
	RClient: RelayChainInterface + Send + Clone + 'static,
{
	let paras_inherent_data = ParachainInherentDataProvider::create_at(
		relay_parent,
		relay_client,
		validation_data,
		para_id,
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

	let other_inherent_data = create_inherent_data_providers
		.create_inherent_data_providers(parent, (relay_parent, validation_data.clone(), author_id))
		.map_err(|e| e as Box<dyn Error + Send + Sync + 'static>)
		.await?
		.create_inherent_data()
		.await
		.map_err(Box::new)?;

	Ok((paras_inherent_data, other_inherent_data))
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
pub(crate) fn first_eligible_key<Block, Client>(
	client: &Client,
	keystore: &dyn Keystore,
	parent: &Block::Header,
	slot_number: u32,
) -> Option<Vec<u8>>
where
	Block: BlockT,
	Client: ProvideRuntimeApi<Block>,
	Client::Api: NimbusApi<Block>,
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
