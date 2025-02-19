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

use crate::{self as pallet_testing, AccountLookup, NimbusId};
use frame_support::parameter_types;
use frame_support::traits::ConstU32;
use frame_support::weights::RuntimeDbWeight;
use frame_system;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
		AuthorInherent: pallet_testing::{Pallet, Call, Storage},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub Authors: Vec<u64> = vec![1, 2, 3, 4, 5];
	pub const TestDbWeight: RuntimeDbWeight = RuntimeDbWeight {
		read: 1,
		write: 10,
	};
}

impl frame_system::Config for Test {
	type ExtensionsWeightInfo = ();
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = TestDbWeight;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type Nonce = u64;
	type Block = Block;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type RuntimeEvent = RuntimeEvent;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
	type RuntimeTask = ();
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}

pub struct DummyBeacon {}
impl nimbus_primitives::SlotBeacon for DummyBeacon {
	fn slot() -> u32 {
		0
	}
}

pub const ALICE: u64 = 1;
pub const ALICE_NIMBUS: [u8; 32] = [1; 32];
pub struct MockAccountLookup;
impl AccountLookup<u64> for MockAccountLookup {
	fn lookup_account(nimbus_id: &NimbusId) -> Option<u64> {
		let nimbus_id_bytes: &[u8] = nimbus_id.as_ref();

		if nimbus_id_bytes == &ALICE_NIMBUS {
			Some(ALICE)
		} else {
			None
		}
	}
}

impl pallet_testing::Config for Test {
	type AuthorId = u64;
	type AccountLookup = MockAccountLookup;
	type CanAuthor = ();
	type SlotBeacon = DummyBeacon;
	type WeightInfo = ();
}

/// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	frame_system::GenesisConfig::<Test>::default()
		.build_storage()
		.unwrap()
		.into()
}
