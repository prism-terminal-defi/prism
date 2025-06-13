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