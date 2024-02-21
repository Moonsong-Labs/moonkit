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

//! Pallet that allows block authors to include their identity in a block via an inherent.
//! Currently the author does not _prove_ their identity, just states it. So it should not be used,
//! for things like equivocation slashing that require authenticated authorship information.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::traits::{FindAuthor, Get};
use nimbus_primitives::{
	AccountLookup, CanAuthor, NimbusId, SlotBeacon, INHERENT_IDENTIFIER, NIMBUS_ENGINE_ID,
};
use parity_scale_codec::{Decode, Encode, FullCodec};
use sp_inherents::{InherentIdentifier, IsFatalError};
use sp_runtime::{ConsensusEngineId, RuntimeString};

pub use crate::weights::WeightInfo;
pub use exec::BlockExecutor;
pub use pallet::*;

#[cfg(any(test, feature = "runtime-benchmarks"))]
mod benchmarks;

pub mod weights;

mod exec;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// The Author Inherent pallet. The core of the nimbus consensus framework's runtime presence.
	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Type used to refer to a block author.
		type AuthorId: sp_std::fmt::Debug + PartialEq + Clone + FullCodec + TypeInfo + MaxEncodedLen;

		/// A type to convert between NimbusId and AuthorId. This is useful when you want to associate
		/// Block authoring behavior with an AuthorId for rewards or slashing. If you do not need to
		/// hold an AuthorId responsible for authoring use `()` which acts as an identity mapping.
		type AccountLookup: AccountLookup<Self::AuthorId>;

		/// The final word on whether the reported author can author at this height.
		/// This will be used when executing the inherent. This check is often stricter than the
		/// Preliminary check, because it can use more data.
		/// If the pallet that implements this trait depends on an inherent, that inherent **must**
		/// be included before this one.
		type CanAuthor: CanAuthor<Self::AuthorId>;

		/// Some way of determining the current slot for purposes of verifying the author's eligibility
		type SlotBeacon: SlotBeacon;

		type WeightInfo: WeightInfo;
	}

	impl<T> sp_runtime::BoundToRuntimeAppPublic for Pallet<T> {
		type Public = NimbusId;
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Author already set in block.
		AuthorAlreadySet,
		/// No AccountId was found to be associated with this author
		NoAccountId,
		/// The author in the inherent is not an eligible author.
		CannotBeAuthor,
	}

	/// Author of current block.
	#[pallet::storage]
	pub type Author<T: Config> = StorageValue<_, T::AuthorId, OptionQuery>;

	/// Check if the inherent was included
	#[pallet::storage]
	pub type InherentIncluded<T: Config> = StorageValue<_, bool, ValueQuery>;

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_: BlockNumberFor<T>) -> Weight {
			// Now extract the author from the digest
			let digest = <frame_system::Pallet<T>>::digest();
			let pre_runtime_digests = digest.logs.iter().filter_map(|d| d.as_pre_runtime());
			if let Some(author) = Self::find_author(pre_runtime_digests) {
				// Store the author so we can confirm eligibility after the inherents have executed
				<Author<T>>::put(&author);
			}

			// on_initialize: 1 write
			// on_finalize: 1 read + 1 write
			T::DbWeight::get().reads_writes(1, 2)
		}
		fn on_finalize(_: BlockNumberFor<T>) {
			// According to parity, the only way to ensure that a mandatory inherent is included
			// is by checking on block finalization that the inherent set a particular storage item:
			// https://github.com/paritytech/polkadot-sdk/issues/2841#issuecomment-1876040854
			assert!(
				InherentIncluded::<T>::take(),
				"Block invalid, missing inherent `kick_off_authorship_validation`"
			);
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// This inherent is a workaround to run code after the "real" inherents have executed,
		/// but before transactions are executed.
		// This should go into on_post_inherents when it is ready https://github.com/paritytech/substrate/pull/10128
		// TODO better weight. For now we just set a somewhat conservative fudge factor
		#[pallet::call_index(0)]
		#[pallet::weight((T::WeightInfo::kick_off_authorship_validation(), DispatchClass::Mandatory))]
		pub fn kick_off_authorship_validation(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			ensure_none(origin)?;

			// First check that the slot number is valid (greater than the previous highest)
			let new_slot = T::SlotBeacon::slot();

			// Now check that the author is valid in this slot
			assert!(
				T::CanAuthor::can_author(&Self::get(), &new_slot),
				"Block invalid, supplied author is not eligible."
			);

			InherentIncluded::<T>::put(true);

			Ok(Pays::No.into())
		}
	}

	#[pallet::inherent]
	impl<T: Config> ProvideInherent for Pallet<T> {
		type Call = Call<T>;
		type Error = InherentError;
		const INHERENT_IDENTIFIER: InherentIdentifier = INHERENT_IDENTIFIER;

		fn is_inherent_required(_: &InherentData) -> Result<Option<Self::Error>, Self::Error> {
			// Return Ok(Some(_)) unconditionally because this inherent is required in every block
			// If it is not found, throw an AuthorInherentRequired error.
			Ok(Some(InherentError::Other(
				sp_runtime::RuntimeString::Borrowed(
					"Inherent required to manually initiate author validation",
				),
			)))
		}

		// Regardless of whether the client is still supplying the author id,
		// we will create the new empty-payload inherent extrinsic.
		fn create_inherent(_data: &InherentData) -> Option<Self::Call> {
			Some(Call::kick_off_authorship_validation {})
		}

		fn is_inherent(call: &Self::Call) -> bool {
			matches!(call, Call::kick_off_authorship_validation { .. })
		}
	}

	impl<T: Config> FindAuthor<T::AuthorId> for Pallet<T> {
		fn find_author<'a, I>(digests: I) -> Option<T::AuthorId>
		where
			I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
		{
			for (id, mut data) in digests.into_iter() {
				if id == NIMBUS_ENGINE_ID {
					let author_id = NimbusId::decode(&mut data)
						.expect("NimbusId encoded in preruntime digest must be valid");

					let author_account = T::AccountLookup::lookup_account(&author_id)
						.expect("No Account Mapped to this NimbusId");

					return Some(author_account);
				}
			}

			None
		}
	}

	impl<T: Config> Get<T::AuthorId> for Pallet<T> {
		fn get() -> T::AuthorId {
			Author::<T>::get().expect("Block author not inserted into Author Inherent Pallet")
		}
	}

	/// To learn whether a given NimbusId can author, as opposed to an account id, you
	/// can ask this pallet directly. It will do the mapping for you.
	impl<T: Config> CanAuthor<NimbusId> for Pallet<T> {
		fn can_author(author: &NimbusId, slot: &u32) -> bool {
			let account = match T::AccountLookup::lookup_account(author) {
				Some(account) => account,
				// Authors whose account lookups fail will not be eligible
				None => {
					return false;
				}
			};

			T::CanAuthor::can_author(&account, slot)
		}
		#[cfg(feature = "runtime-benchmarks")]
		fn set_eligible_author(slot: &u32) {
			let eligible_authors = T::CanAuthor::get_authors(slot);
			if let Some(author) = eligible_authors.first() {
				Author::<T>::put(author)
			}
		}
	}
}

#[derive(Encode)]
#[cfg_attr(feature = "std", derive(Debug, Decode))]
pub enum InherentError {
	Other(RuntimeString),
}

impl IsFatalError for InherentError {
	fn is_fatal_error(&self) -> bool {
		match *self {
			InherentError::Other(_) => true,
		}
	}
}

impl InherentError {
	/// Try to create an instance ouf of the given identifier and data.
	#[cfg(feature = "std")]
	pub fn try_from(id: &InherentIdentifier, data: &[u8]) -> Option<Self> {
		if id == &INHERENT_IDENTIFIER {
			<InherentError as parity_scale_codec::Decode>::decode(&mut &data[..]).ok()
		} else {
			None
		}
	}
}
