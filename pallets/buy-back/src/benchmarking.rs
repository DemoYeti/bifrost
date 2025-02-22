// This file is part of Bifrost.

// Copyright (C) Liebi Technologies PTE. LTD.
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

// Ensure we're `no_std` when compiling for Wasm.

#![cfg(feature = "runtime-benchmarks")]

use crate::{BalanceOf, Call, Config, Pallet, Pallet as BuyBack, *};
use bifrost_primitives::{CurrencyId, TokenSymbol, DOT};
use frame_benchmarking::v1::{account, benchmarks, BenchmarkError};
use frame_support::{
	assert_ok,
	traits::{EnsureOrigin, Hooks},
};
use frame_system::RawOrigin;
use orml_traits::MultiCurrency;
use sp_runtime::traits::UniqueSaturatedFrom;

benchmarks! {
	set_vtoken {
		let origin = T::ControlOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
	}: _<T::RuntimeOrigin>(origin,CurrencyId::VToken(TokenSymbol::KSM),1_000_000u32.into(),Permill::from_percent(2),1000u32.into(),1000u32.into(),true)

	charge {
		let test_account: T::AccountId = account("seed",1,1);

		T::MultiCurrency::deposit(DOT, &test_account, BalanceOf::<T>::unique_saturated_from(1_000_000_000_000_000u128))?;
	}: _(RawOrigin::Signed(test_account),DOT,BalanceOf::<T>::unique_saturated_from(9_000_000_000_000u128))

	remove_vtoken {
		let origin = T::ControlOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
		assert_ok!(BuyBack::<T>::set_vtoken(
			T::ControlOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
			CurrencyId::VToken(TokenSymbol::KSM),
			1_000_000u32.into(),
			Permill::from_percent(2),
			1000u32.into(),
			1000u32.into(),
			true
		));
	}: _<T::RuntimeOrigin>(origin,CurrencyId::Token(TokenSymbol::KSM))


	on_idle {
		let origin = T::ControlOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
		assert_ok!(BuyBack::<T>::set_vtoken(
			T::ControlOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
			CurrencyId::VToken(TokenSymbol::KSM),
			1_000_000u32.into(),
			Permill::from_percent(2),
			1000u32.into(),
			1000u32.into(),
			true
		));
	}: {
		BuyBack::<T>::on_idle(BlockNumberFor::<T>::from(0u32),Weight::from_parts(0, u64::MAX));
	}

	impl_benchmark_test_suite!(BuyBack,crate::mock::ExtBuilder::default().build(),crate::mock::Runtime);
}
