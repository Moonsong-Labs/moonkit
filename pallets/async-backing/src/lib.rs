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

		/// The slot duration Nimbus should run with, expressed in milliseconds.
		/// The effective value of this type should not change while the chain is running.
		#[pallet::constant]
		type SlotDuration: Get<Self::Moment>;

		/// TODO: Remove this constant once chopsticks has been updated
		/// - https://github.com/AcalaNetwork/chopsticks/blob/1a84b55097d2efdfaee64964b4b36af7c741d854/packages/core/src/utils/index.ts#L132
		///
		/// Purely informative, but used by mocking tools like chospticks to allow knowing how to mock
		/// blocks
		#[pallet::constant]
		type ExpectedBlockTime: Get<Self::Moment>;
	}

	/// Current relay chain slot paired with a number of authored blocks.
	///
	/// This is updated in [`FixedVelocityConsensusHook::on_state_proof`] with the current relay
	/// chain slot as provided by the relay chain state proof.
	#[pallet::storage]
	#[pallet::getter(fn slot_info)]
	pub type SlotInfo<T: Config> = StorageValue<_, (Slot, u32), OptionQuery>;
}

impl<T: Config> frame_support::traits::PostInherents for Pallet<T> {
	fn post_inherents() {
		let slot_duration = T::SlotDuration::get();
		assert!(
			!slot_duration.is_zero(),
			"Nimbus slot duration cannot be zero."
		);
		// Get the parachain slot
		let now = pallet_timestamp::Now::<T>::get();
		let timestamp_slot = now / slot_duration;
		let timestamp_slot = Slot::from(timestamp_slot.saturated_into::<u64>());

		// Get the relay chain slot
		let (current_slot, _) = SlotInfo::<T>::get().expect("Relay slot to exist");

		assert_eq!(
			current_slot, timestamp_slot,
			"The parachain timestamp slot must match the relay chain slot. This likely means that the configured block \
			time in the node and/or rest of the runtime is not compatible with Nimbus's `SlotDuration`",
		);
	}
}
