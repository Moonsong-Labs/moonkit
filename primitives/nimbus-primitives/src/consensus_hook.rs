use cumulus_pallet_parachain_system::{
	self as parachain_system,
	consensus_hook::{ConsensusHook, UnincludedSegmentCapacity},
	relay_state_snapshot::RelayChainStateProof,
};
use sp_std::{marker::PhantomData, num::NonZeroU32};
use sp_consensus_slots::{Slot, SlotDuration};
use frame_support::pallet_prelude::*;

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
		const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32,
		const V: u32,
		const C: u32,
	> ConsensusHook for FixedVelocityConsensusHook<T, RELAY_CHAIN_SLOT_DURATION_MILLIS, V, C>
{
	// Validates the number of authored blocks within the slot with respect to the `V + 1` limit.
	fn on_state_proof(state_proof: &RelayChainStateProof) -> (Weight, UnincludedSegmentCapacity) {
		// Ensure velocity is non-zero.
		let velocity = V.max(1);
		let relay_chain_slot = state_proof.read_slot().expect("failed to read relay chain slot");

		/* let (slot, authored) = pallet::Pallet::<T>::slot_info()
			.expect("slot info is inserted on block initialization"); */

 		// Convert relay chain timestamp.
		let relay_chain_timestamp =
			u64::from(RELAY_CHAIN_SLOT_DURATION_MILLIS).saturating_mul(*relay_chain_slot);
        /*
		let para_slot_duration = SlotDuration::from_millis(Aura::<T>::slot_duration().into());
		let para_slot_from_relay =
			Slot::from_timestamp(relay_chain_timestamp.into(), para_slot_duration);

		// Perform checks.
		assert_eq!(slot, para_slot_from_relay, "slot number mismatch");
		if authored > velocity + 1 {
			panic!("authored blocks limit is reached for the slot")
		} */
		let weight = T::DbWeight::get().reads(1);

		(
			weight,
			NonZeroU32::new(sp_std::cmp::max(C, 1))
				.expect("1 is the minimum value and non-zero; qed")
				.into(),
		)
	}
}

/* impl<
		T: parachain_system::Config,
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
			return false
		}

		if last_slot == new_slot {
			authored_so_far < velocity + 1
		} else {
			// disallow slot from moving backwards.
			last_slot < new_slot
		}
	}
} */