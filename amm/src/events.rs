use scrypto::prelude::*;
use common::structs::*;

#[derive(ScryptoSbor, ScryptoEvent, Debug)]
pub struct InstantiateAMMEvent {
    pub market_state: MarketState,
    pub market_fee: MarketFee,
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct AddLiquidityEvent {
    
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct RemoveLiquidityEvent {
    
}

#[derive(ScryptoSbor, ScryptoEvent, Clone, Debug)]
pub struct SwapEvent {
    pub timestamp: UtcDateTime,
    pub market_pair: (ResourceAddress, ResourceAddress),
    pub size: Decimal,
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
}

#[derive(ScryptoSbor, ScryptoEvent, Clone, Debug)]
pub struct MarketUpdate {
    timestamp: UtcDateTime,
}