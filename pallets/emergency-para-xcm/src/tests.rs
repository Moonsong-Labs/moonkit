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
use frame_support::{assert_noop, assert_ok};
use sp_runtime::traits::Dispatchable;

#[test]
fn remains_in_normal_mode_when_relay_block_diff_is_within_threshold() {
	ExtBuilder {}.build().execute_with(|| {
		let current_block = 1;
		let new_block = PAUSED_THRESHOLD + current_block;
		EmergencyParaXcm::check_associated_relay_number(new_block, current_block);
		let xcm_mode = EmergencyParaXcm::mode();
		assert!(xcm_mode == XcmMode::Normal);
	})
}

#[test]
fn pauses_xcm_when_relay_block_diff_is_above_threshold() {
	ExtBuilder {}.build().execute_with(|| {
		let current_block = 1;
		let new_block = PAUSED_THRESHOLD + current_block + 1;
		EmergencyParaXcm::check_associated_relay_number(new_block, current_block);
		let xcm_mode = EmergencyParaXcm::mode();
		assert!(xcm_mode == XcmMode::Paused);
		assert_eq!(events(), vec![Event::EnteredPausedXcmMode,]);
	})
}

#[test]
fn cannot_go_from_paused_to_normal_with_wrong_origin() {
	ExtBuilder {}.build().execute_with(|| {
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
	ExtBuilder {}.build().execute_with(|| {
		let call: RuntimeCall = Call::paused_to_normal {}.into();
		assert_noop!(
			call.dispatch(RuntimeOrigin::root()),
			Error::<Test>::NotInPausedMode
		);
	});
}

#[test]
fn can_go_from_paused_to_normal() {
	ExtBuilder {}.build().execute_with(|| {
		Mode::<Test>::set(XcmMode::Paused);
		let call: RuntimeCall = Call::paused_to_normal {}.into();
		assert_ok!(call.dispatch(RuntimeOrigin::root()));
		let xcm_mode = EmergencyParaXcm::mode();
		assert!(xcm_mode == XcmMode::Normal);
	});
}
