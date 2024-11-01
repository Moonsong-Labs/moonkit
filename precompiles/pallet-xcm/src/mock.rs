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

//! Test utilities
use super::*;
use frame_support::{
	construct_runtime, parameter_types,
	traits::{Everything, Nothing, OriginTrait},
	weights::{RuntimeDbWeight, Weight},
};
use frame_system::EnsureRoot;
use pallet_evm::{EnsureAddressNever, EnsureAddressRoot, GasWeightMapping};
use precompile_utils::{
	mock_account,
	precompile_set::*,
	testing::{AddressInPrefixedSet, MockAccount},
};
use sp_core::{ConstU32, H160, H256, U256};
use sp_runtime::traits::{BlakeTwo256, IdentityLookup, TryConvert};
use sp_runtime::BuildStorage;
use xcm::latest::{prelude::*, Error as XcmError};
use xcm_builder::{
	AllowUnpaidExecutionFrom, Case, FixedWeightBounds, IsConcrete, SovereignSignedViaLocation,
};
use xcm_executor::{
	traits::{ConvertLocation, TransactAsset, WeightTrader},
	AssetsInHolding,
};
pub use xcm_primitives::{
	location_matcher::{ForeignAssetMatcher, SingleAddressMatcher},
	AccountIdAssetIdConversion,
};
use Junctions::Here;

pub type AccountId = MockAccount;
pub type Balance = u128;

type Block = frame_system::mocking::MockBlockU32<Runtime>;

// Configure a mock runtime to test the pallet.
construct_runtime!(
	pub enum Runtime	{
		System: frame_system,
		Balances: pallet_balances,
		Evm: pallet_evm,
		Timestamp: pallet_timestamp,
		PolkadotXcm: pallet_xcm,
		Assets: pallet_assets,
		ForeignAssetCreator: pallet_foreign_asset_creator,
	}
);

parameter_types! {
	pub ParachainId: cumulus_primitives_core::ParaId = 100.into();
	pub LocalNetworkId: Option<NetworkId> = None;
}

parameter_types! {
	pub const BlockHashCount: u32 = 250;
	pub const SS58Prefix: u8 = 42;
	pub const MockDbWeight: RuntimeDbWeight = RuntimeDbWeight {
		read: 1,
		write: 5,
	};
}

impl frame_system::Config for Runtime {
	type BaseCallFilter = Everything;
	type DbWeight = MockDbWeight;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeTask = RuntimeTask;
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
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type BlockWeights = ();
	type BlockLength = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
	type SingleBlockMigrations = ();
	type MultiBlockMigrator = ();
	type PreInherents = ();
	type PostInherents = ();
	type PostTransactions = ();
}
parameter_types! {
	pub const ExistentialDeposit: u128 = 0;
}
impl pallet_balances::Config for Runtime {
	type MaxReserves = ();
	type ReserveIdentifier = ();
	type MaxLocks = ();
	type Balance = Balance;
	type RuntimeEvent = RuntimeEvent;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
	type RuntimeHoldReason = ();
	type FreezeIdentifier = ();
	type MaxFreezes = ();
	type RuntimeFreezeReason = ();
}
parameter_types! {
	pub const AssetDeposit: u64 = 0;
	pub const ApprovalDeposit: u64 = 0;
	pub const StringLimit: u32 = 50;
	pub const MetadataDepositBase: u64 = 0;
	pub const MetadataDepositPerByte: u64 = 0;
}

pub const FOREIGN_ASSET_ADDRESS_PREFIX: &[u8] = &[255u8; 18];

// Instruct how to go from an H160 to an AssetID
// We just take the lowest 2 bytes
impl AccountIdAssetIdConversion<AccountId, AssetId> for Runtime {
	/// The way to convert an account to assetId is by ensuring that the prefix is [0xFF, 18]
	/// and by taking the lowest 2 bytes as the assetId
	fn account_to_asset_id(account: AccountId) -> Option<(Vec<u8>, AssetId)> {
		let h160_account: H160 = account.into();
		let mut data = [0u8; 2];
		let (prefix_part, id_part) = h160_account.as_fixed_bytes().split_at(18);
		if prefix_part == FOREIGN_ASSET_ADDRESS_PREFIX {
			data.copy_from_slice(id_part);
			let asset_id: AssetId = u16::from_be_bytes(data);
			Some((prefix_part.to_vec(), asset_id))
		} else {
			None
		}
	}

