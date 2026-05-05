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
use frame_support::traits::{Hooks, PostInherents};
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

#[test]
#[should_panic(expected = "Block invalid, missing author in pre-runtime digest")]
fn test_post_inherents_panics_when_author_missing_from_digest() {
	new_test_ext().execute_with(|| {
		let block_number = 1;
		System::initialize(&block_number, &H256::default(), &Digest { logs: vec![] });

		// post_inherents should panic because there is no pre-runtime digest with the author
		AuthorInherent::post_inherents();
	});
}

#[test]
fn test_on_initialize_then_post_inherents_lifecycle() {
	new_test_ext().execute_with(|| {
		// Block 1: full block with author
		let block_1 = 1;
		System::initialize(
			&block_1,
			&H256::default(),
			&Digest {
				logs: vec![DigestItem::PreRuntime(
					NIMBUS_ENGINE_ID,
					NimbusId::from_slice(&ALICE_NIMBUS).unwrap().encode(),
				)],
			},
		);
		AuthorInherent::on_initialize(block_1);
		assert_eq!(
			None,
			<Author<Test>>::get(),
			"Author must be None after on_initialize"
		);
		AuthorInherent::post_inherents();
		assert_eq!(Some(ALICE), <Author<Test>>::get());

		System::finalize();

		// Block 2: on_initialize clears, then post_inherents sets again
		let block_2 = 2;
		System::initialize(
			&block_2,
			&H256::default(),
			&Digest {
				logs: vec![DigestItem::PreRuntime(
					NIMBUS_ENGINE_ID,
					NimbusId::from_slice(&ALICE_NIMBUS).unwrap().encode(),
				)],
			},
		);
		AuthorInherent::on_initialize(block_2);
		assert_eq!(
			None,
			<Author<Test>>::get(),
			"Author must be None after on_initialize"
		);
		AuthorInherent::post_inherents();
		assert_eq!(
			Some(ALICE),
			<Author<Test>>::get(),
			"Author set again after post_inherents"
		);
	});
}

#[test]
#[should_panic(expected = "No Account Mapped to this NimbusId")]
fn test_post_inherents_panics_when_nimbus_id_is_not_mapped() {
	new_test_ext().execute_with(|| {
		let block_number = 1;
		System::initialize(
			&block_number,
			&H256::default(),
			&Digest {
				logs: vec![DigestItem::PreRuntime(
					NIMBUS_ENGINE_ID,
					NimbusId::from_slice(&[9; 32]).unwrap().encode(),
				)],
			},
		);

		AuthorInherent::post_inherents();
	});
}

#[test]
#[should_panic(expected = "NimbusId encoded in preruntime digest must be valid")]
fn test_post_inherents_panics_when_nimbus_digest_bytes_are_invalid() {
	new_test_ext().execute_with(|| {
		let block_number = 1;
		System::initialize(
			&block_number,
			&H256::default(),
			&Digest {
				logs: vec![DigestItem::PreRuntime(NIMBUS_ENGINE_ID, vec![1, 2, 3])],
			},
		);

		AuthorInherent::post_inherents();
	});
}

#[test]
#[should_panic(expected = "Block invalid, supplied author is not eligible.")]
fn test_post_inherents_panics_when_author_is_ineligible() {
	new_test_ext().execute_with(|| {
		let block_number = 1;
		System::initialize(
			&block_number,
			&H256::default(),
			&Digest {
				logs: vec![DigestItem::PreRuntime(
					NIMBUS_ENGINE_ID,
					NimbusId::from_slice(&BOB_NIMBUS).unwrap().encode(),
				)],
			},
		);

		AuthorInherent::post_inherents();
	});
}
