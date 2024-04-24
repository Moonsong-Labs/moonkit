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

use xcm::latest::{Junction::*, Location};

pub struct XcmSiblingDestinationGenerator;
impl XcmSiblingDestinationGenerator {
	pub fn generate(para_id: u32) -> Location {
		Location::new(1, Parachain(para_id))
	}
}

pub struct XcmLocalBeneficiary20Generator;
impl XcmLocalBeneficiary20Generator {
	pub fn generate(key: [u8; 20]) -> Location {
		Location::new(0, AccountKey20 { network: None, key })
	}
}

pub struct XcmLocalBeneficiary32Generator;
impl XcmLocalBeneficiary32Generator {
	pub fn generate(id: [u8; 32]) -> Location {
		Location::new(0, AccountId32 { network: None, id })
	}
}