	// The opposite conversion
	fn asset_id_to_account(prefix: &[u8], asset_id: AssetId) -> AccountId {
		let mut data = [0u8; 20];
		data[0..18].copy_from_slice(prefix);
		data[18..20].copy_from_slice(&asset_id.to_be_bytes());
		AccountId::from(data)
	}
}

pub type AssetId = u16;

impl pallet_assets::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type AssetId = AssetId;
	type AssetIdParameter = parity_scale_codec::Compact<AssetId>;
	type Currency = Balances;
	type CreateOrigin = frame_support::traits::NeverEnsureOrigin<AccountId>;
	type ForceOrigin = EnsureRoot<AccountId>;
	type AssetDeposit = AssetDeposit;
	type AssetAccountDeposit = AssetDeposit;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = ApprovalDeposit;
	type StringLimit = StringLimit;
	type Freezer = ();
	type Extra = ();
	type CallbackHandle = ();
	type WeightInfo = ();
	type RemoveItemsLimit = ConstU32<1000>;
	pallet_assets::runtime_benchmarks_enabled! {
		type BenchmarkHelper = ();
	}
}

impl pallet_foreign_asset_creator::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type ForeignAsset = Location;
	type ForeignAssetCreatorOrigin = EnsureRoot<AccountId>;
	type ForeignAssetModifierOrigin = EnsureRoot<AccountId>;
	type ForeignAssetDestroyerOrigin = EnsureRoot<AccountId>;
	type Fungibles = Assets;
	type WeightInfo = ();
	type OnForeignAssetCreated = ();
	type OnForeignAssetDestroyed = ();
}

pub type Precompiles<R, M> =
	PrecompileSetBuilder<R, PrecompileAt<AddressU64<1>, PalletXcmPrecompile<R, M>>>;

pub type AccountIdAlias = <mock::Runtime as frame_system::Config>::AccountId;

pub type SingleAddressMatch = SingleAddressMatcher<AccountIdAlias, 2050, Balances>;

pub type ForeignAssetMatch =
	ForeignAssetMatcher<AccountIdAlias, AssetId, mock::Runtime, ForeignAssetCreator>;

pub type PCall = PalletXcmPrecompileCall<Runtime, (SingleAddressMatch, ForeignAssetMatch)>;

mock_account!(ParentAccount, |_| MockAccount::from_u64(4));

// use simple encoding for parachain accounts.
mock_account!(
	SiblingParachainAccount(u32),
	|v: SiblingParachainAccount| { AddressInPrefixedSet(0xffffffff, v.0 as u128).into() }
);

const MAX_POV_SIZE: u64 = 5 * 1024 * 1024;
/// Block storage limit in bytes. Set to 40 KB.
const BLOCK_STORAGE_LIMIT: u64 = 40 * 1024;

parameter_types! {
	pub BlockGasLimit: U256 = U256::from(u64::MAX);
	pub PrecompilesValue: Precompiles<Runtime, (SingleAddressMatch, ForeignAssetMatch)> = Precompiles::new();
	pub const WeightPerGas: Weight = Weight::from_parts(1, 0);
	pub GasLimitPovSizeRatio: u64 = {
		let block_gas_limit = BlockGasLimit::get().min(u64::MAX.into()).low_u64();
		block_gas_limit.saturating_div(MAX_POV_SIZE)
	};
	pub GasLimitStorageGrowthRatio: u64 = {
		let block_gas_limit = BlockGasLimit::get().min(u64::MAX.into()).low_u64();
		block_gas_limit.saturating_div(BLOCK_STORAGE_LIMIT)
	};
}

/// A mapping function that converts Ethereum gas to Substrate weight
/// We are mocking this 1-1 to test db read charges too
pub struct MockGasWeightMapping;
impl GasWeightMapping for MockGasWeightMapping {
	fn gas_to_weight(gas: u64, _without_base_weight: bool) -> Weight {
		Weight::from_parts(gas, 1)
	}
	fn weight_to_gas(weight: Weight) -> u64 {
		weight.ref_time().into()
	}
}

