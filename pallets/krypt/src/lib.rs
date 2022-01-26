//! Primitive Krypto-Currency
//! Following ERC-20 standard for fungible tokens
//! Functionalities that are included :
//! function totalSupply() public view returns (uint256);
//! function balanceOf(address tokenOwner) public view returns (uint);
//! function allowance(address tokenOwner, address spender)
//! public view returns (uint);
//! function transfer(address to, uint tokens) public returns (bool);
//! function transferFrom(address from, address to, uint tokens) public returns (bool);
//! function approve(address spender, uint tokens)  public returns (bool);
//! string public constant name;
//! string public constant symbol;
//! uint8 public constant decimals;
#![cfg_attr(not(feature = "std"), no_std)]
pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use codec::{Codec, MaxEncodedLen};
	use core::convert::Infallible;
	#[allow(unused)]
	use frame_support::traits::{Currency, Imbalance, TryDrop};
	use frame_support::{
		dispatch::{DispatchErrorWithPostInfo, DispatchResult, DispatchResultWithPostInfo},
		traits::tokens::Balance,
	};
	use frame_support::{pallet_prelude::*, Blake2_128Concat, Twox64Concat};
	use frame_system::{ensure_root, ensure_signed, pallet_prelude::*};
	use sp_std::{fmt::Debug, iter::Sum};
	// use frame_support::{
	// 	sp_runtime::traits::{Hash, Zero},
	// 	dispatch::{DispatchResultWithPostInfo, DispatchResult},
	// 	traits::{Currency, ExistenceRequirement, Randomness},
	// };
	// use frame_support::weights::PostDispatchInfo;
	use scale_info::TypeInfo;
	// use sp_io::hashing::blake2_128;
	use sp_runtime::{
		traits::{AtLeast32BitUnsigned, CheckedAdd, CheckedSub, Saturating, StaticLookup, Zero},
		ArithmeticError,
	};

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: IsType<<Self as frame_system::Config>::Event> + From<Event<Self>>;
		type Balance: Member
			+ Parameter
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
		#[pallet::constant]
		type MaxTokenSupply: Get<Self::Balance>;
	}

	#[pallet::error]
	pub enum Error<T> {
		/// When minting overflows constant `MaxTokenSupply`
		MintCausingTotalSupplyOverflow,
		/// When minting overflows the bounds of the concrete type managing balances
		MintTypeOverflow,
		/// Insufficient Funds for operation
		InsufficientFunds,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		MintedNewSupply(<T as Config>::Balance),
		TransferSuccess(T::AccountId, T::AccountId, T::Balance),
		// Writing <T as Config>::Balance in order to avoid confusion
		// with the Runtime's instance of Balance (from Balances pallet)
		// is not necessary because of the T: Conig trait bound on this Event
		TotalIssued(T::Balance),
	}

	/// Total supply that has been so far minted and in circulation
	/// Note: This is different from MaxTokenSupply which defines the upper limit for
	/// the number of tokens
	#[pallet::storage]
	#[pallet::getter(fn total_issued)]
	pub(super) type TotalIssued<T: Config> = StorageValue<_, T::Balance, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_balance_of)]
	/// Mapping of Account -> Balance
	pub(super) type BalanceToAccount<T: Config> = StorageMap<
		_,
		// Remember to use a cryptographic hash function for sensitive information
		Blake2_128Concat,
		T::AccountId,
		T::Balance,
		ValueQuery,
	>;

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		/// Allow only Root to mint new tokens & transfer it to some benefactor account
		/// Set a hard uppper limit on the total number of tokens in supply
		pub fn mint(
			origin: OriginFor<T>,
			#[pallet::compact] amount: <T as Config>::Balance,
			benefactor: T::AccountId,
		) -> DispatchResult {
			// Check if sudo call
			ensure_root(origin.clone())?;

			// Ensure No MaxTokenSupply or Balance type overflow
			ensure!(
				Self::does_adding_overflow_maxtokensupply(amount).is_ok(),
				Error::<T>::MintCausingTotalSupplyOverflow
			);

			// Check if Benefactor already has funds
			let previous_balance = <BalanceToAccount<T>>::try_get(&benefactor).unwrap_or_default();
			let final_balance = previous_balance.saturating_add(amount);
			<BalanceToAccount<T>>::insert(&benefactor, final_balance);
			// Call to this helper updates `TotalIssued` storage item that tracks all minted counts in existence
			Self::include_mint_amount(amount);
			Self::deposit_event(Event::MintedNewSupply(amount));
			Ok(().into())
		}

		/// Transfer funds from `from` to `to`
		#[pallet::weight(10_000)]
		pub fn transfer_from(
			origin: OriginFor<T>,
			to: <T::Lookup as StaticLookup>::Source,
			#[pallet::compact] amount: T::Balance,
		) -> DispatchResult {
			// Check if origin is signed and has funds
			let sender = ensure_signed(origin)?;
			ensure!(Self::has_sufficient_funds(&sender, amount), Error::<T>::InsufficientFunds);
			let to = T::Lookup::lookup(to)?;
			Self::transfer_unchecked(&sender, &to, amount).expect("Shouldn't fail");
			Self::deposit_event(Event::TransferSuccess(sender, to, amount));
			Ok(().into())
		}

		#[pallet::weight(10_000)]
		pub fn total_issuance(origin: OriginFor<T>) -> DispatchResult {
			ensure_signed(origin)?;
			Self::deposit_event(Event::<T>::TotalIssued(Self::total_issued()));
			Ok(().into())
		}
	}

	// Private Helper functions
	impl<T: Config> Pallet<T> {
		fn include_mint_amount(amount: T::Balance) {
			// This call shouldn't go overbound because the only caller to this function is `mint` and
			// they check for overflow errors
			TotalIssued::<T>::put(amount.checked_add(&Self::total_issued()).expect("Cannot fail"));
		}

		fn does_adding_overflow_maxtokensupply(amount: T::Balance) -> Result<(), Error<T>> {
			let total_already_minted: T::Balance = Self::total_issued();

			let new_supply =
				total_already_minted.checked_add(&amount).ok_or(Error::<T>::MintTypeOverflow)?;

			// Check that new mint doesn't exceed MaxTokenSupply
			if new_supply <= T::MaxTokenSupply::get() {
				Ok(())
			} else {
				Err(Error::<T>::MintCausingTotalSupplyOverflow)
			}
		}

		fn has_sufficient_funds(s: &T::AccountId, amount: T::Balance) -> bool {
			match BalanceToAccount::<T>::try_get(&s).ok() {
				Some(balance) if balance >= amount => true,
				_ => false,
			}
		}

		fn transfer_unchecked(
			sender: &T::AccountId,
			to: &T::AccountId,
			amount: T::Balance,
		) -> Result<(), Infallible> {
			let previous_sender_balance = Self::get_balance_of(sender);
			// We've already performed the safety check in `has_sufficient_funds`
			let new_sender_balance = previous_sender_balance
				.checked_sub(&amount)
				.expect("Never has insufficient balance though");
			BalanceToAccount::<T>::insert(&sender, new_sender_balance);
			BalanceToAccount::<T>::insert(&to, amount);

			Ok(())
		}
	}
}
