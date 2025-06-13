use scrypto::prelude::*;
use crate::structs::*;

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct TokenizeEvent {
    pub amount_tokenized: Decimal,
    pub pt_amount_minted: Decimal,
    pub yt_update_or_mint: UpdateOrMint,
}

#[derive(ScryptoSbor, Debug, PartialEq, Eq)]
pub enum UpdateOrMint {
    Update(NonFungibleLocalId, YieldTokenData),
    Mint(NonFungibleLocalId, YieldTokenData),
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct RedeemEvent {
    pub asset_amount_owed: Decimal,
    pub pt_amount_burned: Decimal,
    pub yt_update_or_burn: UpdateOrBurn,
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct PTRedeemEvent {
    pub asset_amount_owed: Decimal,
    pub pt_amount_burned: Decimal,
}

#[derive(ScryptoSbor, Debug, PartialEq, Eq)]
pub enum UpdateOrBurn {
    Update(NonFungibleLocalId, YieldTokenData),
    Burn
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct MigrationEvent {
    pub migration_initiated: bool,
    pub migration_date: Option<UtcDateTime>,
    pub recipient: Option<ComponentAddress>,
    pub transaction_hash: Option<Hash>,
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct MigrationExecutedEvent {
    pub migration_date: UtcDateTime,
    pub recipient: ComponentAddress,
    pub transaction_hash: Hash,
}

#[derive(ScryptoSbor, ScryptoEvent, Debug, PartialEq, Eq)]
pub struct ClaimEvent{
    pub non_fungible_local_id: NonFungibleLocalId,
    pub yt_data: YieldTokenData,
    pub current_redemption_factor: Decimal,
    pub asset_amount_owed: Decimal,
}