impl pallet_evm::Config for Runtime {
	type AccountProvider = pallet_evm::FrameSystemAccountProvider<Self>;
	type FeeCalculator = ();
	type GasWeightMapping = MockGasWeightMapping;
	type WeightPerGas = WeightPerGas;
	type CallOrigin = EnsureAddressRoot<AccountId>;
	type WithdrawOrigin = EnsureAddressNever<AccountId>;
	type AddressMapping = AccountId;
	type Currency = Balances;
	type RuntimeEvent = RuntimeEvent;
	type Runner = pallet_evm::runner::stack::Runner<Self>;
	type PrecompilesValue = PrecompilesValue;
	type PrecompilesType = Precompiles<Self, (SingleAddressMatch, ForeignAssetMatch)>;
	type ChainId = ();
	type OnChargeTransaction = ();
	type BlockGasLimit = BlockGasLimit;
	type BlockHashMapping = pallet_evm::SubstrateBlockHashMapping<Self>;
	type FindAuthor = ();
	type OnCreate = ();
	type GasLimitPovSizeRatio = GasLimitPovSizeRatio;
	type GasLimitStorageGrowthRatio = GasLimitStorageGrowthRatio;
	type SuicideQuickClearLimit = ConstU32<0>;
	type Timestamp = Timestamp;
	type WeightInfo = pallet_evm::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	pub const MinimumPeriod: u64 = 5;
}
impl pallet_timestamp::Config for Runtime {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

use frame_system::RawOrigin as SystemRawOrigin;
use xcm::latest::Junction;
pub struct MockAccountToAccountKey20<Origin, AccountId>(PhantomData<(Origin, AccountId)>);

impl<Origin: OriginTrait + Clone, AccountId: Into<H160>> TryConvert<Origin, Location>
	for MockAccountToAccountKey20<Origin, AccountId>
where
	Origin::PalletsOrigin: From<SystemRawOrigin<AccountId>>
		+ TryInto<SystemRawOrigin<AccountId>, Error = Origin::PalletsOrigin>,
{
	fn try_convert(o: Origin) -> Result<Location, Origin> {
		o.try_with_caller(|caller| match caller.try_into() {
			Ok(SystemRawOrigin::Signed(who)) => {
				let account_h160: H160 = who.into();
				Ok(Junction::AccountKey20 {
					network: None,
					key: account_h160.into(),
				}
				.into())
			}
			Ok(other) => Err(other.into()),
			Err(other) => Err(other),
		})
	}
}

pub struct MockParentMultilocationToAccountConverter;
impl ConvertLocation<AccountId> for MockParentMultilocationToAccountConverter {
	fn convert_location(location: &Location) -> Option<AccountId> {
		match location {
			Location {
				parents: 1,
				interior: Here,
			} => Some(ParentAccount.into()),
			_ => None,
		}
	}
}

pub struct MockParachainMultilocationToAccountConverter;
impl ConvertLocation<AccountId> for MockParachainMultilocationToAccountConverter {
	fn convert_location(location: &Location) -> Option<AccountId> {
		match location.unpack() {
			(1, [Parachain(id)]) => Some(SiblingParachainAccount(*id).into()),
			_ => None,
		}
	}
}

pub type LocationToAccountId = (
	MockParachainMultilocationToAccountConverter,
	MockParentMultilocationToAccountConverter,
	xcm_builder::AccountKey20Aliases<LocalNetworkId, AccountId>,
);

pub type Barrier = AllowUnpaidExecutionFrom<Everything>;

pub type LocalOriginToLocation = MockAccountToAccountKey20<RuntimeOrigin, AccountId>;

parameter_types! {
	pub MatcherLocation: Location = Location::here();

	pub UniversalLocation: InteriorLocation =
		[GlobalConsensus(RelayNetwork::get()), Parachain(ParachainId::get().into())].into();

	pub const BaseXcmWeight: Weight = Weight::from_parts(1000u64, 1000u64);
	pub const RelayNetwork: NetworkId = NetworkId::Polkadot;

	pub MaxInstructions: u32 = 100;
	pub const MaxAssetsIntoHolding: u32 = 64;
}

impl pallet_xcm::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type SendXcmOrigin = xcm_builder::EnsureXcmOrigin<RuntimeOrigin, LocalOriginToLocation>;
	type XcmRouter = TestSendXcm;
	type ExecuteXcmOrigin = xcm_builder::EnsureXcmOrigin<RuntimeOrigin, LocalOriginToLocation>;
	type XcmExecuteFilter = frame_support::traits::Everything;
	type XcmExecutor = xcm_executor::XcmExecutor<XcmConfig>;
	type XcmTeleportFilter = Everything;
	type XcmReserveTransferFilter = Everything;
	type Weigher = FixedWeightBounds<BaseXcmWeight, RuntimeCall, MaxInstructions>;
	type UniversalLocation = UniversalLocation;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	const VERSION_DISCOVERY_QUEUE_SIZE: u32 = 100;
	type AdvertisedXcmVersion = ();
	type Currency = Balances;
	type CurrencyMatcher = IsConcrete<MatcherLocation>;
	type TrustedLockers = ();
	type SovereignAccountOf = ();
	type MaxLockers = ConstU32<8>;
	type WeightInfo = pallet_xcm::TestWeightInfo;
	type MaxRemoteLockConsumers = ConstU32<0>;
	type RemoteLockConsumerIdentifier = ();
	type AdminOrigin = frame_system::EnsureRoot<AccountId>;
}

