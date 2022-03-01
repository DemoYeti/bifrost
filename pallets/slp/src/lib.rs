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

pub use agents::KusamaAgent;
use cumulus_primitives_core::ParaId;
use frame_support::{dispatch::result::Result, pallet_prelude::*, weights::Weight};
use frame_system::{ensure_root, ensure_signed, pallet_prelude::OriginFor};
use node_primitives::CurrencyId;
use orml_traits::MultiCurrency;
pub use primitives::{Delays, Ledger, TimeUnit};
use sp_arithmetic::traits::Zero;
use sp_runtime::traits::{Convert, UniqueSaturatedFrom};
pub use weights::WeightInfo;
use xcm::latest::*;

use crate::{
	primitives::{MinimumsMaximums, XcmOperation, KSM},
	traits::{StakingAgent, ValidatorManager, VtokenMintingOperator},
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
type BoxType<T> = Box<dyn StakingAgent<MultiLocation, MultiLocation, BalanceOf<T>, TimeUnit>>;

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

		/// The interface to call VtokenMinting module functions.
		type VtokenMinting: VtokenMintingOperator<CurrencyId, BalanceOf<Self>, TimeUnit>;

		/// Substrate account converter, which can convert a u16 number into a sub-account with
		/// MultiLocation format.
		type AccountConverter: Convert<u16, MultiLocation>;

		/// Parachain Id which is gotten from the runtime.
		type ParachainId: Get<ParaId>;

		/// Routes the XCM message outbound.
		type XcmSender: SendXcm;
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
		GreaterThanMaximum,
		AlreadyBonded,
		AccountNotExist,
		DelegatorNotExist,
		XcmFailure,
		DelegatorNotBonded,
		ExceedActiveMaximum,
		ProblematicLedger,
		NotEnoughToUnbond,
		ExceedUnlockingRecords,
		RebondExceedUnlockingAmount,
		DecodingError,
		VectorEmpty,
		ValidatorSetNotExist,
		InvalidTimeUnit,
		AmountZero,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub (crate) fn deposit_event)]
	pub enum Event<T: Config> {
		DelegatorInitialized {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
		},
		DelegatorBonded {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			bonded_amount: BalanceOf<T>,
		},
		DelegatorBondExtra {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			extra_bonded_amount: BalanceOf<T>,
		},
		DelegatorUnbond {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			unbond_amount: BalanceOf<T>,
		},
		DelegatorRebond {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			rebond_amount: BalanceOf<T>,
		},
		Delegated {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			targets: Vec<MultiLocation>,
		},
		Undelegated {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			targets: Vec<MultiLocation>,
		},
		Payout {
			currency_id: CurrencyId,
			validator: MultiLocation,
			time_unit: Option<TimeUnit>,
		},
		Liquidize {
			currency_id: CurrencyId,
			delegator_id: MultiLocation,
			time_unit: Option<TimeUnit>,
		},
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

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			let delegator_id = staking_agent
				.initialize_delegator()
				.ok_or(Error::<T>::FailToInitializeDelegator)?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorInitialized { currency_id, delegator_id });
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

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.bond(who.clone(), amount)?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorBonded {
				currency_id,
				delegator_id: who,
				bonded_amount: amount,
			});
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

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.bond_extra(who.clone(), amount)?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorBondExtra {
				currency_id,
				delegator_id: who,
				extra_bonded_amount: amount,
			});
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

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.unbond(who.clone(), amount)?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorUnbond {
				currency_id,
				delegator_id: who,
				unbond_amount: amount,
			});
			Ok(())
		}

		/// Rebond some unlocking amount to a delegator.
		#[pallet::weight(T::WeightInfo::rebond())]
		pub fn rebond(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.rebond(who.clone(), amount)?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::DelegatorRebond {
				currency_id,
				delegator_id: who,
				rebond_amount: amount,
			});
			Ok(())
		}

		/// Delegate to some validator set.
		#[pallet::weight(T::WeightInfo::delegate())]
		pub fn delegate(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			targets: Vec<MultiLocation>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.delegate(who.clone(), targets.clone())?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::Delegated {
				currency_id,
				delegator_id: who,
				targets,
			});
			Ok(())
		}

		/// Re-delegate existing delegation to a new validator set.
		#[pallet::weight(T::WeightInfo::undelegate())]
		pub fn undelegate(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			targets: Vec<MultiLocation>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.undelegate(who.clone(), targets.clone())?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::Undelegated {
				currency_id,
				delegator_id: who,
				targets,
			});
			Ok(())
		}

		/// Re-delegate existing delegation to a new validator set.
		#[pallet::weight(T::WeightInfo::redelegate())]
		pub fn redelegate(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			targets: Vec<MultiLocation>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.redelegate(who.clone(), targets.clone())?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::Delegated {
				currency_id,
				delegator_id: who,
				targets,
			});
			Ok(())
		}

		/// Initiate payout for a certain delegator.
		#[pallet::weight(T::WeightInfo::payout())]
		pub fn payout(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			validator: MultiLocation,
			when: Option<TimeUnit>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.payout(who, validator.clone(), when.clone())?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::Payout { currency_id, validator, time_unit: when });
			Ok(())
		}

		/// Initiate payout for a certain delegator.
		#[pallet::weight(T::WeightInfo::liquidize())]
		pub fn liquidize(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			who: MultiLocation,
			when: Option<TimeUnit>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			let staking_agent = Self::get_currency_staking_agent(currency_id)?;
			staking_agent.liquidize(who.clone(), when.clone())?;

			// Deposit event.
			Pallet::<T>::deposit_event(Event::Liquidize {
				currency_id,
				delegator_id: who,
				time_unit: when,
			});
			Ok(())
		}

		#[pallet::weight(T::WeightInfo::increase_token_pool())]
		pub fn increase_token_pool(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			// Ensure the amount is valid.
			ensure!(amount > Zero::zero(), Error::<T>::AmountZero);

			T::VtokenMinting::increase_token_pool(currency_id, amount)?;
			Ok(())
		}

		#[pallet::weight(T::WeightInfo::decrease_token_pool())]
		pub fn decrease_token_pool(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			// Ensure the amount is valid.
			ensure!(amount > Zero::zero(), Error::<T>::AmountZero);

			T::VtokenMinting::decrease_token_pool(currency_id, amount)?;
			Ok(())
		}

		#[pallet::weight(T::WeightInfo::update_ongoing_time_unit())]
		pub fn update_ongoing_time_unit(
			origin: OriginFor<T>,
			currency_id: CurrencyId,
			time_unit: TimeUnit,
		) -> DispatchResult {
			// Ensure origin
			let authorized = Self::ensure_authorized(origin, currency_id);
			ensure!(authorized, Error::<T>::NotAuthorized);

			T::VtokenMinting::update_ongoing_time_unit(currency_id, time_unit)?;
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
			// need to be sorted and remove duplicates
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

		fn get_currency_staking_agent(currency_id: CurrencyId) -> Result<BoxType<T>, Error<T>> {
			match currency_id {
				KSM =>
					Ok(Box::new(
						KusamaAgent::<T, T::AccountConverter, T::ParachainId, T::XcmSender>::new(),
					)),
				_ => Err(Error::<T>::NotSupportedCurrencyId),
			}
		}
	}
}
