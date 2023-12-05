// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Cumulus.

// Cumulus is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Cumulus is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Cumulus.  If not, see <http://www.gnu.org/licenses/>.

//! The definition of a [`FixedVelocityConsensusHook`] for consensus logic to manage
//! block velocity.
//!
//! The velocity `V` refers to the rate of block processing by the relay chain.

use crate::*;
use cumulus_pallet_parachain_system::{
	self as parachain_system,
	consensus_hook::{ConsensusHook, UnincludedSegmentCapacity},
	relay_state_snapshot::RelayChainStateProof,
};
use frame_support::pallet_prelude::*;
use sp_consensus_slots::Slot;
use sp_std::{marker::PhantomData, num::NonZeroU32};

/// A consensus hook for a fixed block processing velocity and unincluded segment capacity.
///
/// Relay chain slot duration must be provided in milliseconds.
pub struct FixedVelocityConsensusHook<
	T,
	const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32,
	const V: u32,
	const C: u32,
>(PhantomData<T>);

impl<
		T: pallet::Config,
		const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32,
		const V: u32,
		const C: u32,
	> ConsensusHook for FixedVelocityConsensusHook<T, RELAY_CHAIN_SLOT_DURATION_MILLIS, V, C>
where
	<T as pallet_timestamp::Config>::Moment: Into<u64>,
{
	// Validates the number of authored blocks within the slot with respect to the `V + 1` limit.
	fn on_state_proof(state_proof: &RelayChainStateProof) -> (Weight, UnincludedSegmentCapacity) {
		// Ensure velocity is non-zero.
		let velocity = V.max(1);
		let relay_chain_slot = state_proof
			.read_slot()
			.expect("failed to read relay chain slot");

		// Convert relay chain timestamp.
		let relay_chain_timestamp =
			u64::from(RELAY_CHAIN_SLOT_DURATION_MILLIS).saturating_mul(*relay_chain_slot);

		let new_slot = T::ParachainSlot::get_current_slot().unwrap_or(relay_chain_slot);

		// Update Slot Info
		let authored = match SlotInfo::<T>::get() {
			Some((slot, authored)) if slot == new_slot => {
				if !T::AllowMultipleBlocksPerSlot::get() {
					panic!("Block invalid; Supplied slot number is not high enough");
				}
				authored + 1
			}
			Some((slot, _)) if slot < new_slot => 1,
			Some(..) => {
				panic!("slot moved backwards")
			}
			None => 1,
		};

		let para_slot_from_relay =
			T::ParachainSlot::para_slot_from_relay_timestamp(relay_chain_timestamp)
				.unwrap_or(relay_chain_slot);

		// Perform checks.
		assert_eq!(new_slot, para_slot_from_relay, "slot number mismatch");
		if authored > velocity + 1 {
			panic!("authored blocks limit is reached for the slot")
		}

		// Store new slot info
		SlotInfo::<T>::put((new_slot, authored));

		// Account weights
		let weight = T::DbWeight::get().reads_writes(1, 1);

		// Return weight and unincluded segment capacity
		(
			weight,
			NonZeroU32::new(sp_std::cmp::max(C, 1))
				.expect("1 is the minimum value and non-zero; qed")
				.into(),
		)
	}
}

impl<
		T: pallet::Config + parachain_system::Config,
		const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32,
		const V: u32,
		const C: u32,
	> FixedVelocityConsensusHook<T, RELAY_CHAIN_SLOT_DURATION_MILLIS, V, C>
{
	/// Whether it is legal to extend the chain, assuming the given block is the most
	/// recently included one as-of the relay parent that will be built against, and
	/// the given slot.
	///
	/// This should be consistent with the logic the runtime uses when validating blocks to
	/// avoid issues.
	///
	/// When the unincluded segment is empty, i.e. `included_hash == at`, where at is the block
	/// whose state we are querying against, this must always return `true` as long as the slot
	/// is more recent than the included block itself.
	pub fn can_build_upon(included_hash: T::Hash, new_slot: Slot) -> bool {
		let velocity = V.max(1);
		let (last_slot, authored_so_far) = match pallet::Pallet::<T>::slot_info() {
			None => return true,
			Some(x) => x,
		};

		let size_after_included =
			parachain_system::Pallet::<T>::unincluded_segment_size_after(included_hash);

		// can never author when the unincluded segment is full.
		if size_after_included >= C {
			return false;
		}

		if last_slot == new_slot {
			authored_so_far < velocity + 1
		} else {
			// disallow slot from moving backwards.
			last_slot < new_slot
		}
	}
}