use sp_std::cell::RefCell;
use xcm::latest::{opaque, Assets as XcmAssets};
// Simulates sending a XCM message
thread_local! {
	pub static SENT_XCM: RefCell<Vec<(Location, opaque::Xcm)>> = RefCell::new(Vec::new());
}

pub struct TestSendXcm;
impl SendXcm for TestSendXcm {
	type Ticket = ();

	fn validate(
		destination: &mut Option<Location>,
		message: &mut Option<opaque::Xcm>,
	) -> SendResult<Self::Ticket> {
		SENT_XCM.with(|q| {
			q.borrow_mut()
				.push((destination.clone().unwrap(), message.clone().unwrap()))
		});
		Ok(((), XcmAssets::new()))
	}

	fn deliver(_: Self::Ticket) -> Result<XcmHash, SendError> {
		Ok(XcmHash::default())
	}
}

pub struct DoNothingRouter;
impl SendXcm for DoNothingRouter {
	type Ticket = ();

	fn validate(
		_destination: &mut Option<Location>,
		_message: &mut Option<Xcm<()>>,
	) -> SendResult<Self::Ticket> {
		Ok(((), XcmAssets::new()))
	}

	fn deliver(_: Self::Ticket) -> Result<XcmHash, SendError> {
		Ok(XcmHash::default())
	}
}

pub struct DummyAssetTransactor;
impl TransactAsset for DummyAssetTransactor {
	fn deposit_asset(_what: &Asset, _who: &Location, _context: Option<&XcmContext>) -> XcmResult {
		Ok(())
	}

	fn withdraw_asset(
		_what: &Asset,
		_who: &Location,
		_maybe_context: Option<&XcmContext>,
	) -> Result<AssetsInHolding, XcmError> {
		Ok(AssetsInHolding::default())
	}
}

pub struct DummyWeightTrader;
impl WeightTrader for DummyWeightTrader {
	fn new() -> Self {
		DummyWeightTrader
	}

	fn buy_weight(
		&mut self,
		_weight: Weight,
		_payment: AssetsInHolding,
		_context: &XcmContext,
	) -> Result<AssetsInHolding, XcmError> {
		Ok(AssetsInHolding::default())
	}
}

pub type XcmOriginToTransactDispatchOrigin = (
	// Sovereign account converter; this attempts to derive an `AccountId` from the origin location
	// using `LocationToAccountId` and then turn that into the usual `Signed` origin. Useful for
	// foreign chains who want to have a local sovereign account on this chain which they control.
	SovereignSignedViaLocation<LocationToAccountId, RuntimeOrigin>,
);

parameter_types! {
	pub ForeignReserveLocation: Location = Location::new(
		1,
		[Parachain(2)]
	);

	pub ForeignAsset: Asset = Asset {
		fun: Fungible(10000000),
		id: AssetId(Location::new(
			1,
			[Parachain(2), PalletInstance(3)],
		)),
	};

	pub RelayLocation: Location = Location::parent();

	pub LocalAsset: (AssetFilter, Location) = (All.into(), Location::here());
	pub TrustedForeignAsset: (AssetFilter, Location) = (ForeignAsset::get().into(), ForeignReserveLocation::get());
	pub RelayForeignAsset: (AssetFilter, Location) = (All.into(), RelayLocation::get());
}

