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

//! A pallet to put your incoming XCM execution into a restricted emergency or safe mode automatically.

#![allow(non_camel_case_types)]
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

use cumulus_pallet_parachain_system::CheckAssociatedRelayNumber;
use frame_support::pallet;
use frame_support::pallet_prelude::*;
use frame_support::traits::{ProcessMessage, QueuePausedQuery};
use frame_system::pallet_prelude::*;
use frame_system::RawOrigin;
use parity_scale_codec::{Decode, Encode};
use polkadot_parachain_primitives::primitives::{Id, RelayChainBlockNumber, XcmpMessageHandler};

// TODO: move to file type.rs
#[derive(Decode, Default, Encode, PartialEq, TypeInfo)]
pub enum XcmMode {
	#[default]
	Normal,
	Limited,
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
		+ cumulus_pallet_parachain_system::Config<
			CheckAssociatedRelayNumber = Pallet<Self>,
			XcmpMessageHandler = Pallet<Self>,
		> + pallet_message_queue::Config<QueuePausedQuery = Pallet<Self>>
	{
		/// Overarching event type
		type RuntimeEvent: From<Event> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// TODO doc
		type CheckAssociatedRelayNumber: CheckAssociatedRelayNumber;

		/// TODO doc
		type QueuePausedQuery: QueuePausedQuery<<Self::MessageProcessor as ProcessMessage>::Origin>;

		/// The HRMP handler to be used in normal operating mode
		type HrmpMessageHandler: XcmpMessageHandler;

		/// Allow to apply a lower Weight Limit for DMP messages where block production seem's stall
		type LimitedModeDmpWeightLimit: Get<Weight>;

		/// Allow to apply a lower Weight Limit for HRMP messages where block production seem's stall
		type LimitedModeHrmpWeightLimit: Get<Weight>;

		/// Maximum number of relay block to skip before trigering the Limited mode.
		/// Note that if both Limited and Paused threshold are reach, the Paused mode will be apply.
		type LimitedThreshold: Get<RelayChainBlockNumber>;

		/// Maximum number of relay block to skip before trigering the Paused mode.
		type PausedThreshold: Get<RelayChainBlockNumber>;

		/// Origin allowed to perform a fast authorize upgrade when XcmMode is not normal
		type FastAuthorizeUpgradeOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Origin allowed to resume from limited mode
		type LimitedToNormalOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// origin allowed to resume to normal operations from paused mode
		type PausedToNormalOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// origin alllowed to change Paused mode to Limited
		type PausedToLimitedOrigin: EnsureOrigin<Self::RuntimeOrigin>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(crate) fn deposit_event)]
	pub enum Event {
		/// The xcm mode was put into Limited Mode
		EnteredLimitedXcmMode,
		// The xcm incoming execution was Paused
		EnteredPausedXcmMode,
		/// The XCm incoming execution returned to normal operations
		NormalXcmOperationResumed,
	}

	/// An error that can occur while executing this pallet's extrinsics.
	#[pallet::error]
	pub enum Error<T> {
		/// Fast authorize upgrade is forbiden in normal mode
		NotInLimitedOrPausedMode,
		/// The current XCM Mode is not Limited
		NotInLimitedMode,
		/// The current XCM Mode is not Paused
		NotInPausedMode,
	}

	#[pallet::storage]
	#[pallet::getter(fn mode)]
	/// Whether incoming XCM is enabled, limited or paused
	pub type Mode<T: Config> = StorageValue<_, XcmMode, ValueQuery>;

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_: BlockNumberFor<T>) -> Weight {
			// Account for 1 read and 1 write to `Mode`
			T::DbWeight::get().reads_writes(1, 1)
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight((0, DispatchClass::Operational))]
		pub fn limited_to_normal(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			T::LimitedToNormalOrigin::ensure_origin(origin)?;

			ensure!(
				Mode::<T>::get() == XcmMode::Limited,
				Error::<T>::NotInLimitedMode
			);

			Mode::<T>::set(XcmMode::Normal);
			<Pallet<T>>::deposit_event(Event::NormalXcmOperationResumed);

			Ok(().into())
		}

		#[pallet::call_index(1)]
		#[pallet::weight((0, DispatchClass::Operational))]
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

		#[pallet::call_index(2)]
		#[pallet::weight((0, DispatchClass::Operational))]
		pub fn paused_to_limited(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			T::PausedToLimitedOrigin::ensure_origin(origin)?;

			ensure!(
				Mode::<T>::get() == XcmMode::Paused,
				Error::<T>::NotInPausedMode
			);

			Mode::<T>::set(XcmMode::Limited);
			<Pallet<T>>::deposit_event(Event::EnteredLimitedXcmMode);

			Ok(().into())
		}

		#[pallet::call_index(3)]
		#[pallet::weight((1_000_000, DispatchClass::Operational))]
		pub fn fast_authorize_upgrade(
			origin: OriginFor<T>,
			code_hash: T::Hash,
		) -> DispatchResult {
			T::FastAuthorizeUpgradeOrigin::ensure_origin(origin)?;
			ensure!(
				Mode::<T>::get() != XcmMode::Normal,
				Error::<T>::NotInLimitedOrPausedMode
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

		if current > previous + T::PausedThreshold::get() {
			Mode::<T>::set(XcmMode::Paused);
			<Pallet<T>>::deposit_event(Event::EnteredPausedXcmMode);
		} else if current > previous + T::LimitedThreshold::get() {
			Mode::<T>::set(XcmMode::Limited);
			<Pallet<T>>::deposit_event(Event::EnteredLimitedXcmMode);
		}
	}
}

impl<T: Config> XcmpMessageHandler for Pallet<T> {
	fn handle_xcmp_messages<'a, I: Iterator<Item = (Id, RelayChainBlockNumber, &'a [u8])>>(
		iter: I,
		limit: Weight,
	) -> Weight {
		match Mode::<T>::get() {
			XcmMode::Normal => T::HrmpMessageHandler::handle_xcmp_messages(iter, limit),
			XcmMode::Limited => T::HrmpMessageHandler::handle_xcmp_messages(
				iter,
				T::LimitedModeHrmpWeightLimit::get(),
			),
			XcmMode::Paused => T::HrmpMessageHandler::handle_xcmp_messages(iter, Weight::zero()),
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
