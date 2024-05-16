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

//! The XCM primitive trait implementations

#![cfg_attr(not(feature = "std"), no_std)]

use sp_runtime::DispatchResult;
use sp_std::vec::Vec;

pub mod generators;
pub mod location_matcher;

/// Pause and resume execution of XCM
pub trait PauseXcmExecution {
	fn suspend_xcm_execution() -> DispatchResult;
	fn resume_xcm_execution() -> DispatchResult;
}
impl PauseXcmExecution for () {
	fn suspend_xcm_execution() -> DispatchResult {
		Ok(())
	}
	fn resume_xcm_execution() -> DispatchResult {
		Ok(())
	}
}

/// This trait ensure we can convert AccountIds to AssetIds.
pub trait AccountIdAssetIdConversion<Account, AssetId> {
	// Get assetId and prefix from account
	fn account_to_asset_id(account: Account) -> Option<(Vec<u8>, AssetId)>;

	// Get AccountId from AssetId and prefix
	fn asset_id_to_account(prefix: &[u8], asset_id: AssetId) -> Account;
}
