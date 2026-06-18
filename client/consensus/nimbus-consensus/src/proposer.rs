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

//! Moonkit-local replacement for the upstream `cumulus-client-consensus-proposer`,
//! which was removed in `polkadot-stable2603` (paritytech/polkadot-sdk#9947).
//!
//! Provides a thin extension of the Substrate [`ProposerFactory`] tailored to
//! parachain block production. The caller owns the `ProofRecorder` and drains
//! the resulting storage proof after this proposer returns.

use async_trait::async_trait;
use cumulus_primitives_parachain_inherent::ParachainInherentData;
use sc_basic_authorship::ProposerFactory;
use sc_block_builder::BlockBuilderApi;
use sc_transaction_pool_api::TransactionPool;
use sp_api::{ApiExt, CallApiAt, ProofRecorder, ProvideRuntimeApi};
use sp_blockchain::HeaderBackend;
use sp_consensus::{Environment, Proposal, ProposeArgs, Proposer};
use sp_externalities::Extensions;
use sp_inherents::{InherentData, InherentDataProvider};
use sp_runtime::{traits::Block as BlockT, Digest};
use std::time::Duration;

/// Errors that can occur when proposing a parachain block.
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct Error {
	inner: anyhow::Error,
}

impl Error {
	/// Create an error tied to the creation of a proposer.
	pub fn proposer_creation(err: impl Into<anyhow::Error>) -> Self {
		Error {
			inner: err.into().context("Proposer Creation"),
		}
	}

	/// Create an error tied to the proposing logic itself.
	pub fn proposing(err: impl Into<anyhow::Error>) -> Self {
		Error {
			inner: err.into().context("Proposing"),
		}
	}
}

/// An interface for proposers.
#[async_trait]
pub trait ProposerInterface<Block: BlockT> {
	/// Propose a collation using the supplied `InherentData` and the provided
	/// `ParachainInherentData`.
	///
	/// `storage_proof_recorder` and `extra_extensions` are forwarded into the
	/// underlying [`ProposeArgs`] so the caller can drive proof recording and
	/// register extra runtime extensions; see paritytech/polkadot-sdk#9947 for
	/// background.
	async fn propose(
		&mut self,
		parent_header: &Block::Header,
		paras_inherent_data: &ParachainInherentData,
		other_inherent_data: InherentData,
		inherent_digests: Digest,
		max_duration: Duration,
		block_size_limit: Option<usize>,
		storage_proof_recorder: Option<ProofRecorder<Block>>,
		extra_extensions: Extensions,
	) -> Result<Proposal<Block>, Error>;
}

#[async_trait]
impl<Block, A, C> ProposerInterface<Block> for ProposerFactory<A, C>
where
	A: TransactionPool<Block = Block> + 'static,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + CallApiAt<Block> + Send + Sync + 'static,
	C::Api: ApiExt<Block> + BlockBuilderApi<Block>,
	Block: sp_runtime::traits::Block,
{
	async fn propose(
		&mut self,
		parent_header: &Block::Header,
		paras_inherent_data: &ParachainInherentData,
		other_inherent_data: InherentData,
		inherent_digests: Digest,
		max_duration: Duration,
		block_size_limit: Option<usize>,
		storage_proof_recorder: Option<ProofRecorder<Block>>,
		extra_extensions: Extensions,
	) -> Result<Proposal<Block>, Error> {
		let proposer = self
			.init(parent_header)
			.await
			.map_err(|e| Error::proposer_creation(anyhow::Error::new(e)))?;

		let mut inherent_data = other_inherent_data;
		paras_inherent_data
			.provide_inherent_data(&mut inherent_data)
			.await
			.map_err(|e| Error::proposing(anyhow::Error::new(e)))?;

		proposer
			.propose(ProposeArgs {
				inherent_data,
				inherent_digests,
				max_duration,
				block_size_limit,
				storage_proof_recorder,
				extra_extensions,
			})
			.await
			.map_err(|e| Error::proposing(anyhow::Error::new(e)))
	}
}
