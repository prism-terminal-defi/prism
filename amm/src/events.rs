use scrypto::prelude::*;
use crate::structs::*;

#[derive(ScryptoSbor, ScryptoEvent, Debug)]
pub struct InstantiateAMMEvent {
    pub market_state: MarketState,
    pub market_fee: MarketFee,
}


#[derive(ScryptoSbor, ScryptoEvent, Clone, Debug)]
pub struct SwapEvent {
    pub timestamp: UtcDateTime,
    pub resource_sold: ResourceAddress,
    pub sell_size: Decimal,
    pub resource_bought: ResourceAddress,
    pub buy_size: Decimal,
    pub trade_volume: Decimal,
    pub side: String,
    // price
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