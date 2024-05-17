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

use core::str::FromStr;

use crate::{mock::*, Location};
use precompile_utils::{prelude::*, testing::*};
use sp_core::{H160, H256};
use sp_weights::Weight;
use xcm::latest::Junction::*;

fn precompiles() -> Precompiles<Runtime, (SingleAddressMatch, ForeignAssetMatch)> {
	PrecompilesValue::get()
}

#[test]
fn test_solidity_interface_has_all_function_selectors_documented_and_implemented() {
	check_precompile_implements_solidity_interfaces(&["XcmInterface.sol"], PCall::supports_selector)
}

#[test]
fn selectors() {
	assert!(PCall::transfer_assets_location_selectors().contains(&0x59df8416));
	assert!(PCall::transfer_assets_to_para_20_selectors().contains(&0xb489262e));
	assert!(PCall::transfer_assets_to_para_32_selectors().contains(&0x4461e6f5));
	assert!(PCall::transfer_assets_to_relay_selectors().contains(&0xd7c89659));
}

#[test]
fn modifiers() {
	ExtBuilder::default().build().execute_with(|| {
		let mut tester =
			PrecompilesModifierTester::new(PrecompilesValue::get(), Alice, Precompile1);

		tester.test_default_modifier(PCall::transfer_assets_location_selectors());
	});
}

#[test]
fn selector_less_than_four_bytes() {
	ExtBuilder::default().build().execute_with(|| {
		// This selector is only three bytes long when four are required.
		precompiles()
			.prepare_test(Alice, Precompile1, vec![1u8, 2u8, 3u8])
			.execute_reverts(|output| output == b"Tried to read selector out of bounds");
	});
}

#[test]
fn no_selector_exists_but_length_is_right() {
	ExtBuilder::default().build().execute_with(|| {
		precompiles()
			.prepare_test(Alice, Precompile1, vec![1u8, 2u8, 3u8, 4u8])
			.execute_reverts(|output| output == b"Unknown selector");
	});
}

