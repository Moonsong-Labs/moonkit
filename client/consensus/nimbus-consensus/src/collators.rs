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

use crate::*;
use cumulus_client_collator::service::ServiceInterface as CollatorServiceInterface;
use cumulus_client_consensus_common::{ParachainBlockImportMarker, ParachainCandidate};
use cumulus_client_consensus_proposer::ProposerInterface;
use cumulus_primitives_core::ParachainBlockData;
use cumulus_primitives_parachain_inherent::ParachainInherentData;
use futures::prelude::*;
use log::{debug, info};
use nimbus_primitives::{CompatibleDigestItem, DigestsProvider, NimbusId, NIMBUS_KEY_ID};
use polkadot_node_primitives::{Collation, MaybeCompressedPoV};
use sc_consensus::{BlockImport, BlockImportParams};
use sp_application_crypto::ByteArray;
use sp_consensus::{BlockOrigin, Proposal};
use sp_core::{crypto::CryptoTypeId, sr25519, Encode};
use sp_inherents::InherentData;
use sp_keystore::Keystore;
use sp_runtime::{
	traits::{Block as BlockT, Header as HeaderT},
	DigestItem,
};
use std::convert::TryInto;
use std::error::Error;
use std::time::Duration;

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
) -> Result<(Collation, ParachainBlockData<Block>, Block::Hash), Box<dyn Error + Send + 'static>>
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

	let Proposal {
		block,
		storage_changes,
		proof,
	} = proposer
		.propose(
			&parent_header,
			&inherent_data.0,
			inherent_data.1,
			sp_runtime::generic::Digest { logs },
			proposal_duration,
			Some(max_pov_size),
		)
		.await
		.map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

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

	let post_hash = block_import_params.post_hash();

	// Print the same log line as slots (aura and babe)
	info!(
		"🔖 Sealed block for proposal at {}. Hash now {:?}, previously {:?}.",
		*header.number(),
		block_import_params.post_hash(),
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
			proof: proof,
		},
	) {
		tracing::info!(
			target: crate::LOG_TARGET,
			"PoV size {{ header: {}kb, extrinsics: {}kb, storage_proof: {}kb }}",
			block_data.header().encode().len() as f64 / 1024f64,
			block_data.extrinsics().encode().len() as f64 / 1024f64,
			block_data.storage_proof().encode().len() as f64 / 1024f64,
		);

		if let MaybeCompressedPoV::Compressed(ref pov) = collation.proof_of_validity {
			tracing::info!(
				target: crate::LOG_TARGET,
				"Compressed PoV size: {}kb",
				pov.block_data.0.len() as f64 / 1024f64,
			);
		}

		Ok((collation, block_data, post_hash))
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
