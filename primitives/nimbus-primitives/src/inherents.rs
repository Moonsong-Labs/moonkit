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

use sp_inherents::{InherentData, InherentIdentifier};

/// The InherentIdentifier for nimbus's author inherent
pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"author__";

/// A bare minimum inherent data provider that provides no real data.
/// The inherent is simply used as a way to kick off some computation
/// until https://github.com/moonbeam-foundation/substrate/pull/10128 lands.
pub struct InherentDataProvider;

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl sp_inherents::InherentDataProvider for InherentDataProvider {
	async fn provide_inherent_data(
		&self,
		inherent_data: &mut InherentData,
	) -> Result<(), sp_inherents::Error> {
		inherent_data.put_data(INHERENT_IDENTIFIER, &())
	}

	async fn try_handle_error(
		&self,
		identifier: &InherentIdentifier,
		_error: &[u8],
	) -> Option<Result<(), sp_inherents::Error>> {
		// Dont' process modules from other inherents
		if *identifier != INHERENT_IDENTIFIER {
			return None;
		}

		// All errors with the author inehrent are fatal
		Some(Err(sp_inherents::Error::Application(Box::from(
			String::from("Error processing dummy nimbus inherent"),
		))))
	}
}
