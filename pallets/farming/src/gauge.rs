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

use codec::{FullCodec, HasCompact};
use frame_support::pallet_prelude::*;
use node_primitives::{BlockNumber, CurrencyId};
use orml_traits::RewardHandler;
use scale_info::TypeInfo;
use sp_core::U256;
use sp_runtime::{
	traits::{
		AtLeast32BitUnsigned, MaybeSerializeDeserialize, Member, Saturating, UniqueSaturatedInto,
		Zero, *,
	},
	ArithmeticError, FixedPointOperand, Permill, RuntimeDebug, SaturatedConversion,
};
use sp_std::{borrow::ToOwned, collections::btree_map::BTreeMap, fmt::Debug, prelude::*};

use crate::*;

#[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo)]
pub struct GaugeInfo<BalanceOf: HasCompact, CurrencyIdOf: Ord, BlockNumberFor, AccountIdOf> {
	pub who: Option<AccountIdOf>,
	pub share: BalanceOf,
	pub share_total: BTreeMap<CurrencyIdOf, BalanceOf>,
	pub withdrawn_rewards: BTreeMap<CurrencyIdOf, BalanceOf>,
	pub gauge_amount: BalanceOf,
	pub gauge_time_factor: BalanceOf,
	pub last_time_factor: BalanceOf,
	pub gauge_start_block: BlockNumberFor,
	pub gauge_stop_block: BlockNumberFor,
	pub gauge_last_block: BlockNumberFor,
	pub last_claim_block: BlockNumberFor,
}

impl<BalanceOf, CurrencyIdOf, BlockNumberFor, AccountIdOf> Default
	for GaugeInfo<BalanceOf, CurrencyIdOf, BlockNumberFor, AccountIdOf>
where
	BalanceOf: Default + HasCompact,
	CurrencyIdOf: Ord,
	BlockNumberFor: Default,
{
	fn default() -> Self {
		Self {
			who: None,
			share: Default::default(),
			share_total: BTreeMap::new(),
			withdrawn_rewards: BTreeMap::new(),
			gauge_amount: Default::default(),
			gauge_time_factor: Default::default(),
			last_time_factor: Default::default(),
			gauge_start_block: Default::default(),
			gauge_stop_block: Default::default(),
			gauge_last_block: Default::default(),
			last_claim_block: Default::default(),
		}
	}
}

/// The Reward Pool Info.
#[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo)]
pub struct GaugePoolInfo<BalanceOf: HasCompact, CurrencyIdOf: Ord, BlockNumberFor> {
	pub pid: PoolId,
	pub token: CurrencyIdOf,
	pub gauge_amount: BalanceOf,
	pub gauge_time_factor: BalanceOf,
	pub gauge_state: GaugeState,
	pub gauge_start_block: BlockNumberFor,
	pub gauge_last_block: BlockNumberFor,
}

#[derive(Encode, Decode, Copy, Clone, Eq, PartialEq, RuntimeDebug, TypeInfo)]
pub enum GaugeState {
	Unbond,
	Bonded,
}

impl<BalanceOf, CurrencyIdOf, BlockNumberFor> Default
	for GaugePoolInfo<BalanceOf, CurrencyIdOf, BlockNumberFor>
where
	BalanceOf: Default + HasCompact,
	CurrencyIdOf: Ord + Default,
	BlockNumberFor: Default,
{
	fn default() -> Self {
		Self {
			pid: Default::default(),
			token: Default::default(),
			gauge_amount: Default::default(),
			gauge_time_factor: Default::default(),
			gauge_start_block: Default::default(),
			gauge_last_block: Default::default(),
			gauge_state: GaugeState::Unbond,
		}
	}
}

impl<BalanceOf, CurrencyIdOf, BlockNumberFor> GaugePoolInfo<BalanceOf, CurrencyIdOf, BlockNumberFor>
where
	BalanceOf: Default + HasCompact,
	CurrencyIdOf: Ord + Default,
	BlockNumberFor: Clone,
{
	pub fn new(pid: PoolId, token: CurrencyIdOf, current_block_number: BlockNumberFor) -> Self {
		Self {
			pid,
			token,
			gauge_amount: Default::default(),
			gauge_time_factor: Default::default(),
			gauge_start_block: current_block_number.clone(),
			gauge_last_block: current_block_number,
			gauge_state: GaugeState::Bonded,
		}
	}
}

