// Copyright 2025 PrismTerminal
// 
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use scrypto::prelude::*;

#[derive(ScryptoSbor, Copy, Clone, Debug)]
pub struct MarketState {
    pub initial_rate_anchor: PreciseDecimal,
    pub scalar_root: Decimal,
    pub last_ln_implied_rate: PreciseDecimal,
}

#[derive(ScryptoSbor, Copy, Clone, Debug)]
pub struct MarketInfo {
    pub maturity_date: UtcDateTime,
    pub underlying_asset_address: ResourceAddress,
    pub pt_address: ResourceAddress,
    pub yt_address: ResourceAddress,
    pub pool_unit_address: ResourceAddress,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketFee {
    pub ln_fee_rate: PreciseDecimal,
    pub reserve_fee_percent: Decimal,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketFeeInput {
    pub fee_rate: Decimal,
    pub reserve_fee_percent: Decimal,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketCompute {
    pub rate_scalar: Decimal,
    pub rate_anchor: PreciseDecimal,
    pub redemption_factor: Decimal,
    pub total_pt_amount: Decimal,
    pub total_base_asset_amount: Decimal,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct PoolVaultReserves {
    pub total_pt_amount: Decimal,
    pub total_underlying_asset_amount: Decimal,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct ResourceInformation {
    pub amount: Decimal,
    pub divisibility: i64,
}

#[derive(ScryptoSbor, Copy, Clone, Debug)]
pub struct PoolStat {
    pub trading_fees_collected: PreciseDecimal,
    pub reserve_fees_collected: PreciseDecimal,
    pub total_fees_collected: PreciseDecimal,
}

#[derive(ScryptoSbor, NonFungibleData, Debug, PartialEq, Eq)]
pub struct YieldTokenData {
    pub underlying_asset_address: ResourceAddress,
    #[mutable]
    pub last_claim_redemption_factor: Decimal,
    #[mutable]
    pub yt_amount: Decimal,
    #[mutable]
    pub yield_claimed: Decimal,
    #[mutable]
    pub accrued_yield: Decimal,
    pub maturity_date: UtcDateTime,
}