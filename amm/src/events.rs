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

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct SwapEvent {
    
}