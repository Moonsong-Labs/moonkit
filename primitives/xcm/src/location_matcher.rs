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

use crate::AccountIdAssetIdConversion;
use frame_support::{traits::PalletInfoAccess, Parameter};
use sp_core::H160;
use sp_runtime::traits::MaybeEquivalence;
use sp_std::{fmt::Debug, marker::PhantomData};
use xcm::latest::{Junction::*, Location};

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
pub struct ForeignAssetMatcher<
	AccountId,
	AssetId,
	AccountIdAssetIdConverter,
	AssetIdToLocationManager,
>(
	PhantomData<(
		AccountId,
		AssetId,
		AccountIdAssetIdConverter,
		AssetIdToLocationManager,
	)>,
);

impl<AccountId, AssetId, AccountIdAssetIdConverter, AssetIdToLocationManager>
	AccountIdToLocationMatcher<AccountId>
	for ForeignAssetMatcher<AccountId, AssetId, AccountIdAssetIdConverter, AssetIdToLocationManager>
where
	AccountIdAssetIdConverter: AccountIdAssetIdConversion<AccountId, AssetId>,
	AssetIdToLocationManager: MaybeEquivalence<Location, AssetId>,
{
	fn convert(account: AccountId) -> Option<Location> {
		if let Some((_prefix, asset_id)) = AccountIdAssetIdConverter::account_to_asset_id(account) {
			return AssetIdToLocationManager::convert_back(&asset_id);
		}
		None
	}
}

// Matcher for any pallet that handles ERC20s internally.
pub struct Erc20PalletMatcher<AccountId, PalletInstance>(PhantomData<(AccountId, PalletInstance)>);

impl<AccountId, PalletInstance> AccountIdToLocationMatcher<AccountId>
	for Erc20PalletMatcher<AccountId, PalletInstance>
where
	AccountId: Parameter + Into<H160>,
	PalletInstance: PalletInfoAccess,
{
	fn convert(account: AccountId) -> Option<Location> {
		let h160_account = account.into();
		Some(Location::new(
			0,
			[
				PalletInstance(<PalletInstance as PalletInfoAccess>::index() as u8),
				AccountKey20 {
					key: h160_account.0,
					network: None,
				},
			],
		))
	}
}
