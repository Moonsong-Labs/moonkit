// Copyright 2019-2022 Moonsong Labs
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

//! Unit testing
use crate::mock::{
	events, ExtBuilder, MaintenanceMode, RuntimeCall as OuterCall, RuntimeOrigin, Test,
};
use crate::{Call, Error, Event};
use cumulus_primitives_core::AggregateMessageOrigin;
use frame_support::traits::QueuePausedQuery;
use frame_support::{assert_noop, assert_ok};
use sp_runtime::traits::Dispatchable;

#[test]
fn can_remark_during_normal_operation() {
	ExtBuilder::default().build().execute_with(|| {
		let call: OuterCall = frame_system::Call::remark { remark: vec![] }.into();
		assert_ok!(call.dispatch(RuntimeOrigin::signed(1)));
	})
}

#[test]
fn cannot_remark_during_maintenance_mode() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			let call: OuterCall = frame_system::Call::remark { remark: vec![] }.into();
			assert_noop!(
				call.dispatch(RuntimeOrigin::signed(1)),
				frame_system::Error::<Test>::CallFiltered
			);
		})
}

#[test]
fn can_enter_maintenance_mode() {
	ExtBuilder::default().build().execute_with(|| {
		let call: OuterCall = Call::enter_maintenance_mode {}.into();
		assert_ok!(call.dispatch(RuntimeOrigin::root()));

		assert_eq!(events(), vec![Event::EnteredMaintenanceMode,]);
	})
}

#[test]
fn cannot_enter_maintenance_mode_from_wrong_origin() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			let call: OuterCall = Call::enter_maintenance_mode {}.into();
			assert_noop!(
				call.dispatch(RuntimeOrigin::signed(1)),
				frame_system::Error::<Test>::CallFiltered
			);
		})
}

#[test]
fn cannot_enter_maintenance_mode_when_already_in_it() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			let call: OuterCall = Call::enter_maintenance_mode {}.into();
			assert_noop!(
				call.dispatch(RuntimeOrigin::root()),
				Error::<Test>::AlreadyInMaintenanceMode
			);
		})
}

#[test]
fn can_resume_normal_operation() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			let call: OuterCall = Call::resume_normal_operation {}.into();
			assert_ok!(call.dispatch(RuntimeOrigin::root()));

			assert_eq!(events(), vec![Event::NormalOperationResumed,]);
		})
}

#[test]
fn cannot_resume_normal_operation_from_wrong_origin() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			let call: OuterCall = Call::resume_normal_operation {}.into();
			assert_noop!(
				call.dispatch(RuntimeOrigin::signed(1)),
				frame_system::Error::<Test>::CallFiltered
			);
		})
}

#[test]
fn cannot_resume_normal_operation_while_already_operating_normally() {
	ExtBuilder::default().build().execute_with(|| {
		let call: OuterCall = Call::resume_normal_operation {}.into();
		assert_noop!(
			call.dispatch(RuntimeOrigin::root()),
			Error::<Test>::NotInMaintenanceMode
		);
	})
}

#[test]
fn can_do_enter_maintenance_mode() {
	ExtBuilder::default()
		.with_maintenance_mode(false)
		.build()
		.execute_with(|| {
			MaintenanceMode::do_enter_maintenance_mode();
			assert!(MaintenanceMode::maintenance_mode());
			assert_eq!(events(), vec![Event::EnteredMaintenanceMode,]);
		})
}

#[test]
fn do_enter_maintenance_mode_when_already_in_it() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			MaintenanceMode::do_enter_maintenance_mode();
			assert!(MaintenanceMode::maintenance_mode());
			assert_eq!(events(), vec![Event::EnteredMaintenanceMode,]);
		})
}

#[cfg(feature = "xcm-support")]
#[test]
fn queue_pause_in_non_maintenance() {
	ExtBuilder::default()
		.with_maintenance_mode(false)
		.build()
		.execute_with(|| {
			assert_eq!(
				MaintenanceMode::is_paused(&AggregateMessageOrigin::Here),
				false
			);
		})
}

#[cfg(feature = "xcm-support")]
#[test]
fn queue_pause_in_maintenance() {
	ExtBuilder::default()
		.with_maintenance_mode(true)
		.build()
		.execute_with(|| {
			assert!(MaintenanceMode::is_paused(&AggregateMessageOrigin::Here),);
		})
}