impl<T: Config> Pallet<T>
where
	BlockNumberFor<T>: Into<BalanceOf<T>>,
	/* BlockNumberFor<T>: Into<u128>,
	 * BalanceOf<T>: From<BlockNumberFor<T>>,
	 * BalanceOf<T>: From<<T as frame_system::Config>::BlockNumber>, */
{
	#[transactional]
	pub fn create_gauge_pool(
		pid: PoolId,
		pool_info: &mut PoolInfo<BalanceOf<T>, CurrencyIdOf<T>, AccountIdOf<T>, BlockNumberFor<T>>,
		gauge_token: CurrencyIdOf<T>,
	) -> DispatchResult {
		let gid = Self::gauge_pool_next_id();
		pool_info.gauge = Some(gid);
		let current_block_number = frame_system::Pallet::<T>::block_number();
		let mut gauge_pool_info = GaugePoolInfo::new(pid, gauge_token, current_block_number);

		GaugePoolInfos::<T>::insert(gid, &gauge_pool_info);
		GaugePoolNextId::<T>::mutate(|id| -> DispatchResult {
			*id = id.checked_add(1).ok_or(ArithmeticError::Overflow)?;
			Ok(())
		})?;
		Ok(())
	}

	#[transactional]
	pub fn gauge_add(
		who: &AccountIdOf<T>,
		pid: PoolId,
		gid: PoolId,
		gauge_value: BalanceOf<T>,
		gauge_block: BlockNumberFor<T>,
	) -> DispatchResult {
		let current_block_number = frame_system::Pallet::<T>::block_number();
		GaugePoolInfos::<T>::mutate(gid, |gauge_pool_info| -> DispatchResult {
			let interval_block = current_block_number - gauge_pool_info.gauge_last_block;
			let total_time_factor = interval_block
				.into()
				.checked_mul(&gauge_pool_info.gauge_amount)
				.ok_or(ArithmeticError::Overflow)?;

			gauge_pool_info.gauge_last_block = current_block_number;
			gauge_pool_info.gauge_time_factor = gauge_pool_info
				.gauge_time_factor
				.checked_add(&total_time_factor)
				.ok_or(ArithmeticError::Overflow)?;
			gauge_pool_info.gauge_amount = gauge_pool_info
				.gauge_amount
				.checked_add(&gauge_value)
				.ok_or(ArithmeticError::Overflow)?;
			GaugeInfos::<T>::mutate(gid, who, |gauge_info| -> DispatchResult {
				if gauge_info.gauge_start_block.is_zero() {
					gauge_info.gauge_start_block = current_block_number;
					gauge_info.last_claim_block = current_block_number;
					gauge_info.gauge_stop_block = current_block_number + gauge_block;
					// return Ok(());
				}
				let user_interval_block = current_block_number - gauge_info.gauge_last_block;
				let time_factor = user_interval_block
					.into()
					.checked_mul(&gauge_info.gauge_amount)
					.ok_or(ArithmeticError::Overflow)?;

				gauge_info.gauge_last_block = current_block_number;
				gauge_info.gauge_time_factor = gauge_info
					.gauge_time_factor
					.checked_add(&time_factor)
					.ok_or(ArithmeticError::Overflow)?;
				gauge_info.gauge_amount = gauge_info
					.gauge_amount
					.checked_add(&gauge_value)
					.ok_or(ArithmeticError::Overflow)?;
				Ok(())
			})?;
			// let pool_info = Self::pool_infos(&pid);
			// if let Some(ref keeper) = pool_info.keeper {
			// 	T::MultiCurrency::transfer(gauge_info.token, who, &keeper, gauge_value)?;
			// } else {
			// }
			let pool_info = Self::pool_infos(&pid);
			T::MultiCurrency::transfer(
				gauge_pool_info.token,
				who,
				&pool_info.keeper.ok_or(Error::<T>::KeeperNotExist)?,
				gauge_value,
			)?;
			Ok(())
		})?;
		Ok(())
	}

	pub fn gauge_claim(
		who: &AccountIdOf<T>,
		gid: PoolId,
		gauge_amount: BalanceOf<T>,
		gauge_last_block: BlockNumberFor<T>,
	) -> DispatchResult {
		let current_block_number: BlockNumberFor<T> = frame_system::Pallet::<T>::block_number();
		let gauge_pool_info = GaugePoolInfos::<T>::get(gid);
		let pool_info = PoolInfos::<T>::get(gauge_pool_info.pid);
		GaugeInfos::<T>::mutate(gid, who, |gauge_info| -> DispatchResult {
			let gauge_rate = Permill::from_rational(
				gauge_info.gauge_time_factor,
				gauge_pool_info.gauge_time_factor,
			);
			let interval_block_rate =
				gauge_rate * (current_block_number - gauge_info.last_claim_block);

			pool_info.basic_rewards.clone().iter().try_for_each(
				|(reward_currency, reward_amount)| -> DispatchResult {
					// let reward_to_claim = gauge_rate * interval_block * reward_amount;
					let reward_to_claim = reward_amount
						.checked_mul(&interval_block_rate.into())
						.ok_or(ArithmeticError::Overflow)?;
					if let Some(ref keeper) = pool_info.keeper {
						T::MultiCurrency::transfer(
							*reward_currency,
							&keeper,
							&who,
							reward_to_claim,
						)?;
					}
					Ok(())
				},
			);
			gauge_info.last_claim_block = current_block_number;

			Ok(())
		})?;
		// GaugePoolInfos::<T>::mutate(gauge, |gauge_info| {});
		Ok(())
	}

	pub fn gauge_remove(
		who: &AccountIdOf<T>,
		pool: PoolId,
		gauge_value: BalanceOf<T>,
		gauge: PoolId,
	) -> DispatchResult {
		let current_block_number = frame_system::Pallet::<T>::block_number();

		Ok(())
	}

	pub fn gauge_cal_rewards(
		gauge_amount: BalanceOf<T>,
		gauge_last_block: BlockNumberFor<T>,
	) -> DispatchResult {
		// SharesAndWithdrawnRewards::<T>::mutate(pool, who, |share_info| {});
		// GaugePoolInfos::<T>::mutate(gauge, |gauge_info| {});
		Ok(())
	}
}
