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

use super::*;
use crate as pallet_emergency_para_xcm;
use cumulus_pallet_parachain_system::ParachainSetCode;
use cumulus_primitives_core::{
	relay_chain::BlockNumber as RelayBlockNumber, AggregateMessageOrigin, ParaId,
	XcmpMessageHandler,
};
use frame_support::parameter_types;
use frame_support::traits::ConstU32;
use frame_support::weights::Weight;
use frame_system::EnsureRoot;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
		ParachainSystem: cumulus_pallet_parachain_system::{Pallet, Call, Storage, Inherent, Event<T>, Config<T>},
		MessageQueue: pallet_message_queue::{Pallet, Call, Storage, Event<T>},
		EmergencyParaXcm: pallet_emergency_para_xcm::{Pallet, Call, Storage, Event},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
}

pub type AccountId = u64;

impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type Nonce = u64;
	type Block = Block;
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
	type SS58Prefix = ();
	type OnSetCode = ParachainSetCode<Self>;
	type MaxConsumers = ConstU32<16>;
	type RuntimeTask = ();
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}

parameter_types! {
	pub ParachainId: ParaId = 100.into();
	pub const RelayOrigin: AggregateMessageOrigin = AggregateMessageOrigin::Parent;
}

impl cumulus_pallet_parachain_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type SelfParaId = ParachainId;
	type OnSystemEvent = ();
	type OutboundXcmpMessageSource = ();
	type XcmpMessageHandler = JustConsumeAllWeight;
	type ReservedXcmpWeight = ();
	type DmpQueue = frame_support::traits::EnqueueWithOrigin<MessageQueue, RelayOrigin>;
	type ReservedDmpWeight = ();
	type CheckAssociatedRelayNumber = EmergencyParaXcm;
	type WeightInfo = ();
	type ConsensusHook = cumulus_pallet_parachain_system::ExpectParentIncluded;
}

parameter_types! {
	pub const MessageQueueMaxStale: u32 = 8;
	pub const MessageQueueHeapSize: u32 = 64 * 1024;
	pub const MaxWeight: Weight = Weight::MAX;
}

impl pallet_message_queue::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type MessageProcessor =
		pallet_message_queue::mock_helpers::NoopMessageProcessor<AggregateMessageOrigin>;
	type Size = u32;
	type HeapSize = MessageQueueHeapSize;
	type MaxStale = MessageQueueMaxStale;
	type ServiceWeight = MaxWeight;
	type IdleMaxServiceWeight = MaxWeight;
	type QueueChangeHandler = ();
	type QueuePausedQuery = EmergencyParaXcm;
	type WeightInfo = ();
}

pub(crate) const PAUSED_THRESHOLD: u32 = 5;

impl Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type CheckAssociatedRelayNumber = cumulus_pallet_parachain_system::AnyRelayNumber;
	type QueuePausedQuery = ();
	type PausedThreshold = ConstU32<PAUSED_THRESHOLD>;
	type FastAuthorizeUpgradeOrigin = EnsureRoot<AccountId>;
	type PausedToNormalOrigin = EnsureRoot<AccountId>;
}

pub struct JustConsumeAllWeight;

impl XcmpMessageHandler for JustConsumeAllWeight {
	fn handle_xcmp_messages<'a, I: Iterator<Item = (ParaId, RelayBlockNumber, &'a [u8])>>(
		_iter: I,
		max_weight: Weight,
	) -> Weight {
		max_weight
	}
}

/// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	let storage = frame_system::GenesisConfig::<Test>::default()
		.build_storage()
		.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| System::set_block_number(1));
	ext
}

pub(crate) fn events() -> Vec<pallet_emergency_para_xcm::Event> {
	System::events()
		.into_iter()
		.map(|r| r.event)
		.filter_map(|e| {
			if let RuntimeEvent::EmergencyParaXcm(inner) = e {
				Some(inner)
			} else {
				None
			}
		})
		.collect::<Vec<_>>()
}
