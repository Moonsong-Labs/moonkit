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

#![cfg_attr(not(feature = "std"), no_std)]

use fp_evm::{PrecompileFailure, PrecompileHandle};
use frame_support::{
	dispatch::{GetDispatchInfo, PostDispatchInfo},
	traits::ConstU32,
};
use pallet_evm::AddressMapping;
use precompile_utils::prelude::*;

use sp_core::{MaxEncodedLen, H256, U256};
use sp_runtime::traits::Dispatchable;
use sp_std::marker::PhantomData;
use sp_weights::Weight;
use xcm::{
	latest::{Asset, AssetId, Assets, Fungibility, Location},
	prelude::WeightLimit::*,
	VersionedAssets, VersionedLocation,
};
use xcm_primitives::{
	generators::{
		XcmLocalBeneficiary20Generator, XcmLocalBeneficiary32Generator,
		XcmSiblingDestinationGenerator,
	},
	location_matcher::AccountIdToLocationMatcher,
};

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub const MAX_ASSETS_ARRAY_LIMIT: u32 = 2;
type GetArrayLimit = ConstU32<MAX_ASSETS_ARRAY_LIMIT>;

pub struct PalletXcmPrecompile<Runtime, LocationMatcher>(PhantomData<(Runtime, LocationMatcher)>);

