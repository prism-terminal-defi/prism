use scrypto::prelude::*;

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketState {
    pub total_pt: Decimal,
    pub total_asset: Decimal,
    pub scalar_root: Decimal,
    /// The natural log of the implied rate of the last trade.
    pub last_ln_implied_rate: PreciseDecimal,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketInfo {
    /// The expiration date of the market. Once the market has expired,
    /// no more trades can be made.
    pub maturity_date: UtcDateTime,
    pub underlying_asset_address: ResourceAddress,
    pub pt_address: ResourceAddress,
    pub yt_address: ResourceAddress,
    pub pool_unit_address: ResourceAddress,

}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketFee {
    // The trading fee charged on each trade.
    pub fee_rate: PreciseDecimal,
    // The reserve fee rate.
    pub reserve_fee_percent: Decimal,
}

#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketFeeInput {
    // The trading fee charged on each trade.
    pub fee_rate: Decimal,
    // The reserve fee rate.
    pub reserve_fee_percent: Decimal,
}

/// Retrieves before-trade calculations for the 
/// exchange rate.
#[derive(ScryptoSbor, Clone, Debug)]
pub struct MarketCompute {
    pub rate_scalar: Decimal,
    pub rate_anchor: PreciseDecimal,
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

#[derive(ScryptoSbor, Clone, Debug)]
pub struct PoolStat {
    pub trading_fees_collected: PreciseDecimal,
    pub reserve_fees_collected: PreciseDecimal,
    pub total_fees_collected: PreciseDecimal,
}