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
use cumulus_pallet_parachain_system::RelayChainState;

#[test]
fn can_call_inherent_twice_with_same_relay_block() {
	ExtBuilder::default()
		.with_balances(vec![(ALICE, 15)])
		.build()
		.execute_with(|| {
			let relay_state = RelayChainState {
				number: 1,
				state_root: H256::default(),
			};
			set_current_relay_chain_state(relay_state);
			Pallet::<Test>::set_relay_storage_root();
			Pallet::<Test>::set_relay_storage_root();

			// Only the first item has been inserted
			assert_eq!(
				u32::try_from(RelayStorageRootKeys::<Test>::get().len()).unwrap(),
				1
			);
		});
}

#[test]
fn oldest_items_are_removed_first() {
	ExtBuilder::default()
		.with_balances(vec![(ALICE, 15)])
		.build()
		.execute_with(|| {
			fill_relay_storage_roots::<Test>();
			let keys = RelayStorageRootKeys::<Test>::get();
			assert_eq!(
				u32::try_from(keys.len()).unwrap(),
				<Test as Config>::MaxStorageRoots::get()
			);
			assert_eq!(keys[0], 0);
			assert!(RelayStorageRoot::<Test>::get(0).is_some());

			let relay_state = RelayChainState {
				number: 1000,
				state_root: H256::default(),
			};
			set_current_relay_chain_state(relay_state);
			Pallet::<Test>::set_relay_storage_root();

			// Only the first item has been removed
			let keys = RelayStorageRootKeys::<Test>::get();
			assert_eq!(
				u32::try_from(keys.len()).unwrap(),
				<Test as Config>::MaxStorageRoots::get()
			);
			assert_eq!(keys[0], 1);
			assert!(RelayStorageRoot::<Test>::get(0).is_none());
		});
}

#[test]
fn little_endian_keys_are_handled_properly() {
	ExtBuilder::default()
		.with_balances(vec![(ALICE, 15)])
		.build()
		.execute_with(|| {
			// If the keys were stored in little endian, 256 would be removed right after being
			// inserted
			for i in 250..260 {
				let relay_state = RelayChainState {
					number: i,
					state_root: H256::default(),
				};
				set_current_relay_chain_state(relay_state);
				Pallet::<Test>::set_relay_storage_root();
			}

			// Only the first item has been removed
			let keys = RelayStorageRootKeys::<Test>::get();
			assert_eq!(
				u32::try_from(keys.len()).unwrap(),
				<Test as Config>::MaxStorageRoots::get()
			);
			assert_eq!(keys, vec![256, 257, 258, 259]);
		});
}

#[test]
fn bounded_vec_limit_higher_than_max_storage_roots() {
	ExtBuilder::default()
		.with_balances(vec![(ALICE, 15)])
		.build()
		.execute_with(|| {
			// Insert a 0 that will get overwritten by `storage::unhashed::put_raw` below, but this
			// way we know we got the right key
			let relay_state = RelayChainState {
				number: 0,
				state_root: H256::default(),
			};
			set_current_relay_chain_state(relay_state);
			// Create a bounded vec with a limit higher than `MaxStorageRoots`
			let vec1: BoundedVec<u8, ConstU32<20>> =
				BoundedVec::truncate_from((0u8..30).into_iter().collect());
			let bytes = vec1.encode();
			let raw_key = frame_support::storage::storage_prefix(
				b"RelayStorageRoots",
				b"RelayStorageRootKeys",
			);
			frame_support::storage::unhashed::put_raw(&raw_key, bytes.as_slice());

			// Now insert a new relay storage root
			let relay_state = RelayChainState {
				number: 31,
				state_root: H256::default(),
			};
			set_current_relay_chain_state(relay_state);
			Pallet::<Test>::set_relay_storage_root();

			// The storage is considered corrupted, so only the new relay number will be stored.
			// The storage map will still contain some of the old values, need to be manually
			// cleaned up.
			let keys = RelayStorageRootKeys::<Test>::get();
			assert_eq!(keys, vec![31]);
		});
}
