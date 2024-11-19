// Copyright 2019-2022 Moonsong Labs
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

//! A pallet to put your incoming XCM execution into a restricted emergency
//! or safe mode automatically.
//!
//! Whenever the difference between the current relay block number and
//! the relay block number from the previous block is greater than a
//! certain threshold, the pallet will enter an `Paused` mode that
//! will prevent any XCM messages to be executed (while still enqueuing them)
//! and will allow a configurable origin to authorize a runtime upgrade.
//! This helps on situations where a bug could cause the execution
//! of a particular message to render a block invalid (e.g: if the message takes
//! too long to execute), by allowing to unstall the chain without the need
//! of a governance proposal on the relay chain.
//!
//! The pallet implements some traits from other pallets and forces the runtime
//! to use those implementations. These work as wrappers for the actual traits
//! to be used by the runtime, which should be given as Config types here.
//! In particular:
//! * `CheckAssociatedRelayNumber` is used to check the relay chain block diff and
//!    enter emergency mode when appropriate.
//! * `QueuePausedQuery` is used to pause XCM execution

#![allow(non_camel_case_types)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use pallet::*;

use cumulus_pallet_parachain_system::CheckAssociatedRelayNumber;
use frame_support::pallet;
use frame_support::pallet_prelude::*;
use frame_support::traits::{ProcessMessage, QueuePausedQuery};
use frame_system::pallet_prelude::*;
use frame_system::{RawOrigin, WeightInfo};
use parity_scale_codec::{Decode, Encode};
use polkadot_parachain_primitives::primitives::RelayChainBlockNumber;

#[derive(Decode, Default, Encode, PartialEq, TypeInfo)]
/// XCM Execution mode
pub enum XcmMode {
	/// Normal operation
	#[default]
	Normal,
	/// Paused: no XCM messages are processed and `FastAuthorizedUpgrade`
	/// origin can authorize a runtime upgrade
	Paused,
}

#[pallet]
pub mod pallet {
	use super::*;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(PhantomData<T>);

	/// Configuration trait of this pallet.
	#[pallet::config]
	pub trait Config:
		frame_system::Config
		+ cumulus_pallet_parachain_system::Config<CheckAssociatedRelayNumber = Pallet<Self>>
		+ pallet_message_queue::Config<QueuePausedQuery = Pallet<Self>>
	{
		/// Overarching event type
		type RuntimeEvent: From<Event> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Used before check associated relay block number. It should be what would be passed to
		/// `cumulus_pallet_parachain_system` if this pallet was not being used.
		type CheckAssociatedRelayNumber: CheckAssociatedRelayNumber;

		/// Used to check wether message queue is paused when `XcmMode` is `Normal`. It should be
		/// what would be passed to `pallet_message_queue` if this pallet was not being used.
		type QueuePausedQuery: QueuePausedQuery<<Self::MessageProcessor as ProcessMessage>::Origin>;

		/// Maximum number of relay block to skip before trigering the Paused mode.
		type PausedThreshold: Get<RelayChainBlockNumber>;

		/// Origin allowed to perform a fast authorize upgrade when XcmMode is not normal
		type FastAuthorizeUpgradeOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// origin allowed to resume to normal operations from paused mode
		type PausedToNormalOrigin: EnsureOrigin<Self::RuntimeOrigin>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(crate) fn deposit_event)]
	pub enum Event {
		/// The XCM incoming execution was Paused
		EnteredPausedXcmMode,
		/// The XCM incoming execution returned to normal operation
		NormalXcmOperationResumed,
	}

	/// An error that can occur while executing this pallet's extrinsics.
	#[pallet::error]
	pub enum Error<T> {
		/// The current XCM Mode is not Paused
		NotInPausedMode,
	}

	#[pallet::storage]
	/// Whether incoming XCM is enabled or paused
	pub type Mode<T: Config> = StorageValue<_, XcmMode, ValueQuery>;

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_: BlockNumberFor<T>) -> Weight {
			// Account for 1 read and 1 write to `Mode`
			// during `check_associated_relay_number`
			T::DbWeight::get().reads_writes(1, 1)
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Resume `Normal` mode
		#[pallet::call_index(0)]
		#[pallet::weight((T::DbWeight::get().reads_writes(1, 2), DispatchClass::Operational))]
		pub fn paused_to_normal(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			T::PausedToNormalOrigin::ensure_origin(origin)?;

			ensure!(
				Mode::<T>::get() == XcmMode::Paused,
				Error::<T>::NotInPausedMode
			);

			Mode::<T>::set(XcmMode::Normal);
			<Pallet<T>>::deposit_event(Event::NormalXcmOperationResumed);

			Ok(().into())
		}

		/// Authorize a runtime upgrade. Only callable in `Paused` mode
		#[pallet::call_index(1)]
		#[pallet::weight((T::SystemWeightInfo::authorize_upgrade().saturating_add(T::DbWeight::get().reads(1_u64)), DispatchClass::Operational))]
		pub fn fast_authorize_upgrade(origin: OriginFor<T>, code_hash: T::Hash) -> DispatchResult {
			T::FastAuthorizeUpgradeOrigin::ensure_origin(origin)?;
			ensure!(
				Mode::<T>::get() == XcmMode::Paused,
				Error::<T>::NotInPausedMode
			);

			frame_system::Pallet::<T>::authorize_upgrade(RawOrigin::Root.into(), code_hash)
		}
	}
}

impl<T: Config> CheckAssociatedRelayNumber for Pallet<T> {
	fn check_associated_relay_number(
		current: RelayChainBlockNumber,
		previous: RelayChainBlockNumber,
	) {
		<T as Config>::CheckAssociatedRelayNumber::check_associated_relay_number(current, previous);

		if (previous != 0) && (current > (previous + T::PausedThreshold::get())) {
			Mode::<T>::set(XcmMode::Paused);
			<Pallet<T>>::deposit_event(Event::EnteredPausedXcmMode);
		}
	}
}

impl<T> QueuePausedQuery<<T::MessageProcessor as ProcessMessage>::Origin> for Pallet<T>
where
	T: Config,
{
	fn is_paused(origin: &<T::MessageProcessor as ProcessMessage>::Origin) -> bool {
		if Mode::<T>::get() == XcmMode::Normal {
			<T as Config>::QueuePausedQuery::is_paused(origin)
		} else {
			true
		}
	}
}
