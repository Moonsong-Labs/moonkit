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
