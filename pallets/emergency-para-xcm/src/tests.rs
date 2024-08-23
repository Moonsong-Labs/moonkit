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
use crate::mock::*;
use crate::*;
use cumulus_primitives_core::AggregateMessageOrigin;
use frame_support::{assert_noop, assert_ok};
use sp_core::Hasher;
use sp_runtime::traits::Dispatchable;

#[test]
fn remains_in_normal_mode_when_relay_block_diff_is_within_threshold() {
	new_test_ext().execute_with(|| {
		let previous_block = 1;
		let current_block = PAUSED_THRESHOLD + previous_block;
		EmergencyParaXcm::check_associated_relay_number(current_block, previous_block);
		let xcm_mode = Mode::<Test>::get();
		assert!(xcm_mode == XcmMode::Normal);
	})
}

#[test]
fn remains_in_normal_mode_if_previous_relay_block_is_0() {
	new_test_ext().execute_with(|| {
		let previous_block = 0;
		let current_block = PAUSED_THRESHOLD + previous_block + 1;
		EmergencyParaXcm::check_associated_relay_number(current_block, previous_block);
		let xcm_mode = Mode::<Test>::get();
		assert!(xcm_mode == XcmMode::Normal);
	})
}

#[test]
fn pauses_xcm_when_relay_block_diff_is_above_threshold() {
	new_test_ext().execute_with(|| {
		let previous_block = 1;
		let current_block = PAUSED_THRESHOLD + previous_block + 1;
		EmergencyParaXcm::check_associated_relay_number(current_block, previous_block);
		let xcm_mode = Mode::<Test>::get();
		assert!(xcm_mode == XcmMode::Paused);
		assert_eq!(events(), vec![Event::EnteredPausedXcmMode,]);
	})
}

#[test]
fn cannot_go_from_paused_to_normal_with_wrong_origin() {
	new_test_ext().execute_with(|| {
		Mode::<Test>::set(XcmMode::Paused);
		let call: RuntimeCall = Call::paused_to_normal {}.into();
		assert_noop!(
			call.dispatch(RuntimeOrigin::signed(1)),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn cannot_go_from_paused_to_normal_when_not_paused() {
	new_test_ext().execute_with(|| {
		let call: RuntimeCall = Call::paused_to_normal {}.into();
		assert_noop!(
			call.dispatch(RuntimeOrigin::root()),
			Error::<Test>::NotInPausedMode
		);
	});
}

#[test]
fn can_go_from_paused_to_normal() {
	new_test_ext().execute_with(|| {
		Mode::<Test>::set(XcmMode::Paused);
		let call: RuntimeCall = Call::paused_to_normal {}.into();
		assert_ok!(call.dispatch(RuntimeOrigin::root()));
		let xcm_mode = Mode::<Test>::get();
		assert!(xcm_mode == XcmMode::Normal);
		assert_eq!(events(), vec![Event::NormalXcmOperationResumed,]);
	});
}

#[test]
fn delegates_queue_paused_query_on_normal_mode() {
	new_test_ext().execute_with(|| {
		let paused = EmergencyParaXcm::is_paused(&AggregateMessageOrigin::Parent);
		assert!(!paused);
	});
}

#[test]
fn pauses_queue_on_paused_mode() {
	new_test_ext().execute_with(|| {
		Mode::<Test>::set(XcmMode::Paused);
		let paused = EmergencyParaXcm::is_paused(&AggregateMessageOrigin::Parent);
		assert!(paused);
	});
}

#[test]
fn cannot_authorize_upgrade_with_wrong_origin() {
	new_test_ext().execute_with(|| {
		Mode::<Test>::set(XcmMode::Paused);
		let runtime = substrate_test_runtime_client::runtime::wasm_binary_unwrap().to_vec();
		let hash = <Test as frame_system::Config>::Hashing::hash(&runtime);
		let call: RuntimeCall = Call::fast_authorize_upgrade { code_hash: hash }.into();
		assert_noop!(
			call.dispatch(RuntimeOrigin::signed(1)),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn cannot_authorize_upgrade_on_normal_mode() {
	new_test_ext().execute_with(|| {
		let runtime = substrate_test_runtime_client::runtime::wasm_binary_unwrap().to_vec();
		let hash = <Test as frame_system::Config>::Hashing::hash(&runtime);
		let call: RuntimeCall = Call::fast_authorize_upgrade { code_hash: hash }.into();
		assert_noop!(
			call.dispatch(RuntimeOrigin::root()),
			Error::<Test>::NotInPausedMode
		);
	});
}

#[test]
fn can_authorize_upgrade_on_paused_mode() {
	new_test_ext().execute_with(|| {
		Mode::<Test>::set(XcmMode::Paused);
		let runtime = substrate_test_runtime_client::runtime::wasm_binary_unwrap().to_vec();
		let hash = <Test as frame_system::Config>::Hashing::hash(&runtime);
		let call: RuntimeCall = Call::fast_authorize_upgrade { code_hash: hash }.into();
		assert_ok!(call.dispatch(RuntimeOrigin::root()));
		assert!(System::authorized_upgrade().is_some())
	});
}
