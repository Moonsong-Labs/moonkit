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

#[cfg(tests)]
type RelayChainStateProof = crate::mock::FakeRelayChainStateProof;

/// A consensus hook for a fixed block processing velocity and unincluded segment capacity.
///
/// Relay chain slot duration must be provided in milliseconds.
pub struct FixedVelocityConsensusHook<T, const V: u32, const C: u32>(PhantomData<T>);

impl<T: pallet::Config, const V: u32, const C: u32> ConsensusHook
	for FixedVelocityConsensusHook<T, V, C>
where
	<T as pallet_timestamp::Config>::Moment: Into<u64>,
{
	// Validates the number of authored blocks within the slot with respect to the `V + 1` limit.
	fn on_state_proof(state_proof: &RelayChainStateProof) -> (Weight, UnincludedSegmentCapacity) {
		let relay_chain_slot = state_proof
			.read_slot()
			.expect("failed to read relay chain slot");

		Self::on_state_proof_inner(relay_chain_slot)
	}
}

impl<T: pallet::Config, const V: u32, const C: u32> FixedVelocityConsensusHook<T, V, C>
where
	<T as pallet_timestamp::Config>::Moment: Into<u64>,
{
	pub(crate) fn on_state_proof_inner(
		relay_chain_slot: cumulus_primitives_core::relay_chain::Slot,
	) -> (Weight, UnincludedSegmentCapacity) {
		// Ensure velocity is non-zero.
		let velocity = V.max(1);

		// Get and verify the parachain slot
		let new_slot = T::GetAndVerifySlot::get_and_verify_slot(&relay_chain_slot)
			.expect("slot number mismatch");

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

		// Perform checks.
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

impl<T: pallet::Config + parachain_system::Config, const V: u32, const C: u32>
	FixedVelocityConsensusHook<T, V, C>
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
