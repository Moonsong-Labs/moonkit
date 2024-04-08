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

use frame_support::Parameter;
use sp_core::H160;
use sp_runtime::traits::MaybeEquivalence;
use sp_std::marker::PhantomData;
use xcm::latest::{Junction::*, Location};

pub struct AccountIdToLocationConverter<AccountId, AssetId, AssetIdToLocationManager>(
	pub PhantomData<(AccountId, AssetId, AssetIdToLocationManager)>,
);

impl<AccountId, AssetId, AssetIdToLocationManager>
	AccountIdToLocationConverter<AccountId, AssetId, AssetIdToLocationManager>
where
	AccountId: Parameter + From<H160> + Into<H160>,
	AssetId: From<u8> + TryFrom<u128> + TryFrom<u16>,
	AssetIdToLocationManager: MaybeEquivalence<Location, AssetId>,
{
	// Function that converts an AccountId into a Location.
	pub fn convert(
		account: AccountId,
		asset_id_info: AssetIdInfo,
		balances_index: u8,
		erc20_xcm_bridge_index: u8,
	) -> Option<Location> {
		match account {
			// First we check if the account matches the pallet-balances address.
			// In that case we return the self-reserve location, which is the location
			// of pallet-balances.
			// TODO: how do we make it more generic?
			a if a == H160::from_low_u64_be(2050).into() => {
				Some(Location::new(0, [PalletInstance(balances_index)]))
			}
			// Then check if the asset is a foreign asset, we use the prefix for this.
			_ => match Self::account_to_asset_id(account.clone(), asset_id_info) {
				// If we encounter a prefix, it means the account is a foreign asset.
				// In this case we call AssetIdToLocationManager::convert_back() to retrieve the
				// desired location in a generic way.
				Some((_prefix, asset_id)) => AssetIdToLocationManager::convert_back(&asset_id),

				// If the address is not a foreign asset, then it means it is a real ERC20.
				// Here we append an AccountKey20 to the location of the Erc20XcmBridgePallet.
				//
				// TODO: this will retrieve an unreal location in case the pallet is not installed
				// in the runtime, we should handle this case as well.
				// TODO: how do we make it more generic?
				None => {
					let h160_account: H160 = account.into();
					Some(Location::new(
						0,
						[
							PalletInstance(erc20_xcm_bridge_index),
							AccountKey20 {
								network: None,
								key: h160_account.into(),
							},
						],
					))
				}
			},
		}
	}

	// Helper function that retrieves the asset_id of a foreign asset account.
	pub fn account_to_asset_id(
		account: AccountId,
		asset_id_info: AssetIdInfo,
	) -> Option<(Vec<u8>, AssetId)> {
		let h160_account: H160 = account.into();
		let (prefix_part, id_part) = h160_account
			.as_fixed_bytes()
			.split_at(asset_id_info.split_at);

		if prefix_part == asset_id_info.foreign_asset_prefix {
			let asset_id: AssetId = match asset_id_info.bit_type {
				16 => {
					let mut data = [0u8; 2];
					data.copy_from_slice(id_part);
					u16::from_be_bytes(data).try_into().unwrap_or(0u8.into())
				}
				128 => {
					let mut data = [0u8; 16];
					data.copy_from_slice(id_part);
					u128::from_be_bytes(data).try_into().unwrap_or(0u8.into())
				}
				_ => return None,
			};
			return Some((prefix_part.to_vec(), asset_id));
		} else {
			return None;
		}
	}
}

/// Information to retrieve for a specific AssetId type.
pub struct AssetIdInfo<'a> {
	pub foreign_asset_prefix: &'a [u8],
	pub split_at: usize,
	pub bit_type: usize,
}

// Define a trait to abstract over different types of AssetId.
// We retrieve an AssetIdInfo type containing all the information we need
// to handle the generic types over AssetId.
pub trait GetAssetId<T> {
	fn get_asset_id_info() -> AssetIdInfo<'static>;
}

/// A getter that contains the info for each type
/// we admit as AssetId.
pub struct AssetIdInfoGetter;

// Implement GetAssetId trait for u128.
impl GetAssetId<u128> for AssetIdInfoGetter {
	fn get_asset_id_info() -> AssetIdInfo<'static> {
		AssetIdInfo {
			foreign_asset_prefix: &[255u8; 4],
			split_at: 4,
			bit_type: 128,
		}
	}
}

// Implement GetAssetId trait for u16.
impl GetAssetId<u16> for AssetIdInfoGetter {
	fn get_asset_id_info() -> AssetIdInfo<'static> {
		AssetIdInfo {
			foreign_asset_prefix: &[255u8; 18],
			split_at: 18,
			bit_type: 16,
		}
	}
}
