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
use bifrost_asset_registry::AssetMetadata;
use codec::Decode;
use cumulus_pallet_xcm::{ensure_sibling_para, Origin as CumulusOrigin};
use cumulus_primitives_core::ParaId;
use frame_support::{
	dispatch::DispatchResultWithPostInfo, sp_runtime::SaturatedConversion, traits::Get, PalletId,
};
use frame_system::Config as SystemConfig;
use node_primitives::{CurrencyId, CurrencyIdMapping, TryConvertFrom, VtokenMintingInterface};
use orml_traits::{arithmetic::Zero, MultiCurrency, XcmTransfer};
pub use pallet::*;
use scale_info::prelude::vec;
use sp_core::H160;
use sp_std::boxed::Box;
use xcm::{latest::prelude::*, v1::MultiLocation};

pub mod weights;
pub use weights::WeightInfo;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use zenlink_protocol::{AssetId, ExportZenlink};

	#[allow(type_alias_bounds)]
	pub type AccountIdOf<T> = <T as frame_system::Config>::AccountId;

	#[allow(type_alias_bounds)]
	pub type CurrencyIdOf<T> = <<T as Config>::MultiCurrency as MultiCurrency<
		<T as frame_system::Config>::AccountId,
	>>::CurrencyId;

	#[allow(type_alias_bounds)]
	pub type BalanceOf<T> =
		<<T as Config>::MultiCurrency as MultiCurrency<AccountIdOf<T>>>::Balance;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Origin: From<<Self as SystemConfig>::Origin>
			+ Into<Result<CumulusOrigin, <Self as Config>::Origin>>;
		type MultiCurrency: MultiCurrency<AccountIdOf<Self>, CurrencyId = CurrencyId>;

		type DexOperator: ExportZenlink<Self::AccountId>;

		/// The interface to call VtokenMinting module functions.
		type VtokenMintingInterface: VtokenMintingInterface<
			AccountIdOf<Self>,
			CurrencyIdOf<Self>,
			BalanceOf<Self>,
		>;

		/// xtokens xcm transfer interface
		type XcmTransfer: XcmTransfer<AccountIdOf<Self>, BalanceOf<Self>, CurrencyIdOf<Self>>;

		/// Convert MultiLocation to `T::CurrencyId`.
		type CurrencyIdConvert: CurrencyIdMapping<
			CurrencyId,
			MultiLocation,
			AssetMetadata<BalanceOf<Self>>,
		>;

		/// ModuleID for creating sub account
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		#[pallet::constant]
		type ParachainId: Get<ParaId>;

		type WeightInfo: WeightInfo;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		XcmMinted {
			para_id: ParaId,
			caller: H160,
			token_id: CurrencyIdOf<T>,
			token_amount: BalanceOf<T>,
		},
		XcmRedeemed {
			para_id: ParaId,
			caller: H160,
			vtoken_id: CurrencyIdOf<T>,
			vtoken_amount: BalanceOf<T>,
		},
		XcmSwapped {
			para_id: ParaId,
			caller: H160,
			amount_in_max: BalanceOf<T>,
			amount_out: BalanceOf<T>,
			in_currency_id: CurrencyIdOf<T>,
			out_currency_id: CurrencyIdOf<T>,
		},
		XcmClaimed {
			para_id: ParaId,
			caller: H160,
			token_id: CurrencyIdOf<T>,
			token_amount: BalanceOf<T>,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Token not found in vtoken minting
		TokenNotFoundInVtokenMinting,
		/// Token not found in zenlink
		TokenNotFoundInZenlink,
		/// Accountid decode error
		DecodingError,
		/// Multilocation to Curency id convert error
		CurrencyIdConvert,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// vtoken-minting mint
		#[pallet::weight(<T as Config>::WeightInfo::mint())]
		pub fn mint(
			origin: OriginFor<T>,
			caller: H160,
			token_id: Box<MultiLocation>,
			token_amount: BalanceOf<T>,
			weight: u64,
		) -> DispatchResultWithPostInfo {
			// Only accept calls from other chains
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin.clone()))?;
			ensure_signed(origin)?;
			let who = Self::generate_account_id(para, caller)?;
			let currency_id = T::CurrencyIdConvert::get_currency_id(*token_id)
				.ok_or(Error::<T>::TokenNotFoundInVtokenMinting)?;

			T::VtokenMintingInterface::mint(who.clone(), currency_id, token_amount)?;
			let vtoken_id = T::VtokenMintingInterface::vtoken_id(currency_id)
				.ok_or(Error::<T>::TokenNotFoundInVtokenMinting)?;
			// success
			let vtoken_balance = T::MultiCurrency::free_balance(vtoken_id, &who);
			if vtoken_balance != BalanceOf::<T>::zero() {
				T::XcmTransfer::transfer(
					who,
					vtoken_id,
					vtoken_balance,
					MultiLocation {
						parents: 1,
						interior: X2(
							Parachain(para.into()),
							Junction::AccountKey20 { network: Any, key: caller.to_fixed_bytes() },
						),
					},
					weight,
				)
				.ok();
			}

			Self::deposit_event(Event::XcmMinted {
				para_id: para.into(),
				caller,
				token_id: currency_id,
				token_amount,
			});

			Ok(().into())
		}

		/// vtoken-minting redeem
		#[pallet::weight(<T as Config>::WeightInfo::redeem())]
		pub fn redeem(
			origin: OriginFor<T>,
			caller: H160,
			vtoken_id: Box<MultiLocation>,
			vtoken_amount: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin.clone()))?;
			ensure_signed(origin)?;
			let who = Self::generate_account_id(para, caller)?;

			let currency_id = T::CurrencyIdConvert::get_currency_id(*vtoken_id)
				.ok_or(Error::<T>::TokenNotFoundInVtokenMinting)?;

			T::VtokenMintingInterface::redeem(who.clone(), currency_id, vtoken_amount)?;

			Self::deposit_event(Event::XcmRedeemed {
				para_id: para.into(),
				caller,
				vtoken_id: currency_id,
				vtoken_amount,
			});

			Ok(().into())
		}

		/// zenlink inner_swap_assets_for_exact_assets
		#[pallet::weight(<T as Config>::WeightInfo::swap())]
		pub fn swap(
			origin: OriginFor<T>,
			caller: H160,
			amount_in_max: BalanceOf<T>,
			amount_out: BalanceOf<T>,
			in_asset_id: Box<MultiLocation>,
			out_asset_id: Box<MultiLocation>,
			weight: u64,
		) -> DispatchResultWithPostInfo {
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin.clone()))?;
			ensure_signed(origin)?;
			let who = Self::generate_account_id(para, caller)?;

			let in_currency_id = T::CurrencyIdConvert::get_currency_id(*in_asset_id)
				.ok_or(Error::<T>::TokenNotFoundInVtokenMinting)?;
			let out_currency_id = T::CurrencyIdConvert::get_currency_id(*out_asset_id)
				.ok_or(Error::<T>::TokenNotFoundInVtokenMinting)?;

			let in_asset_id: AssetId =
				AssetId::try_convert_from(in_currency_id, T::ParachainId::get().into())
					.map_err(|_| Error::<T>::TokenNotFoundInZenlink)?;

			let out_asset_id: AssetId =
				AssetId::try_convert_from(out_currency_id, T::ParachainId::get().into())
					.map_err(|_| Error::<T>::TokenNotFoundInZenlink)?;

			let path = vec![in_asset_id, out_asset_id];
			T::DexOperator::inner_swap_assets_for_exact_assets(
				&who,
				amount_out.saturated_into(),
				amount_in_max.saturated_into(),
				&path,
				&who,
			)?;

			let out_balance = T::MultiCurrency::free_balance(out_currency_id, &who);
			if out_balance != BalanceOf::<T>::zero() {
				T::XcmTransfer::transfer(
					who,
					out_currency_id,
					out_balance,
					MultiLocation {
						parents: 1,
						interior: X2(
							Parachain(para.into()),
							Junction::AccountKey20 { network: Any, key: caller.to_fixed_bytes() },
						),
					},
					weight,
				)
				.ok();
			}

			Self::deposit_event(Event::XcmSwapped {
				para_id: para.into(),
				caller,
				amount_in_max,
				amount_out,
				in_currency_id,
				out_currency_id,
			});

			Ok(().into())
		}

		/// vtoken-minting rebond
		#[pallet::weight(<T as Config>::WeightInfo::claim())]
		pub fn claim(
			origin: OriginFor<T>,
			caller: H160,
			token_id: Box<MultiLocation>,
			token_amount: BalanceOf<T>,
			weight: u64,
		) -> DispatchResultWithPostInfo {
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin.clone()))?;
			ensure_signed(origin)?;
			let who = Self::generate_account_id(para, caller)?;

			let currency_id = T::CurrencyIdConvert::get_currency_id(*token_id)
				.ok_or(Error::<T>::TokenNotFoundInVtokenMinting)?;

			T::VtokenMintingInterface::rebond(who.clone(), currency_id, token_amount)?;

			let out_balance = T::MultiCurrency::free_balance(currency_id, &who);
			if out_balance != BalanceOf::<T>::zero() {
				T::XcmTransfer::transfer(
					who,
					currency_id,
					out_balance,
					MultiLocation {
						parents: 1,
						interior: X2(
							Parachain(para.into()),
							Junction::AccountKey20 { network: Any, key: caller.to_fixed_bytes() },
						),
					},
					weight,
				)
				.ok();
			}

			Self::deposit_event(Event::XcmClaimed {
				para_id: para.into(),
				caller,
				token_id: currency_id,
				token_amount,
			});

			Ok(().into())
		}
	}
}

impl<T: Config> Pallet<T> {
	pub fn generate_account_id(para: ParaId, address: H160) -> Result<AccountIdOf<T>, Error<T>> {
		let mut account_32 = [0u8; 32];
		account_32[0..20].copy_from_slice(&address[..]);
		let para_bytes = u32::from(para).to_be_bytes();
		account_32[20..24].copy_from_slice(&para_bytes);
		T::AccountId::decode(&mut &account_32[..]).map_err(|_| Error::<T>::DecodingError)
	}
}
