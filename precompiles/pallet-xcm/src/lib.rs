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
use parity_scale_codec::{Decode, DecodeLimit, Encode, MaxEncodedLen};
use precompile_utils::prelude::*;

use scale_info::TypeInfo;
use sp_core::{H256, U256};
use sp_runtime::traits::Dispatchable;
use sp_std::{boxed::Box, marker::PhantomData, vec, vec::Vec};
use xcm::{
	latest::{Asset, AssetId, Assets, Fungibility, Location, WeightLimit},
	VersionedAssetId, VersionedAssets, VersionedLocation, VersionedXcm, MAX_XCM_DECODE_DEPTH,
};
use xcm_executor::traits::TransferType;
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

pub const XCM_SIZE_LIMIT: u32 = 2u32.pow(16);
type GetXcmSizeLimit = ConstU32<XCM_SIZE_LIMIT>;

#[derive(
	Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, Debug, MaxEncodedLen, TypeInfo,
)]
pub enum TransferTypeHelper {
	Teleport = 0,
	LocalReserve = 1,
	DestinationReserve = 2,
}

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
	<Runtime as pallet_evm::Config>::AddressMapping: AddressMapping<Runtime::AccountId>,
{
	#[precompile::public(
		"transferAssetsLocation(\
		(uint8,bytes[]),\
		(uint8,bytes[]),\
		((uint8,bytes[]),uint256)[],\
		uint32)"
	)]
	fn transfer_assets_location(
		handle: &mut impl PrecompileHandle,
		dest: Location,
		beneficiary: Location,
		assets: BoundedVec<(Location, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
	) -> EvmResult {
		// No DB access before try_dispatch but some logical stuff.
		// To prevent spam, we charge an arbitrary amount of gas.
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();

		let (assets_to_send, _) = Self::get_assets_to_send_and_remote_fees(assets, None)?;

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::from(dest)),
			beneficiary: Box::new(VersionedLocation::from(beneficiary)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			fee_asset_item,
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;
		Ok(())
	}

	#[precompile::public(
		"transferAssetsToPara20(\
			uint32,\
			address,\
			(address,uint256)[],\
			uint32)"
	)]
	fn transfer_assets_to_para_20(
		handle: &mut impl PrecompileHandle,
		para_id: u32,
		beneficiary: Address,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
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

		let assets_converted = Self::convert_assets(assets)?;
		let (assets_to_send, _) = Self::get_assets_to_send_and_remote_fees(assets_converted, None)?;

		let dest = XcmSiblingDestinationGenerator::generate(para_id);
		let beneficiary = XcmLocalBeneficiary20Generator::generate(beneficiary.0 .0);

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::from(dest)),
			beneficiary: Box::new(VersionedLocation::from(beneficiary)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			fee_asset_item,
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsToPara32(\
			uint32,\
			bytes32,\
			(address,uint256)[],\
			uint32)"
	)]
	fn transfer_assets_to_para_32(
		handle: &mut impl PrecompileHandle,
		para_id: u32,
		beneficiary: H256,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
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

		let assets_converted = Self::convert_assets(assets)?;
		let (assets_to_send, _) = Self::get_assets_to_send_and_remote_fees(assets_converted, None)?;

		let dest = XcmSiblingDestinationGenerator::generate(para_id);
		let beneficiary = XcmLocalBeneficiary32Generator::generate(beneficiary.0);

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::from(dest)),
			beneficiary: Box::new(VersionedLocation::from(beneficiary)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			fee_asset_item,
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsToRelay(\
			bytes32,\
			(address,uint256)[],\
			uint32)"
	)]
	fn transfer_assets_to_relay(
		handle: &mut impl PrecompileHandle,
		beneficiary: H256,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		fee_asset_item: u32,
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

		let assets_converted = Self::convert_assets(assets)?;
		let (assets_to_send, _) = Self::get_assets_to_send_and_remote_fees(assets_converted, None)?;

		let dest = Location::parent();
		let beneficiary = XcmLocalBeneficiary32Generator::generate(beneficiary.0);

		let call = pallet_xcm::Call::<Runtime>::transfer_assets {
			dest: Box::new(VersionedLocation::from(dest)),
			beneficiary: Box::new(VersionedLocation::from(beneficiary)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			fee_asset_item,
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsUsingTypeAndThenLocation(\
		(uint8,bytes[]),\
		((uint8,bytes[]),uint256)[],\
		uint8,\
		uint8,\
		uint8,\
		bytes)"
	)]
	fn transfer_assets_using_type_and_then_location_no_remote_reserve(
		handle: &mut impl PrecompileHandle,
		dest: Location,
		assets: BoundedVec<(Location, Convert<U256, u128>), GetArrayLimit>,
		assets_transfer_type: u8,
		remote_fees_id_index: u8,
		fees_transfer_type: u8,
		custom_xcm_on_dest: BoundedBytes<GetXcmSizeLimit>,
	) -> EvmResult {
		// No DB access before try_dispatch but some logical stuff.
		// To prevent spam, we charge an arbitrary amount of gas.
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();
		let custom_xcm_on_dest: Vec<u8> = custom_xcm_on_dest.into();

		let (assets_to_send, remote_fees_id) = Self::get_assets_to_send_and_remote_fees(
			assets.clone(),
			Some(remote_fees_id_index as usize),
		)?;
		let remote_fees_id =
			remote_fees_id.ok_or_else(|| RevertReason::custom("remote_fees_id not found"))?;

		let assets_transfer_type = Self::parse_transfer_type(assets_transfer_type)?;
		let fees_transfer_type = Self::parse_transfer_type(fees_transfer_type)?;

		let custom_xcm_on_dest = VersionedXcm::<()>::decode_all_with_depth_limit(
			MAX_XCM_DECODE_DEPTH,
			&mut custom_xcm_on_dest.as_slice(),
		)
		.map_err(|_| RevertReason::custom("Failed decoding custom XCM message"))?;

		let call = pallet_xcm::Call::<Runtime>::transfer_assets_using_type_and_then {
			dest: Box::new(VersionedLocation::from(dest)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			assets_transfer_type: Box::new(assets_transfer_type),
			remote_fees_id: Box::new(VersionedAssetId::from(remote_fees_id)),
			fees_transfer_type: Box::new(fees_transfer_type),
			custom_xcm_on_dest: Box::new(custom_xcm_on_dest),
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsUsingTypeAndThenLocation(\
		(uint8,bytes[]),\
		((uint8,bytes[]),uint256)[],\
		uint8,\
		bytes,\
		(uint8,bytes[]))"
	)]
	fn transfer_assets_using_type_and_then_location_remote_reserve(
		handle: &mut impl PrecompileHandle,
		dest: Location,
		assets: BoundedVec<(Location, Convert<U256, u128>), GetArrayLimit>,
		remote_fees_id_index: u8,
		custom_xcm_on_dest: BoundedBytes<GetXcmSizeLimit>,
		remote_reserve: Location,
	) -> EvmResult {
		// No DB access before try_dispatch but some logical stuff.
		// To prevent spam, we charge an arbitrary amount of gas.
		handle.record_cost(1000)?;

		let origin = Runtime::AddressMapping::into_account_id(handle.context().caller);
		let assets: Vec<_> = assets.into();
		let custom_xcm_on_dest: Vec<u8> = custom_xcm_on_dest.into();

		let (assets_to_send, remote_fees_id) = Self::get_assets_to_send_and_remote_fees(
			assets.clone(),
			Some(remote_fees_id_index as usize),
		)?;
		let remote_fees_id =
			remote_fees_id.ok_or_else(|| RevertReason::custom("remote_fees_id not found"))?;

		let custom_xcm_on_dest = VersionedXcm::<()>::decode_all_with_depth_limit(
			MAX_XCM_DECODE_DEPTH,
			&mut custom_xcm_on_dest.as_slice(),
		)
		.map_err(|_| RevertReason::custom("Failed decoding custom XCM message"))?;

		let asset_and_fees_transfer_type =
			TransferType::RemoteReserve(VersionedLocation::from(remote_reserve));

		let call = pallet_xcm::Call::<Runtime>::transfer_assets_using_type_and_then {
			dest: Box::new(VersionedLocation::from(dest)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			assets_transfer_type: Box::new(asset_and_fees_transfer_type.clone()),
			remote_fees_id: Box::new(VersionedAssetId::from(remote_fees_id)),
			fees_transfer_type: Box::new(asset_and_fees_transfer_type),
			custom_xcm_on_dest: Box::new(custom_xcm_on_dest),
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsUsingTypeAndThenAddress(\
		(uint8,bytes[]),\
		(address,uint256)[],\
		uint8,\
		uint8,\
		uint8,\
		bytes)"
	)]
	fn transfer_assets_using_type_and_then_address_no_remote_reserve(
		handle: &mut impl PrecompileHandle,
		dest: Location,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		assets_transfer_type: u8,
		remote_fees_id_index: u8,
		fees_transfer_type: u8,
		custom_xcm_on_dest: BoundedBytes<GetXcmSizeLimit>,
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
		let custom_xcm_on_dest: Vec<u8> = custom_xcm_on_dest.into();

		let assets_converted = Self::convert_assets(assets)?;
		let (assets_to_send, remote_fees_id) = Self::get_assets_to_send_and_remote_fees(
			assets_converted,
			Some(remote_fees_id_index as usize),
		)?;
		let remote_fees_id =
			remote_fees_id.ok_or_else(|| RevertReason::custom("remote_fees_id not found"))?;

		let assets_transfer_type = Self::parse_transfer_type(assets_transfer_type)?;
		let fees_transfer_type = Self::parse_transfer_type(fees_transfer_type)?;

		let custom_xcm_on_dest = VersionedXcm::<()>::decode_all_with_depth_limit(
			MAX_XCM_DECODE_DEPTH,
			&mut custom_xcm_on_dest.as_slice(),
		)
		.map_err(|_| RevertReason::custom("Failed decoding custom XCM message"))?;

		let call = pallet_xcm::Call::<Runtime>::transfer_assets_using_type_and_then {
			dest: Box::new(VersionedLocation::from(dest)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			assets_transfer_type: Box::new(assets_transfer_type),
			remote_fees_id: Box::new(VersionedAssetId::from(remote_fees_id)),
			fees_transfer_type: Box::new(fees_transfer_type),
			custom_xcm_on_dest: Box::new(custom_xcm_on_dest),
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	#[precompile::public(
		"transferAssetsUsingTypeAndThenAddress(\
		(uint8,bytes[]),\
		(address,uint256)[],\
		uint8,\
		bytes,\
		(uint8,bytes[]))"
	)]
	fn transfer_assets_using_type_and_then_address_remote_reserve(
		handle: &mut impl PrecompileHandle,
		dest: Location,
		assets: BoundedVec<(Address, Convert<U256, u128>), GetArrayLimit>,
		remote_fees_id_index: u8,
		custom_xcm_on_dest: BoundedBytes<GetXcmSizeLimit>,
		remote_reserve: Location,
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
		let custom_xcm_on_dest: Vec<u8> = custom_xcm_on_dest.into();

		let assets_converted = Self::convert_assets(assets)?;
		let (assets_to_send, remote_fees_id) = Self::get_assets_to_send_and_remote_fees(
			assets_converted,
			Some(remote_fees_id_index as usize),
		)?;
		let remote_fees_id =
			remote_fees_id.ok_or_else(|| RevertReason::custom("remote_fees_id not found"))?;

		let custom_xcm_on_dest = VersionedXcm::<()>::decode_all_with_depth_limit(
			MAX_XCM_DECODE_DEPTH,
			&mut custom_xcm_on_dest.as_slice(),
		)
		.map_err(|_| RevertReason::custom("Failed decoding custom XCM message"))?;

		let asset_and_fees_transfer_type =
			TransferType::RemoteReserve(VersionedLocation::from(remote_reserve));

		let call = pallet_xcm::Call::<Runtime>::transfer_assets_using_type_and_then {
			dest: Box::new(VersionedLocation::from(dest)),
			assets: Box::new(VersionedAssets::from(assets_to_send)),
			assets_transfer_type: Box::new(asset_and_fees_transfer_type.clone()),
			remote_fees_id: Box::new(VersionedAssetId::from(remote_fees_id)),
			fees_transfer_type: Box::new(asset_and_fees_transfer_type),
			custom_xcm_on_dest: Box::new(custom_xcm_on_dest),
			weight_limit: WeightLimit::Unlimited,
		};

		RuntimeHelper::<Runtime>::try_dispatch(handle, Some(origin).into(), call, 0)?;

		Ok(())
	}

	// Helper function to convert and prepare each asset into a proper Location.
	fn convert_assets(
		assets: Vec<(Address, Convert<U256, u128>)>,
	) -> Result<Vec<(Location, Convert<U256, u128>)>, PrecompileFailure> {
		let mut assets_to_send: Vec<(Location, Convert<U256, u128>)> = vec![];
		for asset in assets {
			let asset_account = Runtime::AddressMapping::into_account_id(asset.0 .0);
			let asset_location = LocationMatcher::convert(asset_account);

			if let Some(asset_loc) = asset_location {
				assets_to_send.push((asset_loc, asset.1))
			} else {
				return Err(revert("Asset not found"));
			}
		}
		Ok(assets_to_send)
	}

	fn get_assets_to_send_and_remote_fees(
		assets: Vec<(Location, Convert<U256, u128>)>,
		remote_fees_id_index: Option<usize>,
	) -> Result<(Assets, Option<AssetId>), PrecompileFailure> {
		let assets_to_send: Assets = assets
			.into_iter()
			.map(|asset| Asset {
				id: AssetId(asset.0),
				fun: Fungibility::Fungible(asset.1.converted()),
			})
			.collect::<Vec<Asset>>()
			.into();

		if let Some(index) = remote_fees_id_index {
			let remote_fees_id: AssetId = {
				let asset = assets_to_send
					.get(index)
					.ok_or_else(|| RevertReason::custom("remote_fees_id not found"))?;
				AssetId(asset.id.0.clone())
			};
			return Ok((assets_to_send, Some(remote_fees_id)));
		}

		Ok((assets_to_send, None))
	}

	fn parse_transfer_type(transfer_type: u8) -> Result<TransferType, PrecompileFailure> {
		let transfer_type_helper: TransferTypeHelper = TransferTypeHelper::decode(
			&mut transfer_type.to_le_bytes().as_slice(),
		)
		.map_err(|_| RevertReason::custom("Failed decoding value for TransferTypeHelper"))?;

		match transfer_type_helper {
			TransferTypeHelper::Teleport => return Ok(TransferType::Teleport),
			TransferTypeHelper::LocalReserve => return Ok(TransferType::LocalReserve),
			TransferTypeHelper::DestinationReserve => return Ok(TransferType::DestinationReserve),
		}
	}
}
