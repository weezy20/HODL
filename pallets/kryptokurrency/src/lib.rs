//! A first implementation of the Currency and Imbalance traits
//!

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use codec::{Codec, MaxEncodedLen};
	use frame_support::traits::{Currency, Imbalance, TryDrop};
	use frame_support::{pallet_prelude::*, RuntimeDebug};
	use frame_system::pallet_prelude::*;
	use scale_info::TypeInfo;
	use sp_runtime::traits::{
		AtLeast32BitUnsigned, CheckedAdd, CheckedSub, Saturating, StaticLookup, Zero,
	};
	use sp_std::{fmt::Debug, iter::Sum};

	#[pallet::event]
	pub enum Event<T: Config> {
		MintedNewSupply(T::Balance),
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::generate_storage_info]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The Event type
		type Event: IsType<<Self as frame_system::Config>::Event> + From<Event<Self>>;
		/// A type that represents a Balance
		type Balance: Member
			+ Parameter
			+ MaybeSerializeDeserialize
			+ AtLeast32BitUnsigned
			+ Default
			+ Copy
			+ TypeInfo
			+ Codec
			+ Debug
			+ Sum
			+ Zero
			+ Copy
			+ MaxEncodedLen;
		/// Maximum number of Tokens possible in this Chain
		#[pallet::constant]
		type MaxTokenSupply: Get<Self::Balance>;
	}

	/// Account -> Balance map
	#[pallet::storage]
	#[pallet::getter(fn account_of)]
	pub type AccountStore<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, AccountData<T::Balance>>;

	/// Account details for some AccountId
	#[derive(
		Encode, Decode, Clone, PartialEq, Eq, Default, RuntimeDebug, MaxEncodedLen, TypeInfo,
	)]
	pub struct AccountData<Balance> {
		pub free: Balance,
		pub locked: Balance,
	}
	#[allow(unused)]
	impl<Balance: Copy + Ord + Saturating> AccountData<Balance> {
		/// Returns free balance
		fn usable(&self) -> Balance {
			self.free
		}
		fn total(&self) -> Balance {
			self.free.saturating_add(self.locked)
		}
		fn locked(&self) -> Balance {
			self.locked
		}
	}

	/// Storage for Total Issuance
	#[pallet::storage]
	#[pallet::getter(fn total_issuance)]
	pub type TotalIssuance<T: Config> = StorageValue<_, T::Balance>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub balances: Vec<(T::AccountId, T::Balance)>,
		pub max_token_supply: Option<T::Balance>,
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			let runtime_max_token: T::Balance = T::MaxTokenSupply::get();
			let max_tokens =
				if runtime_max_token.is_zero() { None } else { Some(runtime_max_token) };

			Self { balances: Default::default(), max_token_supply: max_tokens }
		}
	}
	/// Build the genesis config storage for allowing
	/// accounts to be endowed at genesis
	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			let total_issuance_at_genesis: T::Balance = self
				.balances
				.iter()
				.fold(Zero::zero(), |acc: T::Balance, &(_, curr)| acc + curr);
			let max_tokens_at_genesis: T::Balance = match self.max_token_supply {
				Some(t) => t,
				None => Zero::zero(),
			};
			// Check if total_issuance_at_genesis doesn't overflow max_tokens_at_genesis
			assert!(
				total_issuance_at_genesis <= max_tokens_at_genesis,
				"Total sum in endowed accounts cannot exceed MaxTokenSupply"
			);

			let endowed_accounts = self
				.balances
				.iter()
				.map(|(acc, _)| acc)
				.cloned()
				.collect::<std::collections::BTreeSet<_>>();

			assert!(
				endowed_accounts.len() == self.balances.len(),
				"Duplicate entries for accounts in genesis"
			);

			self.balances.iter().for_each(|&(ref who, ref free)| {
				AccountStore::<T>::insert(
					who.clone(),
					AccountData { free: free.clone(), ..Default::default() },
				)
			});
			TotalIssuance::<T>::put(total_issuance_at_genesis);
		}
	}

	mod imbalance {
		use super::{Config, Imbalance, RuntimeDebug, Saturating, TryDrop, Zero};
		use core::{cmp::Ordering, result::Result};
		use frame_support::traits::{Get, SameOrOther};
		use sp_std::mem;

		#[derive(RuntimeDebug, PartialEq, Eq)]
		pub struct PositiveImbalance<T: Config>(<T as Config>::Balance);

		#[derive(RuntimeDebug, PartialEq, Eq)]
		pub struct NegativeImbalance<T: Config>(<T as Config>::Balance);

		impl<T: Config> PositiveImbalance<T> {
			fn new(amount: T::Balance) -> Self {
				PositiveImbalance(amount)
			}
		}

		impl<T: Config> Default for PositiveImbalance<T> {
			fn default() -> Self {
				// Imbalance method
				Self::zero()
			}
		}

		// TryDrop for dropping without consideration meaning if the imbalance is zero
		impl<T: Config> TryDrop for PositiveImbalance<T> {
			fn try_drop(self) -> Result<(), Self> {
				// Imbalance method
				self.drop_zero()
			}
		}
		/// Some amount was minted and as a result a PositiveImbalance was returned
		/// Increase TotalIssuance by amount until MaxTokenSupply is hit
		impl<T: Config> Drop for PositiveImbalance<T> {
			fn drop(&mut self) {
				super::TotalIssuance::<T>::mutate(|key| {
					if let Some(total) = *key {
						total.saturating_add(self.0);
						total
					} else {
						T::MaxTokenSupply::get()
					}
				});
			}
		}
		impl<T: Config> NegativeImbalance<T> {
			fn new(amount: T::Balance) -> Self {
				NegativeImbalance(amount)
			}
		}

		impl<T: Config> Default for NegativeImbalance<T> {
			fn default() -> Self {
				// Imbalance method
				Self::zero()
			}
		}

		// TryDrop for dropping without consideration meaning if the imbalance is zero
		impl<T: Config> TryDrop for NegativeImbalance<T> {
			fn try_drop(self) -> Result<(), Self> {
				// Imbalance method
				self.drop_zero()
			}
		}
		/// Some amount was burned somewhere, which gave us a NegativeImbalance
		/// Therefore we should decrease TotalIssuance to reflect this change
		/// Note: This doesn't not affect MaxTokenSupply
		impl<T: Config> Drop for NegativeImbalance<T> {
			fn drop(&mut self) {
				super::TotalIssuance::<T>::mutate(|key| {
					if let Some(total) = *key {
						total.saturating_sub(self.0);
						total
					} else {
						T::Balance::zero()
					}
				});
			}
		}

		// Implementation of Imbalance traits :

		impl<T: Config> Imbalance<T::Balance> for PositiveImbalance<T> {
			type Opposite = NegativeImbalance<T>;
			fn zero() -> Self {
				Self(Zero::zero())
			}
			fn drop_zero(self) -> Result<(), Self> {
				if self.0.is_zero() {
					Ok(())
				} else {
					Err(self)
				}
			}
			fn split(self, amount: T::Balance) -> (Self, Self) {
				let first = self.0.min(amount);
				let second = self.0 - first;
				// Forget the original imbalance without running Drop
				mem::forget(self);
				(Self(first), Self(second))
			}
			fn merge(mut self, other: Self) -> Self {
				self.0 = self.0.saturating_add(other.0);
				mem::forget(other);
				self
			}
			fn subsume(&mut self, other: Self) {
				self.0 = self.0.saturating_add(other.0);
				mem::forget(other);
			}
			fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
				match self.0.cmp(&other.0) {
					Ordering::Less => SameOrOther::Other(Self::Opposite::new(other.0 - self.0)),
					Ordering::Greater => SameOrOther::Same(Self::new(self.0 - other.0)),
					Ordering::Equal => SameOrOther::None,
				}
			}
			fn peek(&self) -> T::Balance {
				self.0.clone()
			}
		}
		impl<T: Config> Imbalance<T::Balance> for NegativeImbalance<T> {
			type Opposite = PositiveImbalance<T>;
			fn zero() -> Self {
				Self(Zero::zero())
			}
			fn drop_zero(self) -> Result<(), Self> {
				if self.0.is_zero() {
					Ok(())
				} else {
					Err(self)
				}
			}
			fn split(self, amount: T::Balance) -> (Self, Self) {
				let first = self.0.min(amount);
				let second = self.0 - first;
				// Forget the original imbalance without running Drop
				mem::forget(self);
				(Self(first), Self(second))
			}
			fn merge(mut self, other: Self) -> Self {
				self.0 = self.0.saturating_add(other.0);
				mem::forget(other);
				self
			}
			fn subsume(&mut self, other: Self) {
				self.0 = self.0.saturating_add(other.0);
				mem::forget(other);
			}
			fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
				match self.0.cmp(&other.0) {
					Ordering::Less => SameOrOther::Other(Self::Opposite::new(other.0 - self.0)),
					Ordering::Greater => SameOrOther::Same(Self::new(self.0 - other.0)),
					Ordering::Equal => SameOrOther::None,
				}
			}
			fn peek(&self) -> T::Balance {
				self.0.clone()
			}
		}
	} // mod imbalance

	// Finally we are ready to implement Currenct<T::AccountId> for our pallet
	pub use self::imbalance::{NegativeImbalance, PositiveImbalance};
	// impl<T: Config> Currency<T::AccountId> for Pallet<T> {
	// 	type Balance = <T as Config>::Balance;
	// 	type PositiveImbalance = PositiveImbalance<T>;
	// 	type NegativeImbalance = NegativeImbalance<T>;

	// 	fn total_balance(who: &T::AccountId) -> Self::Balance {
	// 		if let Some(account_data) = AccountStore::<T>::get(who) {
	// 			account_data.total()
	// 		} else {
	// 			Self::Balance::zero()
	// 		}
	// 	}

	// 	fn can_slash(who: &T::AccountId, value: Self::Balance) -> bool {
	// 		if Self::total_balance(who) >= value { true } else { false } 
	// 	}

	// } // End of Currency impl
} // End of pallet
