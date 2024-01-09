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

#![cfg_attr(not(feature = "std"), no_std)]

pub use crate::weights::WeightInfo;
use cumulus_pallet_parachain_system::RelaychainStateProvider;
use cumulus_primitives_core::relay_chain::BlockNumber as RelayBlockNumber;
use frame_support::pallet;
use frame_support::pallet_prelude::*;
use frame_system::pallet_prelude::*;
pub use pallet::*;
use sp_core::Get;
use sp_core::H256;
use sp_std::collections::vec_deque::VecDeque;

#[cfg(any(test, feature = "runtime-benchmarks"))]
mod benchmarks;
pub mod weights;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[pallet]
pub mod pallet {
	use super::*;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(PhantomData<T>);

	/// Configuration trait of this pallet.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		type RelaychainStateProvider: RelaychainStateProvider;
		#[pallet::constant]
		type MaxStorageRoots: Get<u32>;
		/// Weight info
		type WeightInfo: WeightInfo;
	}

	/// Map of relay block number to relay storage root
	#[pallet::storage]
	pub type RelayStorageRoot<T: Config> =
		StorageMap<_, Twox64Concat, RelayBlockNumber, H256, OptionQuery>;

	/// List of all the keys in `RelayStorageRoot`.
	/// Used to remove the oldest key without having to iterate over all of them.
	#[pallet::storage]
	pub type RelayStorageRootKeys<T: Config> =
		StorageValue<_, VecDeque<RelayBlockNumber>, ValueQuery>;

	impl<T: Config> Pallet<T> {
		/// Populates `RelayStorageRoot` using `RelaychainStateProvider`.
		pub fn set_relay_storage_root() {
			let relay_state = T::RelaychainStateProvider::current_relay_chain_state();

			// If this relay block number has already been stored, skip it.
			if <RelayStorageRoot<T>>::contains_key(relay_state.number) {
				return;
			}

			<RelayStorageRoot<T>>::insert(relay_state.number, relay_state.state_root);
			let mut keys = <RelayStorageRootKeys<T>>::get();
			keys.push_back(relay_state.number);
			// Delete the oldest stored root if the total number is greater than MaxStorageRoots
			if u32::try_from(keys.len()).unwrap() > T::MaxStorageRoots::get() {
				let first_key = keys.pop_front().unwrap();
				<RelayStorageRoot<T>>::remove(first_key);
			}

			<RelayStorageRootKeys<T>>::put(keys);
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_now: BlockNumberFor<T>) -> Weight {
			// Account for weight used in on_finalize
			T::WeightInfo::set_relay_storage_root()
		}
		fn on_finalize(_now: BlockNumberFor<T>) {
			Self::set_relay_storage_root();
		}
	}
}
