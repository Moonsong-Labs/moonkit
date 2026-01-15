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

mod collator;
pub mod collators;

mod import_queue;
mod manual_seal;

pub use import_queue::import_queue;
pub use manual_seal::NimbusManualSealConsensusDataProvider;

use cumulus_primitives_core::{relay_chain::Header as PHeader, PersistedValidationData};
use log::{info, warn};
use nimbus_primitives::{NimbusApi, NimbusId, NIMBUS_KEY_ID};
use polkadot_node_primitives::PoV;
use polkadot_primitives::HeadData;
use polkadot_primitives::{BlockNumber as RBlockNumber, Hash as RHash};
use sc_consensus::BlockImport;
use sc_network_types::PeerId;
use sp_api::ProvideRuntimeApi;
use sp_application_crypto::ByteArray;
use sp_core::Encode;
use sp_keystore::{Keystore, KeystorePtr};
use sp_runtime::traits::{Block as BlockT, Header as HeaderT, NumberFor};
use std::{error::Error, fs, path::PathBuf};

const LOG_TARGET: &str = "filtering-consensus";

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
			.version(*parent.parent_hash())
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
		if let Ok(nimbus_id) = NimbusId::from_slice(type_public_pair) {
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

/// Export the given `pov` to the file system at `path`.
///
/// The file will be named `block_hash_block_number.pov`.
///
/// The `parent_header`, `relay_parent_storage_root` and `relay_parent_number` will also be
/// stored in the file alongside the `pov`. This enables stateless validation of the `pov`.
pub(crate) fn export_pov_to_path<Block: BlockT>(
	path: PathBuf,
	pov: PoV,
	block_hash: Block::Hash,
	block_number: NumberFor<Block>,
	parent_header: Block::Header,
	relay_parent_storage_root: RHash,
	relay_parent_number: RBlockNumber,
	max_pov_size: u32,
) {
	if let Err(error) = fs::create_dir_all(&path) {
		tracing::error!(target: LOG_TARGET, %error, path = %path.display(), "Failed to create PoV export directory");
		return;
	}

	let mut file = match fs::File::create(path.join(format!("{block_hash:?}_{block_number}.pov"))) {
		Ok(f) => f,
		Err(error) => {
			tracing::error!(target: LOG_TARGET, %error, "Failed to export PoV.");
			return;
		}
	};

	pov.encode_to(&mut file);
	PersistedValidationData {
		parent_head: HeadData(parent_header.encode()),
		relay_parent_number,
		relay_parent_storage_root,
		max_pov_size,
	}
	.encode_to(&mut file);
}
