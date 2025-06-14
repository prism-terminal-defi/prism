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

#[derive(ScryptoSbor)]
pub enum RedemptionStrategy {
    FullRedemption,
    PartialRedemption,
    ExpiredMarket,
}

#[derive(ScryptoSbor, Copy, Clone)]
pub struct MigrationState {
    pub migration_initiated: bool,
    pub migration_date: Option<UtcDateTime>,
    pub recipient: Option<ComponentAddress>,
    pub transaction_hash: Option<Hash>,
}