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

use frame_support::pallet_prelude::*;
use sp_consensus_slots::{Slot, SlotDuration};

/// The InherentIdentifier for nimbus's extension inherent
pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"nimb-ext";

/// A way to get the current parachain slot and verify it's validity against the relay slot.
/// If you don't need to have slots at parachain level, you can use the `RelaySlot` implementation.
pub trait GetAndVerifySlot {
	/// Get the current slot
	fn get_and_verify_slot(relay_chain_slot: &Slot) -> Result<Slot, ()>;
}

/// Parachain slot implementation that use the relay chain slot directly
pub struct RelaySlot;
impl GetAndVerifySlot for RelaySlot {
	fn get_and_verify_slot(relay_chain_slot: &Slot) -> Result<Slot, ()> {
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
	fn get_and_verify_slot(relay_chain_slot: &Slot) -> Result<Slot, ()> {
		// Convert relay chain timestamp.
		let relay_chain_timestamp =
			u64::from(RELAY_CHAIN_SLOT_DURATION_MILLIS).saturating_mul((*relay_chain_slot).into());

		let (new_slot, para_slot_duration) = SlotProvider::get();

		let para_slot_from_relay =
			Slot::from_timestamp(relay_chain_timestamp.into(), para_slot_duration);

		if new_slot == para_slot_from_relay {
			Ok(new_slot)
		} else {
			Err(())
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
		type ExpectedBlockTime: Get<Self::Moment>;
	}

	/// First tuple element is the highest slot that has been seen in the history of this chain.
	/// Second tuple element is the number of authored blocks so far.
	/// This is a strictly-increasing value if T::AllowMultipleBlocksPerSlot = false.
	#[pallet::storage]
	#[pallet::getter(fn slot_info)]
	pub type SlotInfo<T: Config> = StorageValue<_, (Slot, u32), OptionQuery>;
}
