// This file is part of Bifrost.

// Copyright (C) 2019-2022 Liebi Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{pallet_prelude::*, weights::Weight};
use frame_system::{ensure_root, ensure_signed, pallet_prelude::OriginFor};
use node_primitives::{CurrencyId, TokenSymbol};
use orml_traits::MultiCurrency;
pub use primitives::{Delays, Ledger, TimeUnit};
use sp_runtime::traits::UniqueSaturatedFrom;
pub use weights::WeightInfo;
use xcm::latest::*;

use crate::{
	primitives::{MinimumsMaximums, SubstrateLedger, XcmOperation},
	traits::{DelegatorManager, StakingAgent, StakingFeeManager, ValidatorManager},
};

mod agents;
mod mock;
pub mod primitives;
mod tests;
pub mod traits;
pub mod weights;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub use pallet::*;

type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type BalanceOf<T> = <<T as Config>::MultiCurrency as MultiCurrency<AccountIdOf<T>>>::Balance;

/// Simplify the CurrencyId.
const KSM: CurrencyId = CurrencyId::Token(TokenSymbol::KSM);

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// Currency operations handler
		type MultiCurrency: MultiCurrency<AccountIdOf<Self>, CurrencyId = CurrencyId>;
		/// The only origin that can modify pallet params
		type ControlOrigin: EnsureOrigin<Self::Origin>;

		/// Set default weight.
		type WeightInfo: WeightInfo;

		/// Kusama agent
		type KusamaAgent: StakingAgent<MultiLocation, MultiLocation, BalanceOf<Self>>
			+ StakingFeeManager<AccountIdOf<Self>, BalanceOf<Self>>
			+ DelegatorManager<MultiLocation, SubstrateLedger<MultiLocation, BalanceOf<Self>>>
			+ ValidatorManager<MultiLocation>;
	}

	#[pallet::error]
	pub enum Error<T> {
		OperateOriginNotSet,
		NotAuthorized,
		NotSupportedCurrencyId,
		FailToInitializeDelegator,
		FailToBond,
		OverFlow,
		NotExist,
		LowerThanMinimum,
		AlreadyBonded,
		DelegatorNotExist,
		XcmFailure,
		DelegatorNotBonded,
		ExceedActiveMaximum,
		ProblematicLedger,
		NotEnoughToUnbond,
		ExceedUnlockingRecords,
		RebondExceedUnlockingAmount,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub (crate) fn deposit_event)]
	pub enum Event<T: Config> {
		/// [CurrencyId, DelegatorId]
		DelegatorInitialized(CurrencyId, MultiLocation),
		/// [CurrencyId, DelegatorId, BondAmount]
		DelegatorBonded(CurrencyId, MultiLocation, BalanceOf<T>),
		/// [CurrencyId, DelegatorId, BondExtraAmount]
		DelegatorBondExtra(CurrencyId, MultiLocation, BalanceOf<T>),
		/// [CurrencyId, DelegatorId, UnbondAmount]
		DelegatorUnbond(CurrencyId, MultiLocation, BalanceOf<T>),
	}

	/// The dest weight limit and fee for execution XCM msg sended out. Must be
	/// sufficient, otherwise the execution of XCM msg on the dest chain will fail.
	///
	/// XcmDestWeightAndFee: DoubleMap: CurrencyId, XcmOperation => (Weight, Balance)
	#[pallet::storage]
	#[pallet::getter(fn xcm_dest_weight_and_fee)]
	pub type XcmDestWeightAndFee<T> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		CurrencyId,
		Blake2_128Concat,
		XcmOperation,
		(Weight, BalanceOf<T>),
		ValueQuery,
		DefaultXcmDestWeightAndFee<T>,
	>;

	// Default Xcm Dest Weight And Fee if not found.
	#[pallet::type_value]
	pub fn DefaultXcmDestWeightAndFee<T: Config>() -> (Weight, BalanceOf<T>) {
		(5_000_000_000 as Weight, BalanceOf::<T>::unique_saturated_from(1_000_000_000_000u128))
	}

	/// One operate origin(can be a multisig account) for a currency. An operating origins are
	/// normal account in Bifrost chain.
	#[pallet::storage]
	#[pallet::getter(fn get_operate_origin)]
	pub type OperateOrigins<T> = StorageMap<_, Blake2_128Concat, CurrencyId, AccountIdOf<T>>;

	/// Record current TimeUnit for a certain chain. For example, Kusama's current era is 808.
	#[pallet::storage]
	#[pallet::getter(fn get_current_time_unit)]
	pub type CurrentTimeUnit<T> = StorageMap<_, Blake2_128Concat, CurrencyId, TimeUnit>;

	/// Currency delays for payouts, delegate, unbond and so on.
	#[pallet::storage]
	#[pallet::getter(fn get_currency_delays)]
	pub type CurrencyDelays<T> = StorageMap<_, Blake2_128Concat, CurrencyId, Delays>;
	/// Origins and Amounts for the staking operating account fee supplement. An operating account
	/// is identified in MultiLocation format.
	#[pallet::storage]
	#[pallet::getter(fn get_fee_source)]
	pub type FeeSources<T> =
		StorageMap<_, Blake2_128Concat, CurrencyId, (MultiLocation, BalanceOf<T>)>;

	/// Delegators in service. A delegator is identified in MultiLocation format.
	/// Currency Id + Sub-account index => MultiLocation
	#[pallet::storage]
	#[pallet::getter(fn get_delegator_multilocation_by_index)]
	pub type DelegatorsIndex2Multilocation<T> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		CurrencyId,
		Blake2_128Concat,
		u16,
		MultiLocation,
		OptionQuery,
	>;

	/// Delegators in service. Currency Id + MultiLocation => Sub-account index
	#[pallet::storage]
	#[pallet::getter(fn get_delegator_index_by_multilocation)]
	pub type DelegatorsMultilocation2Index<T> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		CurrencyId,
		Blake2_128Concat,
		MultiLocation,
		u16,
		OptionQuery,
	>;

	/// Next index of different currency delegators.
	#[pallet::storage]
	#[pallet::getter(fn get_delegator_next_index)]
	pub type DelegatorNextIndex<T> = StorageMap<_, Blake2_128Concat, CurrencyId, u16, ValueQuery>;

	/// Validator in service. A validator is identified in MultiLocation format.
	#[pallet::storage]
	#[pallet::getter(fn get_validators)]
	pub type Validators<T> = StorageMap<_, Blake2_128Concat, CurrencyId, Vec<MultiLocation>>;

	/// Validators for each delegator. CurrencyId + Delegator => Vec<Validator>
	#[pallet::storage]
	#[pallet::getter(fn get_validators_by_delegator)]
	pub type ValidatorsByDelegator<T> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		CurrencyId,
		Blake2_128Concat,
		MultiLocation,
		Vec<MultiLocation>,
		OptionQuery,
	>;

	/// Delegator ledgers. A delegator is identified in MultiLocation format.
	#[pallet::storage]
	#[pallet::getter(fn get_delegator_ledger)]
	pub type DelegatorLedgers<T> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		CurrencyId,
		Blake2_128Concat,
		MultiLocation,
		Ledger<MultiLocation, BalanceOf<T>>,
		OptionQuery,
	>;

	/// Minimum and Maximum constraints for different chains.
	#[pallet::storage]
	#[pallet::getter(fn get_minimums_maximums)]
	pub type MinimumsAndMaximums<T> =
		StorageMap<_, Blake2_128Concat, CurrencyId, MinimumsMaximums<BalanceOf<T>>>;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::hooks]
	impl<T: Config> Hooks<T::BlockNumber> for Pallet<T> {
		fn on_initialize(_n: T::BlockNumber) -> Weight {
			0
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// *****************************/
		/// ****** Outer Calls ******/
		/// *****************************/
		///
		/// Delegator initialization work. Generate a new delegator and return its ID.
		#[pallet::weight(T::WeightInfo::initialize_delegator())]
		pub fn initialize_delegator(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let delegator_id = match currency_id {
				KSM => <T::KusamaAgent as StakingAgent<
					MultiLocation,
					MultiLocation,
					BalanceOf<T>,
				>>::initialize_delegator(),
				_ => Err(Error::<T>::NotSupportedCurrencyId)?,
			}
			.ok_or(Error::<T>::FailToInitializeDelegator)?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorInitialized(currency_id, delegator_id));

			Ok(())
		}

		/// First time bonding some amount to a delegator.
		#[pallet::weight(T::WeightInfo::bond())]
		pub fn bond(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let _ = match currency_id {
				KSM => <T::KusamaAgent as StakingAgent<
					MultiLocation,
					MultiLocation,
					BalanceOf<T>,
				>>::bond(who.clone(), amount),
				_ => Err(Error::<T>::NotSupportedCurrencyId)?,
			};

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorBonded(currency_id, who, amount));

			Ok(())
		}

		/// Bond extra amount to a delegator.
		#[pallet::weight(T::WeightInfo::bond_extra())]
		pub fn bond_extra(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let _ = match currency_id {
				KSM => <T::KusamaAgent as StakingAgent<
					MultiLocation,
					MultiLocation,
					BalanceOf<T>,
				>>::bond_extra(who.clone(), amount),
				_ => Err(Error::<T>::NotSupportedCurrencyId)?,
			};

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorBondExtra(currency_id, who, amount));
			Ok(())
		}

		/// Bond extra amount to a delegator.
		#[pallet::weight(T::WeightInfo::unbond())]
		pub fn unbond(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let _ = match currency_id {
				KSM => <T::KusamaAgent as StakingAgent<
					MultiLocation,
					MultiLocation,
					BalanceOf<T>,
				>>::unbond(who.clone(), amount),
				_ => Err(Error::<T>::NotSupportedCurrencyId)?,
			};

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorUnbond(currency_id, who, amount));
			Ok(())
		}

		/// *****************************/
		/// ****** Storage Setters ******/
		/// *****************************/
		///
		/// Update storage XcmDestWeightAndFee<T>.
		#[pallet::weight(T::WeightInfo::set_xcm_dest_weight_and_fee())]
		pub fn set_xcm_dest_weight_and_fee(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			operation: XcmOperation,
			weight: Weight,
			fee: BalanceOf<T>,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage OperateOrigins<T>.
		#[pallet::weight(T::WeightInfo::set_operate_origin())]
		pub fn set_operate_origin(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: AccountIdOf<T>,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage CurrentTimeUnit<T>.
		#[pallet::weight(T::WeightInfo::set_current_time_unit())]
		pub fn set_current_time_unit(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			time_unit: TimeUnit,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage CurrencyDelays<T>.
		#[pallet::weight(T::WeightInfo::set_currency_delays())]
		pub fn set_currency_delays(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			delays: Delays,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage FeeSources<T>.
		#[pallet::weight(T::WeightInfo::set_fee_source())]
		pub fn set_fee_source(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			fee: BalanceOf<T>,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage DelegatorsIndex2Multilocation<T> 和 DelegatorsMultilocation2Index<T>.
		#[pallet::weight(T::WeightInfo::set_delegators())]
		pub fn set_delegators(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			index: u16,
			who: MultiLocation,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage Validators<T>.
		#[pallet::weight(T::WeightInfo::set_validators())]
		pub fn set_validators(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			validators: Vec<MultiLocation>,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage ValidatorsByDelegator<T>.
		#[pallet::weight(T::WeightInfo::set_validators_by_delegator())]
		pub fn set_validators_by_delegator(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			validators: Vec<MultiLocation>,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage DelegatorLedgers<T>.
		#[pallet::weight(T::WeightInfo::set_delegator_ledger())]
		pub fn set_delegator_ledger(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			ledger: Ledger<MultiLocation, BalanceOf<T>>,
		) -> DispatchResult {
			unimplemented!()
		}

		/// Update storage MinimumsAndMaximums<T>.
		#[pallet::weight(T::WeightInfo::set_delegator_ledger())]
		pub fn set_minimums_and_maximums(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			constraints: MinimumsMaximums<BalanceOf<T>>,
		) -> DispatchResult {
			unimplemented!()
		}
	}

	impl<T: Config> Pallet<T> {
		/// Ensure privileged origin
		fn ensure_authorized(origin: OriginFor<T>, currency_id: CurrencyId) -> bool {
			let operator = ensure_signed(origin.clone()).ok();
			let privileged = OperateOrigins::<T>::get(currency_id);

			let cond1 = operator == privileged;
			let cond2 = ensure_root(origin.clone()).is_ok();

			cond1 & cond2
		}
	}
}