#[precompile_utils::precompile]
impl<Runtime, LocationMatcher> PalletXcmPrecompile<Runtime, LocationMatcher>
where
	Runtime: pallet_xcm::Config + pallet_evm::Config + frame_system::Config,
	<Runtime as frame_system::Config>::RuntimeCall:
		Dispatchable<PostInfo = PostDispatchInfo> + GetDispatchInfo,
	<<Runtime as frame_system::Config>::RuntimeCall as Dispatchable>::RuntimeOrigin:
		From<Option<Runtime::AccountId>>,
	<Runtime as frame_system::Config>::RuntimeCall: From<pallet_xcm::Call<Runtime>>,
	LocationMatcher: AccountIdToLocationMatcher<<Runtime as frame_system::Config>::AccountId>,
{
	#[precompile::public(
		"transferAssetsLocation(\
		(uint8,bytes[]),\
		(uint8,bytes[]),\
		((uint8,bytes[]),uint256)[],\
		uint32,\
		(uint64,uint64))"
	)]
	fn transfer_assets_location(
		handle: &mut impl PrecompileHandle,
		dest: Location,
		beneficiary: Location,
		assets: BoundedVec<(Location, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
		weight: Weight,
	) -> EvmResult {
		// No DB access before try_dispatch but some logical stuff.
		// To prevent spam, we charge an arbitrary amount of gas.
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();

		let assets_to_send: Assets = assets
			.into_iter()
			.map(|asset| Asset {
				id: AssetId(asset.0),
				fun: Fungibility::Fungible(asset.1.converted()),
			})
			.collect::<Vec<Asset>>()
			.into();

		let weight_limit = match weight.ref_time() {
			u64::MAX => Unlimited,
			_ => Limited(weight),
		};

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::V4(dest)),
			beneficiary: Box::new(VersionedLocation::V4(beneficiary)),
			assets: Box::new(VersionedAssets::V4(assets_to_send)),
			fee_asset_item,
			weight_limit,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call)?;
		Ok(())
	}

	#[precompile::public(
		"transferAssetsToPara20(\
			uint32,\
			address,\
			(address,uint256)[],\
			uint32,\
			(uint64,uint64))"
	)]
	fn transfer_assets_to_para_20(
		handle: &mut impl PrecompileHandle,
		para_id: u32,
		beneficiary: Address,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
		weight: Weight,
	) -> EvmResult {
		// Account for a possible storage read inside LocationMatcher::convert().
		//
		// Storage items: AssetIdToForeignAsset (ForeignAssetCreator pallet) or AssetIdType (AssetManager pallet).
		//
		// Blake2_128(16) + AssetId(16) + Location
		handle.record_db_read::<Runtime>(32 + Location::max_encoded_len())?;
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();

		let assets_to_send: Vec<Asset> = Self::check_and_prepare_assets(assets)?;

		let weight_limit = match weight.ref_time() {
			u64::MAX => Unlimited,
			_ => Limited(weight),
		};

		let dest = XcmSiblingDestinationGenerator::generate(para_id);
		let beneficiary = XcmLocalBeneficiary20Generator::generate(beneficiary.0 .0);

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::V4(dest)),
			beneficiary: Box::new(VersionedLocation::V4(beneficiary)),
			assets: Box::new(VersionedAssets::V4(assets_to_send.into())),
			fee_asset_item,
			weight_limit,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsToPara32(\
			uint32,\
			bytes32,\
			(address,uint256)[],\
			uint32,\
			(uint64,uint64))"
	)]
	fn transfer_assets_to_para_32(
		handle: &mut impl PrecompileHandle,
		para_id: u32,
		beneficiary: H256,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
		weight: Weight,
	) -> EvmResult {
		// Account for a possible storage read inside LocationMatcher::convert().
		//
		// Storage items: AssetIdToForeignAsset (ForeignAssetCreator pallet) or AssetIdType (AssetManager pallet).
		//
		// Blake2_128(16) + AssetId(16) + Location
		handle.record_db_read::<Runtime>(32 + Location::max_encoded_len())?;
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();

		let assets_to_send: Vec<Asset> = Self::check_and_prepare_assets(assets)?;

		let weight_limit = match weight.ref_time() {
			u64::MAX => Unlimited,
			_ => Limited(weight),
		};

		let dest = XcmSiblingDestinationGenerator::generate(para_id);
		let beneficiary = XcmLocalBeneficiary32Generator::generate(beneficiary.0);

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::V4(dest)),
			beneficiary: Box::new(VersionedLocation::V4(beneficiary)),
			assets: Box::new(VersionedAssets::V4(assets_to_send.into())),
			fee_asset_item,
			weight_limit,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsToRelay(\
			bytes32,\
			(address,uint256)[],\
			uint32,\
			(uint64,uint64))"
	)]
	fn transfer_assets_to_relay(
		handle: &mut impl PrecompileHandle,
		beneficiary: H256,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
		weight: Weight,
	) -> EvmResult {
		// Account for a possible storage read inside LocationMatcher::convert().
		//
		// Storage items: AssetIdToForeignAsset (ForeignAssetCreator pallet) or AssetIdType (AssetManager pallet).
		//
		// Blake2_128(16) + AssetId(16) + Location
		handle.record_db_read::<Runtime>(32 + Location::max_encoded_len())?;
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();

		let assets_to_send: Vec<Asset> = Self::check_and_prepare_assets(assets)?;

		let weight_limit = match weight.ref_time() {
			u64::MAX => Unlimited,
			_ => Limited(weight),
		};

		let dest = Location::parent();
		let beneficiary = XcmLocalBeneficiary32Generator::generate(beneficiary.0);

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::V4(dest)),
			beneficiary: Box::new(VersionedLocation::V4(beneficiary)),
			assets: Box::new(VersionedAssets::V4(assets_to_send.into())),
			fee_asset_item,
			weight_limit,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call)?;

		Ok(())
	}

	// Helper function to convert and prepare each asset into a proper Location.
	fn check_and_prepare_assets(
		assets: Vec<(Address, Convert<U256, u128>)>,
	) -> Result<Vec<Asset>, PrecompileFailure> {
		let mut assets_to_send: Vec<Asset> = vec![];
		for asset in assets {
			let asset_account = Runtime::AddressMapping::into_account_id(asset.0 .0);
			let asset_location = LocationMatcher::convert(asset_account);
			if asset_location == None {
				return Err(revert("Asset not found"));
			}
			assets_to_send.push(Asset {
				id: AssetId(asset_location.unwrap_or_default()),
				fun: Fungibility::Fungible(asset.1.converted()),
			})
		}
		Ok(assets_to_send)
	}
}
