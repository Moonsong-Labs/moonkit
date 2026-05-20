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

//! Moonkit-local glue for building [`xcm_executor::AssetsInHolding`] values
//! from raw [`Asset`]s, for use by fee-estimation queries that need to invoke
//! [`xcm_executor::traits::WeightTrader::buy_weight`] without ever withdrawing
//! the resulting holding.
//!
//! In stable2603 the holding register tracks dynamic `ImbalanceAccounting`
//! credits rather than raw `u128` amounts, so a concrete credit type is
//! needed. The upstream equivalent lives in `xcm_executor::test_helpers`,
//! which is documented as testing/internal-only and may be feature-gated or
//! removed without a deprecation cycle.

use frame_support::traits::tokens::imbalance::{
	ImbalanceAccounting, UnsafeConstructorDestructor, UnsafeManualAccounting,
};
use sp_std::boxed::Box;
use xcm::latest::prelude::*;

/// A minimal fungible credit. The query never withdraws the holding it builds —
/// it only reads back the unused amount — so faithfully tracking a `u128`
/// balance is sufficient.
struct HoldingCredit(u128);

impl UnsafeConstructorDestructor<u128> for HoldingCredit {
	fn unsafe_clone(&self) -> Box<dyn ImbalanceAccounting<u128>> {
		Box::new(HoldingCredit(self.0))
	}
	fn forget_imbalance(&mut self) -> u128 {
		let amount = self.0;
		self.0 = 0;
		amount
	}
}

impl UnsafeManualAccounting<u128> for HoldingCredit {
	fn saturating_subsume(&mut self, mut other: Box<dyn ImbalanceAccounting<u128>>) {
		self.0 = self.0.saturating_add(other.forget_imbalance());
	}
}

impl ImbalanceAccounting<u128> for HoldingCredit {
	fn amount(&self) -> u128 {
		self.0
	}
	fn saturating_take(&mut self, amount: u128) -> Box<dyn ImbalanceAccounting<u128>> {
		let taken = self.0.min(amount);
		self.0 -= taken;
		Box::new(HoldingCredit(taken))
	}
}

/// Convert an [`Asset`] into the [`xcm_executor::AssetsInHolding`] expected by
/// [`xcm_executor::traits::WeightTrader::buy_weight`].
///
/// Stable, moonkit-local replacement for
/// `xcm_executor::test_helpers::mock_asset_to_holding`.
pub(crate) fn asset_to_holding(asset: Asset) -> xcm_executor::AssetsInHolding {
	match asset.fun {
		Fungible(amount) => xcm_executor::AssetsInHolding::new_from_fungible_credit(
			asset.id,
			Box::new(HoldingCredit(amount)),
		),
		NonFungible(instance) => {
			xcm_executor::AssetsInHolding::new_from_non_fungible(asset.id, instance)
		}
	}
}