pub struct XcmConfig;
impl xcm_executor::Config for XcmConfig {
	type RuntimeCall = RuntimeCall;
	type XcmSender = TestSendXcm;
	type AssetTransactor = DummyAssetTransactor;
	type OriginConverter = XcmOriginToTransactDispatchOrigin;
	type IsReserve = (
		Case<LocalAsset>,
		Case<TrustedForeignAsset>,
		Case<RelayForeignAsset>,
	);
	type IsTeleporter = ();
	type UniversalLocation = UniversalLocation;
	type Barrier = Barrier;
	type Weigher = FixedWeightBounds<BaseXcmWeight, RuntimeCall, MaxInstructions>;
	type Trader = DummyWeightTrader;
	type ResponseHandler = ();
	type SubscriptionService = ();
	type AssetTrap = ();
	type AssetClaims = ();
	type CallDispatcher = RuntimeCall;
	type AssetLocker = ();
	type AssetExchanger = ();
	type PalletInstancesInfo = ();
	type MaxAssetsIntoHolding = MaxAssetsIntoHolding;
	type FeeManager = ();
	type MessageExporter = ();
	type UniversalAliases = Nothing;
	type SafeCallFilter = Everything;
	type Aliasers = Nothing;

	type TransactionalProcessor = ();
	type HrmpNewChannelOpenRequestHandler = ();
	type HrmpChannelAcceptedHandler = ();
	type HrmpChannelClosingHandler = ();
	type XcmRecorder = PolkadotXcm;
}

pub fn root_origin() -> <Runtime as frame_system::Config>::RuntimeOrigin {
	<Runtime as frame_system::Config>::RuntimeOrigin::root()
}

pub fn origin_of(account_id: AccountId) -> <Runtime as frame_system::Config>::RuntimeOrigin {
	<Runtime as frame_system::Config>::RuntimeOrigin::signed(account_id)
}

#[derive(Clone)]
pub struct XcmAssetDetails {
	pub location: Location,
	pub admin: <Runtime as frame_system::Config>::AccountId,
	pub asset_id: <Runtime as pallet_assets::Config>::AssetId,
	pub is_sufficient: bool,
	pub min_balance: u128,
	pub balance_to_mint: u128,
}

pub(crate) struct ExtBuilder {
	// endowed accounts with balances
	balances: Vec<(AccountId, Balance)>,
	xcm_assets: Vec<XcmAssetDetails>,
}

impl Default for ExtBuilder {
	fn default() -> ExtBuilder {
		ExtBuilder {
			balances: vec![],
			xcm_assets: vec![],
		}
	}
}

impl ExtBuilder {
	pub(crate) fn with_balances(mut self, balances: Vec<(AccountId, Balance)>) -> Self {
		self.balances = balances;
		self
	}
	pub(crate) fn with_xcm_assets(mut self, xcm_assets: Vec<XcmAssetDetails>) -> Self {
		self.xcm_assets = xcm_assets;
		self
	}

	pub(crate) fn build(self) -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Runtime>::default()
			.build_storage()
			.expect("Frame system builds valid default genesis config");

		pallet_balances::GenesisConfig::<Runtime> {
			balances: self.balances,
		}
		.assimilate_storage(&mut t)
		.expect("Pallet balances storage can be assimilated");

		let xcm_assets = self.xcm_assets.clone();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| {
			for xcm_asset in xcm_assets {
				ForeignAssetCreator::create_foreign_asset(
					root_origin(),
					xcm_asset.location,
					xcm_asset.asset_id,
					xcm_asset.admin,
					xcm_asset.is_sufficient,
					xcm_asset.min_balance,
				)
				.unwrap();

				Assets::mint(
					origin_of(xcm_asset.admin.into()),
					xcm_asset.asset_id.into(),
					xcm_asset.admin,
					xcm_asset.balance_to_mint,
				)
				.unwrap();
			}
			System::set_block_number(1);
		});
		ext
	}
}
