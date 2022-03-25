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

use codec::{Decode, Encode};
use frame_support::RuntimeDebug;
use node_primitives::{CurrencyId, TimeUnit, TokenSymbol};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_std::vec::Vec;

/// Simplify the CurrencyId.
pub const KSM: CurrencyId = CurrencyId::Token(TokenSymbol::KSM);

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub enum Ledger<DelegatorId, Balance> {
	Substrate(SubstrateLedger<DelegatorId, Balance>),
}

#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct SubstrateLedger<DelegatorId, Balance> {
	/// The delegator account Id
	pub account: DelegatorId,
	/// The total amount of the delegator's balance that we are currently accounting for.
	/// It's just `active` plus all the `unlocking` balances.
	pub total: Balance,
	/// The total amount of the delegator's balance that will be at stake in any forthcoming
	/// rounds.
	pub active: Balance,
	/// Any balance that is becoming free, which may eventually be transferred out
	/// of the delegator (assuming it doesn't get slashed first).
	pub unlocking: Vec<UnlockChunk<Balance>>,
}

/// Just a Balance/BlockNumber tuple to encode when a chunk of funds will be unlocked.
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct UnlockChunk<Balance> {
	/// Amount of funds to be unlocked.
	pub value: Balance,
	/// Era number at which point it'll be unlocked.
	pub unlock_time: TimeUnit,
}

/// A type for accommodating delegator update entries for different kinds of currencies.
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub enum LedgerUpdateEntry<Balance, DelegatorId> {
	/// A type for substrate ledger updating entires
	Substrate(SubstrateLedgerUpdateEntry<Balance, DelegatorId>),
}

/// A type for substrate ledger updating entires
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct SubstrateLedgerUpdateEntry<Balance, DelegatorId> {
	/// The currency id of the delegator that needs to be update
	pub currency_id: CurrencyId,
	/// The delegator id that needs to be update
	pub delegator_id: DelegatorId,
	/// If this is true, the we need to remove the delegator ledger, if not, it is an adding or
	/// unlocking operation.
	pub if_remove: bool,
	/// If this is true, then this is an unlocking entry. If false, then it's a bonding entry.
	pub if_unlock: Option<bool>,
	/// The unlocking/bonding amount.
	pub amount: Option<Balance>,
	/// If this entry is an unlocking entry, it should have unlock_time value. If it is a bonding
	/// entry, this field should be None.
	pub unlock_time: Option<TimeUnit>,
}

/// A type for accommodating validators by delegator update entries for different kinds of
/// currencies.
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub enum ValidatorsByDelegatorUpdateEntry<DelegatorId, ValidatorId> {
	/// A type for substrate validators by delegator updating entires
	Substrate(SubstrateValidatorsByDelegatorUpdateEntry<DelegatorId, ValidatorId>),
}

/// A type for substrate validators by delegator updating entires
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct SubstrateValidatorsByDelegatorUpdateEntry<DelegatorId, ValidatorId> {
	/// The currency id of the delegator that needs to be update
	pub currency_id: CurrencyId,
	/// The delegator id that needs to be update
	pub delegator_id: DelegatorId,
	/// Validators vec to be updated
	pub validators: Vec<(ValidatorId, H256)>,
}

/// Different minimum and maximum requirements for different chain
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct MinimumsMaximums<Balance> {
	/// The minimum bonded amount for a delegator at any time.
	pub delegator_bonded_minimum: Balance,
	/// The minimum amount each time a delegator needs to bond for extra
	pub bond_extra_minimum: Balance,
	/// The minimum unbond amount each time a delegator to unbond.
	pub unbond_minimum: Balance,
	/// The minimum amount each time a delegator needs to rebond
	pub rebond_minimum: Balance,
	/// The maximum number of unbond records at the same time.
	pub unbond_record_maximum: u32,
	/// The maximum number of validators for a delegator to support at the same time.
	pub validators_back_maximum: u32,
	/// The maximum amount of active staking for a delegator. It is used to control ROI.
	pub delegator_active_staking_maximum: Balance,
}

/// XCM operations list
#[derive(Encode, Decode, Eq, PartialEq, Copy, Clone, RuntimeDebug, TypeInfo)]
pub enum XcmOperation {
	// XTokens
	XtokensTransfer,
	Bond,
	WithdrawUnbonded,
	BondExtra,
	Unbond,
	Rebond,
	Delegate,
	Payout,
	Liquidize,
	TransferBack,
	TransferTo,
	Chill,
}
