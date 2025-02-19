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

//! A minimal runtime including the maintenance-mode pallet
use super::*;
use crate as pallet_maintenance_mode;
use frame_support::{
	construct_runtime, parameter_types,
	traits::{Contains, Everything},
	weights::Weight,
};
use frame_system::EnsureRoot;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage, Perbill,
};

//TODO use TestAccount once it is in a common place (currently it lives with democracy precompiles)
pub type AccountId = u64;

type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
construct_runtime!(
	pub enum Test
	{
		System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
		MaintenanceMode: pallet_maintenance_mode::{Pallet, Call, Storage, Event, Config<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u32 = 250;
	pub const MaximumBlockWeight: Weight = Weight::from_parts(1024, 1);
	pub const MaximumBlockLength: u32 = 2 * 1024;
	pub const AvailableBlockRatio: Perbill = Perbill::one();
	pub const SS58Prefix: u8 = 42;
}
impl frame_system::Config for Test {
	type ExtensionsWeightInfo = ();
	type BaseCallFilter = MaintenanceMode;
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type Nonce = u64;
	type Block = Block;
	type RuntimeCall = RuntimeCall;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type RuntimeEvent = RuntimeEvent;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type BlockWeights = ();
	type BlockLength = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
	type RuntimeTask = ();
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}

/// During maintenance mode we will not allow any calls.
pub struct MaintenanceCallFilter;
impl Contains<RuntimeCall> for MaintenanceCallFilter {
	fn contains(_: &RuntimeCall) -> bool {
		false
	}
}

impl Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type NormalCallFilter = Everything;
	type MaintenanceCallFilter = MaintenanceCallFilter;
	type MaintenanceOrigin = EnsureRoot<AccountId>;
	#[cfg(feature = "xcm-support")]
	type XcmExecutionManager = ();
}

/// Externality builder for pallet maintenance mode's mock runtime
pub(crate) struct ExtBuilder {
	maintenance_mode: bool,
}

impl Default for ExtBuilder {
	fn default() -> ExtBuilder {
		ExtBuilder {
			maintenance_mode: false,
		}
	}
}

impl ExtBuilder {
	pub(crate) fn with_maintenance_mode(mut self, m: bool) -> Self {
		self.maintenance_mode = m;
		self
	}

	pub(crate) fn build(self) -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Test>::default()
			.build_storage()
			.expect("Frame system builds valid default genesis config");

		pallet_maintenance_mode::GenesisConfig::<Test>::assimilate_storage(
			&pallet_maintenance_mode::GenesisConfig {
				start_in_maintenance_mode: self.maintenance_mode,
				..Default::default()
			},
			&mut t,
		)
		.expect("Pallet maintenance mode storage can be assimilated");

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}
}

pub(crate) fn events() -> Vec<pallet_maintenance_mode::Event> {
	System::events()
		.into_iter()
		.map(|r| r.event)
		.filter_map(|e| {
			if let RuntimeEvent::MaintenanceMode(inner) = e {
				Some(inner)
			} else {
				None
			}
		})
		.collect::<Vec<_>>()
}
