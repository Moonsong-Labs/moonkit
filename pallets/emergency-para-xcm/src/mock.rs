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
use cumulus_pallet_parachain_system::{CheckAssociatedRelayNumber, ParachainSetCode};
use cumulus_primitives_core::{
	relay_chain::BlockNumber as RelayBlockNumber, AggregateMessageOrigin, InboundDownwardMessage,
	InboundHrmpMessage, ParaId, PersistedValidationData,
};
use frame_support::parameter_types;
use frame_support::traits::{ConstU32, ProcessMessage, ProcessMessageError};
use frame_support::weights::{RuntimeDbWeight, Weight, WeightMeter};
use frame_system::EnsureRoot;
use sp_core::H256;
use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup},
	BuildStorage,
};
use std::cell::RefCell;

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
	type XcmpMessageHandler = EmergencyParaXcm;
	type ReservedXcmpWeight = ();
	type DmpQueue = frame_support::traits::EnqueueWithOrigin<MessageQueue, RelayOrigin>;
	type ReservedDmpWeight = ();
	type CheckAssociatedRelayNumber = EmergencyParaXcm;
	type WeightInfo = ();
}

parameter_types! {
	pub const MessageQueueMaxStale: u32 = 8;
	pub const MessageQueueHeapSize: u32 = 64 * 1024;
	pub const MaxWeight: Weight = Weight::MAX;
}

impl pallet_message_queue::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type MessageProcessor = SaveIntoThreadLocal;
	type Size = u32;
	type HeapSize = MessageQueueHeapSize;
	type MaxStale = MessageQueueMaxStale;
	type ServiceWeight = MaxWeight;
	type QueueChangeHandler = ();
	type QueuePausedQuery = EmergencyParaXcm;
	type WeightInfo = ();
}

pub(crate) const PAUSED_THRESHOLD: u32 = 5;

impl Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type CheckAssociatedRelayNumber = cumulus_pallet_parachain_system::AnyRelayNumber;
	type QueuePausedQuery = ();
	type HrmpMessageHandler = ();
	type PausedThreshold = ConstU32<PAUSED_THRESHOLD>;
	type FastAuthorizeUpgradeOrigin = EnsureRoot<AccountId>;
	type PausedToNormalOrigin = EnsureRoot<AccountId>;
}

// A `MessageProcessor` that stores all messages in thread-local.
// taken from https://github.com/paritytech/polkadot-sdk/blob/3c6ebd9e9bfda58f199cba6ec3023e0d12d6b506/cumulus/pallets/parachain-system/src/mock.rs#L132
pub struct SaveIntoThreadLocal;

std::thread_local! {
	pub static HANDLED_DMP_MESSAGES: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::new());
	pub static HANDLED_XCMP_MESSAGES: RefCell<Vec<(ParaId, RelayChainBlockNumber, Vec<u8>)>> = RefCell::new(Vec::new());
}

impl ProcessMessage for SaveIntoThreadLocal {
	type Origin = AggregateMessageOrigin;

	fn process_message(
		message: &[u8],
		origin: Self::Origin,
		_meter: &mut WeightMeter,
		_id: &mut [u8; 32],
	) -> Result<bool, ProcessMessageError> {
		assert_eq!(origin, Self::Origin::Parent);

		HANDLED_DMP_MESSAGES.with(|m| {
			m.borrow_mut().push(message.to_vec());
			Weight::zero()
		});
		Ok(true)
	}
}

impl XcmpMessageHandler for SaveIntoThreadLocal {
	fn handle_xcmp_messages<'a, I: Iterator<Item = (ParaId, RelayBlockNumber, &'a [u8])>>(
		iter: I,
		_max_weight: Weight,
	) -> Weight {
		HANDLED_XCMP_MESSAGES.with(|m| {
			for (sender, sent_at, message) in iter {
				m.borrow_mut().push((sender, sent_at, message.to_vec()));
			}
			Weight::zero()
		})
	}
}

pub(crate) struct ExtBuilder;

impl ExtBuilder {
	pub(crate) fn build(self) -> sp_io::TestExternalities {
		let mut storage = frame_system::GenesisConfig::<Test>::default()
			.build_storage()
			.expect("Frame system builds valid default genesis config");

		let mut ext = sp_io::TestExternalities::new(storage);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}
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
