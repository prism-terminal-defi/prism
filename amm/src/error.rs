use scrypto::prelude::*;

#[derive(Debug, ScryptoSbor)]
pub struct InsufficientLiquidityErrResponse {
    pub exact_asset_in: Decimal,
    pub total_asset: Decimal,
}

#[derive(Debug, ScryptoSbor)]
pub enum MarketError {
    InvalidExchangeRate(Decimal),
    InvalidPostFeeExchangeRate(Decimal),
    InvalidLastExchangeRate(Decimal),
    MaxMarketProportionReached(Decimal),
    ProportionGreaterThanOrEqualToOne(Decimal),
    ProportionLessThanZero(Decimal),
    InsufficientLiquidity(InsufficientLiquidityErrResponse),
    ArithmeticError(String),
    Other(String),
}

impl std::fmt::Display for MarketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketError::InvalidExchangeRate(rate) => 
                write!(f, "Exchange rate must be greater than 1. Exchange rate: {:?}", rate),
            MarketError::InvalidPostFeeExchangeRate(rate) => 
                write!(f, "Trade is unfavorable after fees are applied. Exchange rate: {:?}", rate),
            MarketError::InvalidLastExchangeRate(rate) => 
                write!(f, "Last exchange rate must be greater than 1. Exchange rate: {:?}", rate),
            MarketError::MaxMarketProportionReached(proportion) => 
                write!(f, "Trade is larger than the market's capacity. Proportion: {:?}", proportion),
            MarketError::ProportionGreaterThanOrEqualToOne(proportion) => 
                write!(f, "Trade is taking out more asset than is in the pool. Proportion: {:?}", proportion),
            MarketError::ProportionLessThanZero(proportion) => 
                write!(f, "Trade is taking out more PT than is in the pool. Proportion: {:?}", proportion),
            MarketError::InsufficientLiquidity(InsufficientLiquidityErrResponse { exact_asset_in, total_asset }) =>
                write!(
                    f, "The requested amount exceeds the available pool balance. Requested asset amount: {:?} | Pool asset balance: {:?}",
                    exact_asset_in, total_asset
                ),
            MarketError::ArithmeticError(msg) => 
                write!(f, "Arithmetic error: {}", msg),
            MarketError::Other(msg) => 
                write!(f, "{}", msg),
        }
    }
}