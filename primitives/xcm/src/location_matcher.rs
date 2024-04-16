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

use frame_support::{traits::PalletInfoAccess, Parameter};
use sp_core::H160;
use sp_runtime::traits::MaybeEquivalence;
use sp_std::{fmt::Debug, marker::PhantomData, mem::size_of};
use xcm::latest::{Junction::*, Location};

/// Information to retrieve for a specific AssetId type.
pub struct AssetIdInfo<'a> {
	pub foreign_asset_prefix: &'a [u8],
	pub size_of: usize,
}

// Define a trait to abstract over different types of AssetId.
// We retrieve an AssetIdInfo type containing all the information we need
// to handle the generic types over AssetId.
pub trait GetAssetIdInfo<T> {
	fn get_asset_id_info() -> AssetIdInfo<'static>;
}

/// A getter that contains the info for each type
/// we admit as AssetId.
pub struct AssetIdInfoGetter;

// Implement GetAssetId trait for u128.
impl GetAssetIdInfo<u128> for AssetIdInfoGetter {
	fn get_asset_id_info() -> AssetIdInfo<'static> {
		AssetIdInfo {
			foreign_asset_prefix: &[255u8; 4],
			size_of: size_of::<u128>(),
		}
	}
}

// Implement GetAssetId trait for u16.
impl GetAssetIdInfo<u16> for AssetIdInfoGetter {
	fn get_asset_id_info() -> AssetIdInfo<'static> {
		AssetIdInfo {
			foreign_asset_prefix: &[255u8; 18],
			size_of: size_of::<u16>(),
		}
	}
}

/// A converter from AccountId to a XCM Location.
pub trait AccountIdToLocationMatcher<AccountId> {
	fn convert(account: AccountId) -> Option<Location>;
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl<AccountId: Debug + Parameter> AccountIdToLocationMatcher<AccountId> for Tuple {
	fn convert(account: AccountId) -> Option<Location> {
		for_tuples!( #(
			match Tuple::convert(account.clone()) { o @ Some(_) => return o, _ => () }
		)* );
		log::trace!(target: "xcm_primitives::convert", "did not match any location to the account: {:?}", &account);
		None
	}
}

/// A matcher for any address that we would like to compare against a received account.
/// Tipically used to manage self-reserve currency through the pallet-balances address.
pub struct SingleAddressMatcher<AccountId, const ADDRESS: u64, PalletInstance>(
	PhantomData<(AccountId, PalletInstance)>,
);

impl<AccountId, const ADDRESS: u64, PalletInstance> AccountIdToLocationMatcher<AccountId>
	for SingleAddressMatcher<AccountId, ADDRESS, PalletInstance>
where
	AccountId: Parameter + From<H160>,
	PalletInstance: PalletInfoAccess,
{
	fn convert(account: AccountId) -> Option<Location> {
		if account == H160::from_low_u64_be(ADDRESS).into() {
			return Some(Location::new(
				0,
				[PalletInstance(
					<PalletInstance as PalletInfoAccess>::index() as u8,
				)],
			));
		}
		None
	}
}

/// Matcher to compare a received account against some possible foreign asset address.
pub struct MatchThroughEquivalence<AccountId, AssetId, AssetIdInfoGetter, AssetIdToLocationManager>(
	PhantomData<(
		AccountId,
		AssetId,
		AssetIdInfoGetter,
		AssetIdToLocationManager,
	)>,
);

impl<AccountId, AssetId, AssetIdInfoGetter, AssetIdToLocationManager>
	MatchThroughEquivalence<AccountId, AssetId, AssetIdInfoGetter, AssetIdToLocationManager>
where
	AccountId: Parameter + Into<H160>,
	AssetId: From<u8> + TryFrom<u16> + TryFrom<u128>,
{
	// Helper function that retrieves the asset_id of a foreign asset account.
	pub fn account_to_asset_id(
		account: AccountId,
		asset_id_info: AssetIdInfo,
	) -> Option<(Vec<u8>, AssetId)> {
		let h160_account: H160 = account.into();
		let (prefix_part, id_part) = h160_account
			.as_fixed_bytes()
			.split_at(asset_id_info.size_of + 2);

		if prefix_part == asset_id_info.foreign_asset_prefix {
			let asset_id: AssetId = match asset_id_info.size_of {
				2 => {
					let mut data = [0u8; 2];
					data.copy_from_slice(id_part);
					u16::from_be_bytes(data).try_into().unwrap_or(0u8.into())
				}
				16 => {
					let mut data = [0u8; 16];
					data.copy_from_slice(id_part);
					u128::from_be_bytes(data).try_into().unwrap_or(0u8.into())
				}
				_ => return None,
			};
			return Some((prefix_part.to_vec(), asset_id));
		}
		None
	}
}

impl<AccountId, AssetId, AssetIdInfoGetter, AssetIdToLocationManager>
	AccountIdToLocationMatcher<AccountId>
	for MatchThroughEquivalence<AccountId, AssetId, AssetIdInfoGetter, AssetIdToLocationManager>
where
	AccountId: Parameter + Into<H160>,
	AssetId: From<u8> + TryFrom<u16> + TryFrom<u128>,
	AssetIdInfoGetter: GetAssetIdInfo<AssetId>,
	AssetIdToLocationManager: MaybeEquivalence<Location, AssetId>,
{
	fn convert(account: AccountId) -> Option<Location> {
		let asset_id_info = AssetIdInfoGetter::get_asset_id_info();
		if let Some((_prefix, asset_id)) = Self::account_to_asset_id(account, asset_id_info) {
			return AssetIdToLocationManager::convert_back(&asset_id);
		}
		None
	}
}

// Matcher for any pallet that handles ERC20s internally.
pub struct Erc20PalletMatcher<AccountId, const PALLET_INDEX: u8>(PhantomData<AccountId>);

impl<AccountId, const PALLET_INDEX: u8> AccountIdToLocationMatcher<AccountId>
	for Erc20PalletMatcher<AccountId, PALLET_INDEX>
where
	AccountId: Parameter + Into<H160>,
{
	fn convert(account: AccountId) -> Option<Location> {
		let h160_account = account.into();
		Some(Location::new(
			0,
			[
				PalletInstance(PALLET_INDEX),
				AccountKey20 {
					key: h160_account.0,
					network: None,
				},
			],
		))
	}
}
