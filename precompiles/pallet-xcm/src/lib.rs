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

use fp_evm::PrecompileHandle;
use frame_support::{
	dispatch::{GetDispatchInfo, PostDispatchInfo},
	traits::ConstU32,
};
use pallet_evm::AddressMapping;
use precompile_utils::prelude::*;

use sp_core::{H160, U256};
use sp_runtime::traits::{Dispatchable, MaybeEquivalence};
use sp_std::marker::PhantomData;
use sp_weights::Weight;
use xcm::{
	latest::{Asset, AssetId, Assets, Fungibility, Location},
	prelude::WeightLimit::*,
	VersionedAssets, VersionedLocation,
};
use xcm_primitives::location_converter::GetAssetId;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub const MAX_ASSETS_ARRAY_LIMIT: u32 = 2;
type GetArrayLimit = ConstU32<MAX_ASSETS_ARRAY_LIMIT>;

pub struct PalletXcmPrecompile<Runtime, AssetId, AssetIdToLocationManager, AssetIdInfoGetter>(
	PhantomData<(
		Runtime,
		AssetId,
		AssetIdToLocationManager,
		AssetIdInfoGetter,
	)>,
);

#[precompile_utils::precompile]
impl<Runtime, AssetId, AssetIdToLocationManager, AssetIdInfoGetter>
	PalletXcmPrecompile<Runtime, AssetId, AssetIdToLocationManager, AssetIdInfoGetter>
where
	Runtime: pallet_xcm::Config + pallet_evm::Config + frame_system::Config,
	<Runtime as frame_system::Config>::RuntimeCall:
		Dispatchable<PostInfo = PostDispatchInfo> + GetDispatchInfo,
	<<Runtime as frame_system::Config>::RuntimeCall as Dispatchable>::RuntimeOrigin:
		From<Option<Runtime::AccountId>>,
	<Runtime as frame_system::Config>::RuntimeCall: From<pallet_xcm::Call<Runtime>>,
	AssetIdToLocationManager: MaybeEquivalence<Location, AssetId>,
	AssetIdInfoGetter: GetAssetId<AssetId>,
	Runtime::AccountId: From<H160> + Into<H160>,
	AssetId: From<u8> + TryFrom<u16> + TryFrom<u128>,
{
	#[precompile::public(
		"transferAssets(\
		(uint8,bytes[]),\
		(uint8,bytes[]),\
		((uint8,bytes[]),uint256)[],\
		uint32,\
		(uint64,uint64))"
	)]
	fn transfer_assets(
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

	// TODO: finish
	#[precompile::public("transferAssetsWithAddress()")]
	fn transfer_assets_with_address(_handle: &mut impl PrecompileHandle) -> EvmResult {
		/*
		// Just testing stuff
		let asset_info = AssetIdInfoGetter::get_asset_id_info();
		let account: <Runtime as frame_system::Config>::AccountId =
			H160::from_low_u64_be(2050).into();

		let result = AccountIdToLocationConverter::<_, AssetId, AssetIdToLocationManager>::convert(
			account, asset_info, 3u8, 42u8,
		);

		println!("RESULT: {:#?}", result); */

		Ok(())
	}
}
