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

pub mod consensus_hook;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use pallet::*;

use frame_support::{pallet_prelude::*, traits::OnTimestampSet};
use sp_consensus_slots::{Slot, SlotDuration};
use sp_runtime::SaturatedConversion;

/// The InherentIdentifier for nimbus's extension inherent
pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"nimb-ext";

/// A way to get the current parachain slot and verify it's validity against the relay slot.
/// If you don't need to have slots at parachain level, you can use the `RelaySlot` implementation.
pub trait GetAndVerifySlot {
	/// Get the current slot
	fn get_and_verify_slot(relay_chain_slot: &Slot) -> Result<Slot, sp_runtime::DispatchError>;
}

/// Parachain slot implementation that use the relay chain slot directly
pub struct RelaySlot;
impl GetAndVerifySlot for RelaySlot {
	fn get_and_verify_slot(relay_chain_slot: &Slot) -> Result<Slot, sp_runtime::DispatchError> {
		Ok(*relay_chain_slot)
	}
}

/// Parachain slot implementation that use a slot provider
pub struct ParaSlot<const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32, SlotProvider>(
	PhantomData<SlotProvider>,
);

impl<const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32, SlotProvider> GetAndVerifySlot
	for ParaSlot<RELAY_CHAIN_SLOT_DURATION_MILLIS, SlotProvider>
where
	SlotProvider: Get<(Slot, SlotDuration)>,
{
	fn get_and_verify_slot(relay_chain_slot: &Slot) -> Result<Slot, sp_runtime::DispatchError> {
		// Convert relay chain timestamp.
		let relay_chain_timestamp =
			u64::from(RELAY_CHAIN_SLOT_DURATION_MILLIS).saturating_mul((*relay_chain_slot).into());

		let (new_slot, para_slot_duration) = SlotProvider::get();

		let para_slot_from_relay =
			Slot::from_timestamp(relay_chain_timestamp.into(), para_slot_duration);

		if new_slot == para_slot_from_relay {
			Ok(new_slot)
		} else {
			Err("Unexpected slot".into())
		}
	}
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	/// The current storage version.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(PhantomData<T>);

	/// The configuration trait.
	#[pallet::config]
	pub trait Config: pallet_timestamp::Config + frame_system::Config {
		/// Whether or not to allow more than one block per slot.
		/// Setting it to 'true' will enable async-backing compatibility.
		type AllowMultipleBlocksPerSlot: Get<bool>;

		/// A way to get the current parachain slot and verify it's validity against the relay slot.
		type GetAndVerifySlot: GetAndVerifySlot;

		/// Purely informative, but used by mocking tools like chospticks to allow knowing how to mock
		/// blocks
		#[pallet::constant]
		type SlotDuration: Get<Self::Moment>;
	}

	/// First tuple element is the highest slot that has been seen in the history of this chain.
	/// Second tuple element is the number of authored blocks so far.
	/// This is a strictly-increasing value if T::AllowMultipleBlocksPerSlot = false.
	#[pallet::storage]
	#[pallet::getter(fn slot_info)]
	pub type SlotInfo<T: Config> = StorageValue<_, (Slot, u32), OptionQuery>;

	impl<T: Config> Pallet<T> {
		/// Determine the slot-duration based on the Timestamp module configuration.
		pub fn slot_duration() -> T::Moment {
			T::SlotDuration::get()
		}
	}
}

impl<T: Config> OnTimestampSet<T::Moment> for Pallet<T> {
	fn on_timestamp_set(moment: T::Moment) {
		let slot_duration = Self::slot_duration();
		assert!(!slot_duration.is_zero(), "Slot duration cannot be zero.");

		let timestamp_slot = moment / slot_duration;
		let timestamp_slot = Slot::from(timestamp_slot.saturated_into::<u64>());

		let Some((current_slot, _)) = SlotInfo::<T>::get() else {
			unreachable!("`SlotInfo` should exist at this point; qed");
		};

		assert_eq!(
			current_slot,
			timestamp_slot,
			"`timestamp_slot` must match `current_slot`. This likely means that the configured block \
			time in the node and/or rest of the runtime is not compatible with Nimbus's Async backing \
			`SlotDuration`",
		);
	}
}
