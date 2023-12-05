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

use crate::consensus_hook::FixedVelocityConsensusHook;
use crate::mock::*;
use std::ops::Deref;

const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32 = 6_000;

type ConsensusHook = FixedVelocityConsensusHook<Test, RELAY_CHAIN_SLOT_DURATION_MILLIS, 1, 1>;

fn assert_slot_info_eq(slot_number: u64, authored: u32) {
	assert_eq!(AsyncBacking::slot_info().unwrap().0.deref(), &slot_number);
	assert_eq!(AsyncBacking::slot_info().unwrap().1, authored);
}

#[test]
fn on_state_proof_inner_update_slot_info() {
	new_test_ext().execute_with(|| {
		assert_eq!(AsyncBacking::slot_info(), None);
		ConsensusHook::on_state_proof_inner(1.into());
		assert_slot_info_eq(1, 1);
	});
}

#[test]
fn can_produce_2_parablock_per_relay_slot() {
	new_test_ext().execute_with(|| {
		assert_eq!(AsyncBacking::slot_info(), None);
		ConsensusHook::on_state_proof_inner(1.into());
		assert_slot_info_eq(1, 1);
		ConsensusHook::on_state_proof_inner(1.into());
		assert_slot_info_eq(1, 2);
	});
}

#[test]
#[should_panic = "authored blocks limit is reached for the slot"]
fn can_not_produce_3_parablock_per_relay_slot() {
	new_test_ext().execute_with(|| {
		assert_eq!(AsyncBacking::slot_info(), None);
		ConsensusHook::on_state_proof_inner(1.into());
		assert_slot_info_eq(1, 1);
		ConsensusHook::on_state_proof_inner(1.into());
		assert_slot_info_eq(1, 2);
		ConsensusHook::on_state_proof_inner(1.into());
	});
}

#[test]
fn can_skip_a_relay_slot() {
	new_test_ext().execute_with(|| {
		assert_eq!(AsyncBacking::slot_info(), None);
		ConsensusHook::on_state_proof_inner(1.into());
		assert_slot_info_eq(1, 1);
		ConsensusHook::on_state_proof_inner(3.into());
		assert_slot_info_eq(3, 1);
	});
}