#[test]
fn test_transfer_assets_works() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.build()
		.execute_with(|| {
			let dest = Location::new(1, [Parachain(2)]);

			// Specify the beneficiary from the destination's point of view
			let beneficiary = Location::new(
				0,
				[AccountKey20 {
					network: None,
					key: [1; 20],
				}],
			);

			let destination_asset_location = Location::new(1, [Parachain(2), PalletInstance(3)]);
			let origin_asset_location = Location::new(0, [PalletInstance(1)]);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_location {
						dest,
						beneficiary,
						assets: vec![
							(origin_asset_location, 100u128.into()),
							(destination_asset_location, 150u128.into()),
						]
						.into(),
						fee_asset_item: 0u32,
						// As we are indicating u64::MAX in ref_time, an Unlimited variant
						// will be applied at the end.
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001001)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_success_when_paying_fees_with_foreign_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.build()
		.execute_with(|| {
			let dest = Location::new(1, [Parachain(2)]);

			// Specify the beneficiary from the destination's point of view
			let beneficiary = Location::new(
				0,
				[AccountKey20 {
					network: None,
					key: [1; 20],
				}],
			);

			let destination_asset_location = Location::new(1, [Parachain(2), PalletInstance(3)]);
			let origin_asset_location = Location::new(0, [PalletInstance(1)]);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_location {
						dest,
						beneficiary,
						assets: vec![
							(origin_asset_location, 100u128.into()),
							(destination_asset_location, 150u128.into()),
						]
						.into(),
						// We also act as a reserve for the foreign asset thus when can pay local
						// fees with it.
						fee_asset_item: 1u32,
						// As we are indicating u64::MAX in ref_time, an Unlimited variant
						// will be applied at the end.
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001001)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_fails_fees_unknown_reserve() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.build()
		.execute_with(|| {
			let dest = Location::new(1, [Parachain(3)]);

			// Specify the beneficiary from the destination's point of view
			let beneficiary = Location::new(
				0,
				[AccountKey20 {
					network: None,
					key: [1; 20],
				}],
			);

			let destination_asset_location = Location::new(1, [Parachain(3), PalletInstance(3)]);
			let origin_asset_location = Location::new(0, [PalletInstance(1)]);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_location {
						dest,
						beneficiary,
						assets: vec![
							(origin_asset_location, 100u128.into()),
							(destination_asset_location, 150u128.into()),
						]
						.into(),
						// No reserve will be found for this asset.
						fee_asset_item: 1u32,
						// As we are indicating u64::MAX in ref_time, an Unlimited variant
						// will be applied at the end.
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_no_logs()
				.execute_reverts(|output| output.ends_with(b"InvalidAssetUnknownReserve\") })"));
		});
}

#[test]
fn test_transfer_assets_to_para_20_native_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.build()
		.execute_with(|| {
			// We send the native currency of the origin chain.
			let pallet_balances_address = H160::from_low_u64_be(2050);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_to_para_20 {
						para_id: 2u32,
						beneficiary: Address(Bob.into()),
						assets: vec![(Address(pallet_balances_address), 500.into())].into(),
						fee_asset_item: 0u32,
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001002)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_to_para_32_native_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.build()
		.execute_with(|| {
			// We send the native currency of the origin chain.
			let pallet_balances_address = H160::from_low_u64_be(2050);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_to_para_32 {
						para_id: 2u32,
						beneficiary: H256([1u8; 32]),
						assets: vec![(Address(pallet_balances_address), 500.into())].into(),
						fee_asset_item: 0u32,
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001002)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_to_relay_native_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.build()
		.execute_with(|| {
			// We send the native currency of the origin chain.
			let pallet_balances_address = H160::from_low_u64_be(2050);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_to_relay {
						beneficiary: H256([1u8; 32]),
						assets: vec![(Address(pallet_balances_address), 500.into())].into(),
						fee_asset_item: 0u32,
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001002)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_to_para_20_foreign_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.with_xcm_assets(vec![XcmAssetDetails {
			location: Location::new(1, [Parachain(2), PalletInstance(3)]),
			admin: Alice.into(),
			asset_id: 5u16,
			is_sufficient: true,
			balance_to_mint: 10000u128,
			min_balance: 1u128,
		}])
		.build()
		.execute_with(|| {
			// Foreign asset with prefix [255; 18] and assetId of 5u16.
			let asset_address =
				H160::from_str("0xfFfFFFffFffFFFFffFFfFfffFfFFFFFfffFF0005").unwrap();

			// We send the native currency of the origin chain and pay fees with it.
			let pallet_balances_address = H160::from_low_u64_be(2050);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_to_para_20 {
						para_id: 2u32,
						beneficiary: Address(Bob.into()),
						assets: vec![
							(Address(pallet_balances_address), 500.into()),
							(Address(asset_address), 500.into()),
						]
						.into(),
						fee_asset_item: 0u32,
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001002)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_to_para_32_foreign_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.with_xcm_assets(vec![XcmAssetDetails {
			location: Location::new(1, [Parachain(2), PalletInstance(3)]),
			admin: Alice.into(),
			asset_id: 5u16,
			is_sufficient: true,
			balance_to_mint: 10000u128,
			min_balance: 1u128,
		}])
		.build()
		.execute_with(|| {
			// Foreign asset with prefix [255; 18] and assetId of 5u16.
			let asset_address =
				H160::from_str("0xfFfFFFffFffFFFFffFFfFfffFfFFFFFfffFF0005").unwrap();

			// We send the native currency of the origin chain and pay fees with it.
			let pallet_balances_address = H160::from_low_u64_be(2050);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_to_para_32 {
						para_id: 2u32,
						beneficiary: H256([0u8; 32]),
						assets: vec![
							(Address(pallet_balances_address), 500.into()),
							(Address(asset_address), 500.into()),
						]
						.into(),
						fee_asset_item: 0u32,
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001002)
				.expect_no_logs()
				.execute_returns(());
		});
}

#[test]
fn test_transfer_assets_to_relay_foreign_asset() {
	ExtBuilder::default()
		.with_balances(vec![(Alice.into(), 1000)])
		.with_xcm_assets(vec![XcmAssetDetails {
			location: Location::parent(),
			admin: Alice.into(),
			asset_id: 5u16,
			is_sufficient: true,
			balance_to_mint: 10000u128,
			min_balance: 1u128,
		}])
		.build()
		.execute_with(|| {
			// Foreign asset with prefix [255; 18] and assetId of 5u16.
			let asset_address =
				H160::from_str("0xfFfFFFffFffFFFFffFFfFfffFfFFFFFfffFF0005").unwrap();

			// We send the native currency of the origin chain and pay fees with it.
			let pallet_balances_address = H160::from_low_u64_be(2050);

			precompiles()
				.prepare_test(
					Alice,
					Precompile1,
					PCall::transfer_assets_to_relay {
						beneficiary: H256([0u8; 32]),
						assets: vec![
							(Address(pallet_balances_address), 500.into()),
							(Address(asset_address), 500.into()),
						]
						.into(),
						fee_asset_item: 0u32,
						weight: Weight::from_parts(u64::MAX, 80000),
					},
				)
				.expect_cost(100001002)
				.expect_no_logs()
				.execute_returns(());
		});
}
