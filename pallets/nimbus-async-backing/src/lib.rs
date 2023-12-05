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

//! Pallet that allows block authors to include their identity in a block via an inherent.
//! Currently the author does not _prove_ their identity, just states it. So it should not be used,
//! for things like equivocation slashing that require authenticated authorship information.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod consensus_hook;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use pallet::*;

use frame_support::pallet_prelude::*;
use sp_consensus_slots::Slot;

/// The InherentIdentifier for nimbus's extension inherent
pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"nimb-ext";

/// Parachain slot implementation
/// If you don't need to have slots at parachain level, you can use the `RelaySlot` implementation.
pub trait ParachainSlot {
	/// Get the current slot
	fn get_current_slot() -> Option<Slot>;
	/// Compute parachain slot from relay chain timestamp
	fn para_slot_from_relay_timestamp(relay_chain_timestamp: u64) -> Option<Slot>;
}

/// Parachain slot implementation that use the relay chain slot directly
pub struct RelaySlot;
impl ParachainSlot for RelaySlot {
	fn get_current_slot() -> Option<Slot> {
		None
	}
	fn para_slot_from_relay_timestamp(_: u64) -> Option<Slot> {
		None
	}
}

/*/// Fixed time slot
struct FixedSlot<GetSlotDuration>(PhantomData<GetSlotDuration>);

impl<GetSlotDuration: Get<u64>> ParachainSlot for FixedSlot<GetSlotDuration> {
	fn para_slot_from_relay_timestamp(relay_chain_timestamp: u64) -> Slot {
		let para_slot_duration = SlotDuration::from_millis(GetSlotDuration::get());
		Slot::from_timestamp(relay_chain_timestamp.into(), para_slot_duration)
	}
}*/

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

		/// Parachain slot implementation
		type ParachainSlot: ParachainSlot;
	}

	/// First tuple element is the highest slot that has been seen in the history of this chain.
	/// Second tuple element is the number of authored blocks so far.
	/// This is a strictly-increasing value if T::AllowMultipleBlocksPerSlot = false.
	#[pallet::storage]
	#[pallet::getter(fn slot_info)]
	pub type SlotInfo<T: Config> = StorageValue<_, (Slot, u32), OptionQuery>;
}
