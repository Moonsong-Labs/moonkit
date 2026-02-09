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

use crate::mock::*;
use crate::pallet::Author;
use frame_support::traits::PostInherents;
use nimbus_primitives::{NimbusId, NIMBUS_ENGINE_ID};
use parity_scale_codec::Encode;
use sp_core::{ByteArray, H256};
use sp_runtime::{Digest, DigestItem};

#[test]
fn test_author_is_extracted_and_stored_from_pre_runtime_digest() {
	new_test_ext().execute_with(|| {
		let block_number = 1;
		System::initialize(
			&block_number,
			&H256::default(),
			&Digest {
				logs: vec![DigestItem::PreRuntime(
					NIMBUS_ENGINE_ID,
					NimbusId::from_slice(&ALICE_NIMBUS).unwrap().encode(),
				)],
			},
		);

		// Initially, no author is set
		assert_eq!(None, <Author<Test>>::get());

		// Call post_inherents which extracts the author from the digest
		// and stores it in the Author storage
		AuthorInherent::post_inherents();

		// Author should now be stored
		assert_eq!(Some(ALICE), <Author<Test>>::get());
	});
}
