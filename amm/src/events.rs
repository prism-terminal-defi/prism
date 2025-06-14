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
use crate::structs::*;

#[derive(ScryptoSbor, ScryptoEvent, Debug)]
pub struct InstantiateAMMEvent {
    pub market_state: MarketState,
    pub market_fee: MarketFee,
}


#[derive(ScryptoSbor, ScryptoEvent, Clone, Debug)]
pub struct SwapEvent {
    pub swap_type: String,
    pub resource_sold: ResourceAddress,
    pub sell_size: Decimal,
    pub resource_bought: ResourceAddress,
    pub buy_size: Decimal,
    pub trade_volume: Decimal,
    pub exchange_rate_before_fees: PreciseDecimal,
    pub exchange_rate_after_fees: Decimal,
    pub reserve_fees: PreciseDecimal,
    pub trading_fees: PreciseDecimal,
    pub total_fees: PreciseDecimal,
    pub effective_implied_rate: Decimal,
    pub trade_implied_rate: PreciseDecimal,
    pub new_implied_rate: PreciseDecimal,
    pub output: Decimal,
    pub local_id: Option<NonFungibleLocalId>,
}

#[derive(ScryptoSbor, ScryptoEvent, Clone, Debug)]
pub struct MarketUpdate {
    timestamp: UtcDateTime,
}