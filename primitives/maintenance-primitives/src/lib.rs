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

//! The Maintenance Mode primitive trait implementations

#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;
use cumulus_primitives_core::{relay_chain::BlockNumber as RelayBlockNumber, DmpMessageHandler};
use frame_support::{
	traits::{Get, Hooks, OffchainWorker, OnFinalize, OnIdle, OnInitialize, OnRuntimeUpgrade},
	weights::Weight,
	inherent::Vec,
};

#[cfg(feature = "try-runtime")]
use sp_runtime::DispatchError;

pub type BlockNumber = u32;

/// The hooks we want to run in Maintenance Mode
pub struct MaintenanceHooks<T>(PhantomData<T>);
impl<T: Hooks<BlockNumber>> OnInitialize<BlockNumber> for MaintenanceHooks<T> {
	fn on_initialize(n: BlockNumber) -> Weight {
		T::on_initialize(n)
	}
}

// We override onIdle for xcmQueue and dmpQueue pallets to not process messages inside it
impl<T: Hooks<BlockNumber>> OnIdle<BlockNumber> for MaintenanceHooks<T> {
	fn on_idle(_n: BlockNumber, _max_weight: Weight) -> Weight {
		Weight::zero()
	}
}

impl<T: Hooks<BlockNumber>> OnRuntimeUpgrade for MaintenanceHooks<T> {
	fn on_runtime_upgrade() -> Weight {
		T::on_runtime_upgrade()
	}
	#[cfg(feature = "try-runtime")]
	fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::DispatchError> {
		T::pre_upgrade()
	}

	#[cfg(feature = "try-runtime")]
	fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::DispatchError> {
		T::post_upgrade(state)
	}
}

impl<T: Hooks<BlockNumber>> OnFinalize<BlockNumber> for MaintenanceHooks<T> {
	fn on_finalize(n: BlockNumber) {
		T::on_finalize(n)
	}
}

impl<T: Hooks<BlockNumber>> OffchainWorker<BlockNumber> for MaintenanceHooks<T> {
	fn offchain_worker(n: BlockNumber) {
		T::offchain_worker(n)
	}
}

pub struct NormalDmpHandler<Runtime, Handler>(PhantomData<(Runtime, Handler)>);
impl<Runtime, Handler> DmpMessageHandler for NormalDmpHandler<Runtime, Handler>
where
	Runtime: frame_system::Config + pallet_migrations::Config,
	Handler: DmpMessageHandler,
{
	// This implementation makes messages be queued
	// Since the limit is 0, messages are queued for next iteration
	fn handle_dmp_messages(
		iter: impl Iterator<Item = (RelayBlockNumber, Vec<u8>)>,
		limit: Weight,
	) -> Weight {
		(if pallet_migrations::Pallet::<Runtime>::should_pause_xcm() {
			Handler::handle_dmp_messages(iter, Weight::zero())
		} else {
			Handler::handle_dmp_messages(iter, limit)
		}) + <Runtime as frame_system::Config>::DbWeight::get().reads(1)
	}
}

pub struct MaintenanceDmpHandler<Handler>(PhantomData<Handler>);
impl<Handler: DmpMessageHandler> DmpMessageHandler for MaintenanceDmpHandler<Handler> {
	// This implementation makes messages be queued
	// Since the limit is 0, messages are queued for next iteration
	fn handle_dmp_messages(
		iter: impl Iterator<Item = (RelayBlockNumber, Vec<u8>)>,
		_limit: Weight,
	) -> Weight {
		Handler::handle_dmp_messages(iter, Weight::zero())
	}
}
