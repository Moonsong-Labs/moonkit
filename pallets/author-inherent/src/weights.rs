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


//! Autogenerated weights for pallet_author_inherent
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 4.0.0-dev
//! DATE: 2023-05-02, STEPS: `50`, REPEAT: `20`, LOW RANGE: `[]`, HIGH RANGE: `[]`
//! WORST CASE MAP SIZE: `1000000`
//! HOSTNAME: `benchmarker`, CPU: `Intel(R) Core(TM) i7-7700K CPU @ 4.20GHz`
//! EXECUTION: Some(Wasm), WASM-EXECUTION: Compiled, CHAIN: None, DB CACHE: 1024

// Executed Command:
// ./target/release/moonbeam
// benchmark
// pallet
// --execution=wasm
// --wasm-execution=compiled
// --pallet
// *
// --extrinsic
// *
// --steps
// 50
// --repeat
// 20
// --template=./benchmarking/frame-weight-template.hbs
// --json-file
// raw.json
// --output
// weights/

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use sp_std::marker::PhantomData;

/// Weight functions needed for pallet_author_inherent.
pub trait WeightInfo {
	fn kick_off_authorship_validation() -> Weight;
}

/// Weights for pallet_author_inherent using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: ParachainSystem ValidationData (r:1 w:0)
	/// Proof Skipped: ParachainSystem ValidationData (max_values: Some(1), max_size: None, mode: Measured)
	/// Storage: AuthorInherent HighestSlotSeen (r:1 w:1)
	/// Proof: AuthorInherent HighestSlotSeen (max_values: Some(1), max_size: Some(4), added: 499, mode: MaxEncodedLen)
	/// Storage: AuthorInherent Author (r:1 w:0)
	/// Proof: AuthorInherent Author (max_values: Some(1), max_size: Some(20), added: 515, mode: MaxEncodedLen)
	/// Storage: ParachainStaking SelectedCandidates (r:1 w:0)
	/// Proof Skipped: ParachainStaking SelectedCandidates (max_values: Some(1), max_size: None, mode: Measured)
	/// Storage: AuthorFilter EligibleCount (r:1 w:0)
	/// Proof Skipped: AuthorFilter EligibleCount (max_values: Some(1), max_size: None, mode: Measured)
	/// Storage: Randomness PreviousLocalVrfOutput (r:1 w:0)
	/// Proof Skipped: Randomness PreviousLocalVrfOutput (max_values: Some(1), max_size: None, mode: Measured)
	fn kick_off_authorship_validation() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `371`
		//  Estimated: `10418`
		// Minimum execution time: 25_775_000 picoseconds.
		Weight::from_parts(26_398_000, 10418)
			.saturating_add(T::DbWeight::get().reads(6_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
}

// For backwards compatibility and tests
impl WeightInfo for () {
	/// Storage: ParachainSystem ValidationData (r:1 w:0)
	/// Proof Skipped: ParachainSystem ValidationData (max_values: Some(1), max_size: None, mode: Measured)
	/// Storage: AuthorInherent HighestSlotSeen (r:1 w:1)
	/// Proof: AuthorInherent HighestSlotSeen (max_values: Some(1), max_size: Some(4), added: 499, mode: MaxEncodedLen)
	/// Storage: AuthorInherent Author (r:1 w:0)
	/// Proof: AuthorInherent Author (max_values: Some(1), max_size: Some(20), added: 515, mode: MaxEncodedLen)
	/// Storage: ParachainStaking SelectedCandidates (r:1 w:0)
	/// Proof Skipped: ParachainStaking SelectedCandidates (max_values: Some(1), max_size: None, mode: Measured)
	/// Storage: AuthorFilter EligibleCount (r:1 w:0)
	/// Proof Skipped: AuthorFilter EligibleCount (max_values: Some(1), max_size: None, mode: Measured)
	/// Storage: Randomness PreviousLocalVrfOutput (r:1 w:0)
	/// Proof Skipped: Randomness PreviousLocalVrfOutput (max_values: Some(1), max_size: None, mode: Measured)
	fn kick_off_authorship_validation() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `371`
		//  Estimated: `10418`
		// Minimum execution time: 25_775_000 picoseconds.
		Weight::from_parts(26_398_000, 10418)
			.saturating_add(RocksDbWeight::get().reads(6_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
}