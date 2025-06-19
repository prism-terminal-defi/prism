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
use ports_interface::prelude::*;
use scrypto_interface::*;
use crate::structs::*;
use crate::events::*;
use crate::pool_adapter_impl::{OneResourcePoolWrapper, ValidatorWrapper};

type PoolAdapter = PoolAdapterInterfaceScryptoStub;

#[blueprint_with_traits]
#[types(
    AssetPool,
    PoolType,
    ValidatorWrapper,
    OneResourcePoolWrapper,
    YieldTokenData,
    RedemptionStrategy,
    MigrationState,
)]
#[events(
    TokenizeEvent, 
    RedeemEvent,
    PTRedeemEvent,
    MigrationEvent,
    MigrationExecutedEvent,
    ClaimEvent,
)]
mod prism_splitter {

    const OWNER_BADGE_RM: ResourceManager = 
        resource_manager!("resource_rdx1tk4zl8p0wzh0g3f39adzv37xg7jmgm0th7q6ud78wv48nffzlsvrch");

    enable_function_auth! {
        instantiate_prism_splitter => rule!(require(OWNER_BADGE_RM.address()));
        instantiate_prism_splitter_with_existing => rule!(require(OWNER_BADGE_RM.address()));
    }

    enable_method_auth! {
        methods {
            // Public methods
            tokenize => PUBLIC;
            redeem => PUBLIC;
            redeem_from_pt => PUBLIC;
            claim_yield => PUBLIC;
            merge_multiple_yt => PUBLIC;
            calc_yield_owed_pub => PUBLIC;
            calc_yield_owed_in_underlying => PUBLIC;
            get_pt_redemption_value => PUBLIC;
            get_underlying_asset_redemption_value => PUBLIC;
            get_underlying_asset_redemption_factor => PUBLIC;
            calc_asset_owed_amount => PUBLIC;
            pt_address => PUBLIC;
            yt_address => PUBLIC;
            underlying_asset => PUBLIC;
            protocol_resources => PUBLIC;
            maturity_date => PUBLIC;
            get_migration_state => PUBLIC;
            get_prism_splitter_is_active => PUBLIC;
            get_late_fee => PUBLIC;
            // Admin methods
            change_redemption_factor => restrict_to: [OWNER];
            change_adapter => restrict_to: [OWNER];
            change_maturity_date => restrict_to: [OWNER];
            initiate_migration => restrict_to: [OWNER];
            cancel_migration => restrict_to: [OWNER];
            migrate_funds_to_new_prism_splitter => restrict_to: [OWNER];
            set_prism_splitter_is_active => restrict_to: [OWNER];
            update_redemption_factor => restrict_to: [SELF, OWNER];
            deposit_to_asset_vault => restrict_to: [SELF, OWNER];
            update_protocol_resource_roles => restrict_to: [OWNER];
            update_protocol_rm => restrict_to: [OWNER];
            change_late_fee => restrict_to: [OWNER];
        }
    }
    struct PrismSplitterV2  {
        pt_rm: FungibleResourceManager,
        yt_rm: NonFungibleResourceManager,
        maturity_date: UtcDateTime,
        underlying_asset_pool: AssetPool,
        redemption_factor: Decimal,
        locked_redemption_factor: bool,
        last_redemption_factor_updated: UtcDateTime,
        asset_vault: FungibleVault,
        fee_vault: FungibleVault,
        late_fee: Decimal,
        migration_state: MigrationState,
        prism_splitter_is_active: bool,
    }

    impl PrismSplitterV2 {
        pub fn instantiate_prism_splitter(
            owner_role_node: CompositeRequirement,
            maturity_date: UtcDateTime,
            underlying_asset: ResourceAddress,
            late_fee: Decimal,
            pool_type: PoolType,
            dapp_definition: ComponentAddress,
            address_reservation: Option<GlobalAddressReservation>,
        ) -> Global<PrismSplitterV2> {
                
            let underlying_asset_rm = 
                ResourceManager::from(underlying_asset);

            assert_eq!(
                underlying_asset_rm.resource_type().is_fungible(), 
                true, 
                "Not a fungible asset!"
            );

            let owner_role = 
                OwnerRole::Updatable(
                    AccessRule::from(
                        owner_role_node.clone()
                    )
                );

            let (address_reservation, component_address) =
                if address_reservation.is_some() {
                    let address_reservation = address_reservation.unwrap();
                    let component_address = 
                        ComponentAddress::try_from(
                            Runtime::get_reservation_address(&address_reservation))
                        .ok()
                        .unwrap();

                    (address_reservation, component_address)
                } else { 
                    Runtime::allocate_component_address(PrismSplitterV2::blueprint_id())
                };
            
            let underlying_asset_pool = match pool_type {
                PoolType::Validator => {
                    assert!(
                        Self::is_valid_lsu(underlying_asset), 
                        "Not a valid LSU"
                    );
                    AssetPool::Validator(
                        ValidatorWrapper(Self::retrieve_validator_component(underlying_asset))
                    )
                },
                PoolType::LiquidityPool => {
                    assert!(
                        Self::is_valid_native_pool(underlying_asset), 
                        "Not a valid native pool"
                    );
                    let pool_component: Global<OneResourcePool> = 
                        get_pool_component_address(underlying_asset).into();
                    AssetPool::LiquidityPool(OneResourcePoolWrapper(pool_component))
                },
                PoolType::CustomPool(pool_adapter) => {
                    AssetPool::CustomPool(pool_adapter.into())
                }
            };

            let underlying_asset_pool_address = match &underlying_asset_pool {
                AssetPool::Validator(validator) => validator.pool_address(),
                AssetPool::LiquidityPool(pool) => pool.pool_address(),
                AssetPool::CustomPool(pool) => pool.pool_address(),
            };

            let redemption_factor = 
                underlying_asset_pool.get_underlying_asset_redemption_factor();

            let (market_name, market_symbol, market_icon) = 
                if Self::is_valid_lsu(underlying_asset) {
                    let validator: Global<Validator> = underlying_asset_pool_address.into();
                    let validator_name: String = 
                        validator
                        .get_metadata("name")
                        .unwrap_or(Some("".to_string()))
                        .unwrap_or("".to_string());
                    let validator_symbol = validator_name.clone();
                    let validator_icon_url: Url = 
                        validator
                        .get_metadata("icon_url")
                        .unwrap_or(Some(UncheckedUrl::of("https://www.prismterminal.com/assets/glowlogo.svg")))
                        .unwrap_or(UncheckedUrl::of("https://www.prismterminal.com/assets/glowlogo.svg"));

                    (validator_name, validator_symbol, validator_icon_url)
                } else {
                    let (name, symbol, icon) = 
                       retrieve_metadata(underlying_asset_rm);
                       
                    (name, symbol, icon)
                };
            
            let pt_rm: FungibleResourceManager = ResourceBuilder::new_fungible(owner_role.clone())
                .divisibility(
                    underlying_asset_rm
                    .resource_type()
                    .divisibility()
                    .unwrap_or(18)
                )
                .metadata(metadata! {
                    init {
                        "name" => format!("{} (Principal Token)", market_name), locked;
                        "symbol" => format!("pt{}", market_symbol), locked;
                        "icon_url" => market_icon.clone(), locked;
                        "description" => 
                            "The Principal Token representation of the underlying asset. This asset gives the holder 
                            the right to redeem the underlying asset at maturity.", 
                            locked;
                        "prism_splitter_component" => GlobalAddress::from(component_address), locked;
                        "underlying_lsu_validator" => GlobalAddress::from(underlying_asset_pool_address), locked;
                        "underlying_asset_address" => GlobalAddress::from(underlying_asset), locked;
                        "maturity_date" => maturity_date.to_string(), locked;
                        "dapp_definition" => GlobalAddress::from(dapp_definition), updatable;
                    }
                })
                .mint_roles(mint_roles! {
                    minter => rule!(require(global_caller(component_address)));
                    minter_updater => 
                        rule!(require(global_caller(component_address)) || require(owner_role_node.clone()));
                })
                .burn_roles(burn_roles! {
                    burner => rule!(require(global_caller(component_address)));
                    burner_updater => 
                        rule!(require(global_caller(component_address)) || require(owner_role_node.clone()));
                })
                .create_with_no_initial_supply();

            let yt_rm: NonFungibleResourceManager = 
                ResourceBuilder::new_ruid_non_fungible::<YieldTokenData>(owner_role.clone())
                .metadata(metadata! {
                    init {
                        "name" => format!("{} (Yield Token)", market_name), locked;
                        "symbol" => format!("yt{}", market_symbol), locked;
                        "icon_url" => market_icon.clone(), locked;
                        "description" => "The Yield Token representation of the underlying asset. 
                            This asset gives the right to the holder to claim the yield earned by the underlying asset.", locked;
                        "prism_splitter_component" => GlobalAddress::from(component_address), locked;
                        "underlying_lsu_validator" => GlobalAddress::from(underlying_asset_pool_address), locked;
                        "underlying_asset_address" => GlobalAddress::from(underlying_asset), locked;
                        "maturity_date" => maturity_date.to_string(), locked;
                        "dapp_definition" => GlobalAddress::from(dapp_definition), updatable;
                    }
                })
                .mint_roles(mint_roles! {
                    minter => rule!(require(global_caller(component_address)));
                    minter_updater => 
                        rule!(require(global_caller(component_address)) || require(owner_role_node.clone()));
                })
                .burn_roles(burn_roles! {
                    burner => rule!(allow_all);
                    burner_updater => 
                        rule!(require(global_caller(component_address)) || require(owner_role_node.clone()));
                })
                .non_fungible_data_update_roles(non_fungible_data_update_roles! {
                    non_fungible_data_updater => rule!(require(global_caller(component_address)));
                    non_fungible_data_updater_updater => 
                        rule!(require(global_caller(component_address)) || require(owner_role_node.clone()));
                })
                .create_with_no_initial_supply();

            let current_time = 
                UtcDateTime::from_instant(
                    &Clock::current_time_rounded_to_seconds()
                ).unwrap();

            let migration_state = MigrationState {
                migration_initiated: false,
                migration_date: None,
                recipient: None,
                transaction_hash: None,
            };
            
            Self {
                pt_rm,
                yt_rm,
                maturity_date,
                underlying_asset_pool,
                redemption_factor,
                locked_redemption_factor: false,
                last_redemption_factor_updated: current_time,
                asset_vault: FungibleVault::new(underlying_asset),
                fee_vault: FungibleVault::new(underlying_asset),
                late_fee,
                migration_state,
                prism_splitter_is_active: true,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .metadata(Self::set_up_metadata_config(
                market_name, 
                market_symbol, 
                market_icon, 
                pt_rm, 
                yt_rm, 
                underlying_asset, 
                maturity_date, 
                dapp_definition
            ))
            .enable_component_royalties(
                Self::set_up_component_royalties()
            )
            .globalize()
        }

        pub fn instantiate_prism_splitter_with_existing(
            owner_role_rule: AccessRule,
            maturity_date: UtcDateTime,
            underlying_asset: ResourceAddress,
            pt_resource_address: ResourceAddress,
            yt_resource_address: ResourceAddress,
            late_fee: Decimal,
            pool_type: PoolType,
            dapp_definition: ComponentAddress,
            address_reservation: Option<GlobalAddressReservation>,
        ) -> Global<PrismSplitterV2> {

            let (address_reservation, _) =
                if address_reservation.is_some() {
                    let address_reservation = address_reservation.unwrap();
                    let component_address = 
                        ComponentAddress::try_from(
                            Runtime::get_reservation_address(&address_reservation))
                        .ok()
                        .unwrap();

                    (address_reservation, component_address)
                } else { 
                    Runtime::allocate_component_address(PrismSplitterV2::blueprint_id())
                };

            let underlying_asset_rm = 
                ResourceManager::from_address(underlying_asset);

            let pt_rm = FungibleResourceManager::from(pt_resource_address);
            let yt_rm = NonFungibleResourceManager::from(yt_resource_address);

            assert_eq!(
                underlying_asset_rm.resource_type().is_fungible(), 
                true, 
                "Not a fungible asset!"
            );

            let (market_name, market_symbol, market_icon) = 
                retrieve_metadata(underlying_asset_rm.into());

            let owner_role = OwnerRole::Updatable(owner_role_rule);

            let underlying_asset_pool = match pool_type {
                PoolType::Validator => {
                    assert!(
                        Self::is_valid_lsu(underlying_asset), 
                        "Not a valid LSU"
                    );
                    AssetPool::Validator(
                        ValidatorWrapper(Self::retrieve_validator_component(underlying_asset))
                    )
                },
                PoolType::LiquidityPool => {
                    assert!(
                        Self::is_valid_native_pool(underlying_asset), 
                        "Not a valid native pool"
                    );
                    let pool_component: Global<OneResourcePool> = 
                        get_pool_component_address(underlying_asset).into();
                    AssetPool::LiquidityPool(OneResourcePoolWrapper(pool_component))
                },
                PoolType::CustomPool(pool_adapter) => {
                    AssetPool::CustomPool(pool_adapter.into())
                }
            };

            let redemption_factor = 
                underlying_asset_pool.total_stake_amount()
                .checked_div(underlying_asset_pool.total_stake_unit_supply())
                .expect("Overflow");

            let current_time = 
                UtcDateTime::from_instant(
                    &Clock::current_time_rounded_to_seconds()
                ).unwrap();

            let migration_state = MigrationState {
                migration_initiated: false,
                migration_date: None,
                recipient: None,
                transaction_hash: None,
            };
            
            Self {
                pt_rm,
                yt_rm,
                maturity_date,
                underlying_asset_pool,
                redemption_factor,
                locked_redemption_factor: false,
                last_redemption_factor_updated: current_time,
                asset_vault: FungibleVault::new(underlying_asset),
                fee_vault: FungibleVault::new(underlying_asset),
                late_fee,
                migration_state,
                prism_splitter_is_active: true,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .metadata(Self::set_up_metadata_config(
                market_name, 
                market_symbol, 
                market_icon, 
                pt_rm, 
                yt_rm, 
                underlying_asset, 
                maturity_date, 
                dapp_definition
            ))
            .enable_component_royalties(
                Self::set_up_component_royalties()
            )
            .globalize()
        }

        fn set_up_component_royalties() -> (Methods<(RoyaltyAmount, bool)>, RoleAssignmentInit) {
            let royalties: (Methods<(RoyaltyAmount, bool)>, RoleAssignmentInit) = 
                component_royalties! (
                        roles {
                            royalty_setter => OWNER;
                            royalty_setter_updater => OWNER;
                            royalty_locker => OWNER;
                            royalty_locker_updater => OWNER;
                            royalty_claimer => OWNER;
                            royalty_claimer_updater => OWNER;
                        },
                        init {
                            tokenize => Free, updatable;
                            redeem => Free, updatable;
                            redeem_from_pt => Free, updatable;
                            claim_yield => Free, updatable;
                            merge_multiple_yt => Free, updatable;
                            update_redemption_factor => Free, updatable;
                            calc_yield_owed_pub => Free, updatable;
                            calc_yield_owed_in_underlying => Free, updatable;
                            get_pt_redemption_value => Free, updatable;
                            get_underlying_asset_redemption_value => Free, updatable;
                            get_underlying_asset_redemption_factor => Free, updatable;
                            calc_asset_owed_amount => Free, updatable;
                            pt_address => Free, updatable;
                            yt_address => Free, updatable;
                            underlying_asset => Free, updatable;
                            protocol_resources => Free, updatable;
                            maturity_date => Free, updatable;
                            change_redemption_factor => Free, updatable;
                            change_adapter => Free, updatable;
                            change_maturity_date => Free, updatable;
                            initiate_migration => Free, updatable;
                            cancel_migration => Free, updatable;
                            get_migration_state => Free, updatable;
                            get_prism_splitter_is_active => Free, updatable;
                            get_late_fee => Free, updatable;
                            set_prism_splitter_is_active => Free, updatable;
                            migrate_funds_to_new_prism_splitter => Free, updatable;
                            deposit_to_asset_vault => Free, updatable;
                            update_protocol_resource_roles => Free, updatable;
                            update_protocol_rm => Free, updatable;
                            change_late_fee => Free, updatable;
                        } 
                    );
            return royalties
        }

        fn set_up_metadata_config(
            market_name: String,
            market_symbol: String,
            market_icon: UncheckedUrl,
            pt_rm: FungibleResourceManager,
            yt_rm: NonFungibleResourceManager,
            underlying_asset: ResourceAddress,
            maturity_date: UtcDateTime,
            dapp_definition: ComponentAddress,
        ) -> ModuleConfig<KeyValueStoreInit<String, GenericMetadataValue<UncheckedUrl, UncheckedOrigin>>> {
            let metadata = metadata! {
                roles {
                    metadata_locker => OWNER;
                    metadata_locker_updater => OWNER;
                    metadata_setter => OWNER;
                    metadata_setter_updater => OWNER;
                },
                init {
                    "market_name" => market_name, locked;
                    "market_symbol" => market_symbol, locked;
                    "market_icon" => market_icon, locked;
                    "protocol_resources" => vec![
                        pt_rm.address(),
                        yt_rm.address()
                    ], locked;
                    "underlying_asset" => underlying_asset, locked;
                    "maturity_date" => maturity_date.to_string(), updatable;
                    "dapp_definition" => GlobalAddress::from(dapp_definition), updatable;
                }
            };
            return metadata
        }

        fn retrieve_validator_component(
            asset_address: ResourceAddress
        ) -> Global<Validator> {
            let metadata: GlobalAddress = 
                ResourceManager::from(asset_address)
                .get_metadata("validator")
                .unwrap()
                .unwrap_or_else(||
                    Runtime::panic(String::from("Not an Asset!"))
                );
            ComponentAddress::try_from(metadata)
                .unwrap()
                .into()
        }

        fn is_valid_lsu(input_asset_address: ResourceAddress) -> bool {
            // Step 1: Check if "validator" metadata exists with explicit type annotations
            let validator_address_result = 
                match ResourceManager::from(input_asset_address)
                    .get_metadata::<&str, GlobalAddress>("validator") {  
                        Ok(Some(metadata)) => Some(metadata),  // "validator" field exists, return the address
                        Ok(None) => None,  // "validator" field does not exist
                        Err(_) => None,    // Error in fetching or converting metadata
                    };
        
            if let Some(validator_address) = validator_address_result {
                // Step 2: If "validator" exists, check "pool_unit" metadata for the validator address
                let validator: Global<Validator> = 
                    match ComponentAddress::try_from(validator_address) {
                        Ok(address) => Global::from(address),
                        Err(_) => {
                            Runtime::panic(String::from("Invalid validator address"))
                        }
                    };
        
                let pool_unit_metadata = 
                    validator.get_metadata::<&str, GlobalAddress>("pool_unit");
        
                match pool_unit_metadata {
                    Ok(Some(pool_unit_address)) => {
                        // Step 3: Ensure the pool_unit address matches the input_asset_address
                        if ResourceAddress::try_from(pool_unit_address).ok().unwrap() == input_asset_address {
                            return true;  // Success, the addresses match
                        } else {
                            return false;  // The addresses do not match
                        }
                    },
                    Ok(None) => {
                        // "pool_unit" metadata is missing, return false (invalid Asset)
                        return false;
                    },
                    Err(_) => {
                        // Error in fetching "pool_unit" metadata, return false
                        Runtime::panic(String::from("Error retrieving pool_unit metadata"))
                    }
                }
            }
            // Step 4: If no "validator" field or invalid conversion, return false
            false
        }

        fn is_valid_native_pool(input_asset_address: ResourceAddress) -> bool {
            let pool_address = 
                get_pool_component_address(input_asset_address);

            let blueprint_id = BlueprintId::new(
                &POOL_PACKAGE,
                "OneResourcePool".to_string()
            );

            ScryptoVmV1Api::object_instance_of(pool_address.as_node_id(), &blueprint_id)
        }

        /// Calculates the yield owed for a given YT.
        /// 
        /// # Mechanics
        /// 1. Matches 2 cases: if the caller provided an existing YT or not.
        ///
        /// Scenario 1: If the caller provided an existing YT, update YT
        /// * Checks whether YT is the same as PrismSplitterV2
        /// * Retrieves YieldTokenData 
        /// * Calculates unclaimed_yield, underlying_asset_tracked_amount, and yield_claimed.
        /// * Calculates the new underlying_asset_tracked_amount by adding current 
        /// underlying_asset_tracked_amount and the amount of the underlying asset passed in.
        /// * Calculates the new yt_amount (redemption value at start) if there is unclaimed
        /// yield (we track unclaimed yield when users tokenize as we do not want to force users
        /// to claim yield when they tokenize). 
        /// ** If there is unclaimed yield, new_redemption_value_at_start is calculated by getting the 
        /// redemption value of the new_underlying_asset_amount_tracked and subtracting unclaimed_yield.
        /// (The value of unclaimed yield is denominated in the redemption value of the underlying
        /// asset as is when calculating the yield owed when users claim yield).
        /// ** Calculating new value of yt_amount resets the redemption value at start to have a
        /// new baseline.
        fn handle_optional_yt_bucket(
            &mut self,
            optional_yt_bucket: Option<NonFungibleBucket>,
            asset_amount: Decimal,
            redemption_value_of_underlying_asset: Decimal,
        ) -> (UpdateOrMint, NonFungibleBucket) {
            match optional_yt_bucket {
                Some(yt_bucket) => {
                    // Assert YT sent is the same as YT associated to this PrismSplitterV2
                    assert_eq!(yt_bucket.resource_address(), self.yt_rm.address());
                    assert_eq!(yt_bucket.amount(), Decimal::ONE, "Can only have one YT NFT for now");

                    let local_id = yt_bucket.non_fungible_local_id();
                    let mut yt_data: YieldTokenData = self.yt_rm.get_non_fungible_data(&local_id);

                    let redemption_value_of_asset_to_tokenize = 
                        self.underlying_asset_pool
                        .get_redemption_value(asset_amount);

                    let new_redemption_value_at_start = 
                        yt_data.yt_amount
                        .checked_add(redemption_value_of_asset_to_tokenize)
                        .unwrap();

                    let new_last_claim_redemption_factor = self.redemption_factor;

                    let new_accrued_yield = self.calc_total_yield_owed(&yt_data, yt_data.yt_amount);
                    
                    //-----------------------------------------------------------------------
                    // STATE CHANGES
                    //-----------------------------------------------------------------------
                    yt_data.yt_amount = new_redemption_value_at_start;
                    yt_data.accrued_yield = new_accrued_yield;
                    yt_data.last_claim_redemption_factor = new_last_claim_redemption_factor;

                    self.update_yield_token_data(&local_id, &yt_data);

                    //-----------------------------------------------------------------------
                    // STATE CHANGES
                    //-----------------------------------------------------------------------

                    (UpdateOrMint::Update(local_id, yt_data), yt_bucket)

                },
                None => {
                    let yt_bucket = 
                        self.yt_rm
                        .mint_ruid_non_fungible(
                            YieldTokenData {
                                underlying_asset_address: self.asset_vault.resource_address(),
                                last_claim_redemption_factor: self.underlying_asset_pool.get_underlying_asset_redemption_factor(),
                                yt_amount: redemption_value_of_underlying_asset,
                                yield_claimed: Decimal::ZERO,
                                accrued_yield: Decimal::ZERO,
                                maturity_date: self.maturity_date
                            }
                        );

                    self.redemption_factor = 
                        self.underlying_asset_pool
                        .get_underlying_asset_redemption_factor();

                    (UpdateOrMint::Mint(
                        yt_bucket.non_fungible_local_id(), 
                        yt_bucket.non_fungible().data()
                    ), yt_bucket)
                }
            }
        }

        /// Redeems the underlying Asset from PT.
        /// 
        /// Can only redeem from PT if maturity date has passed.
        ///
        /// # Arguments
        ///
        /// * `pt_bucket`: [`FungibleBucket`] - A fungible bucket of PT.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A fungible bucket of the owed Asset.
        pub fn redeem_from_pt(
            &mut self,
            pt_bucket: FungibleBucket,
        ) -> FungibleBucket {
            // To redeem PT only, must wait until after maturity.
            assert_eq!(self.is_market_expired(), true);
            assert_eq!(
                pt_bucket.resource_address(), 
                self.pt_rm.address()
            );
            assert_eq!(pt_bucket.is_empty(), false);
            assert!(self.prism_splitter_is_active);
            self.update_redemption_factor();

            let asset_owed_amount = 
                self.underlying_asset_pool.calc_asset_owed_amount(pt_bucket.amount());
   
            let mut asset_owed_bucket = 
                self.withdraw_from_asset_vault(asset_owed_amount);

            asset_owed_bucket = if self.is_one_day_after_maturity() {
                self.charge_late_fee(asset_owed_bucket)
            } else {
                asset_owed_bucket
            };

            Runtime::emit_event(
                PTRedeemEvent {
                    asset_amount_owed: asset_owed_bucket.amount(),
                    pt_amount_burned: pt_bucket.amount(),
                }
            );
        
            pt_bucket.burn();

            return asset_owed_bucket
        }

        pub fn merge_multiple_yt(
            &mut self,
            yt_buckets: NonFungibleBucket,
        ) -> NonFungibleBucket {
            assert!(yt_buckets.amount() > Decimal::ONE);
            assert_eq!(
                yt_buckets.resource_address(), 
                self.yt_rm.address(), 
                "Invalid YT resource"
            );
            assert!(self.prism_splitter_is_active);
            self.update_redemption_factor();
        
            let mut combined_data = YieldTokenData {
                underlying_asset_address: self.asset_vault.resource_address(),
                last_claim_redemption_factor: self.redemption_factor,
                yt_amount: Decimal::ZERO,
                yield_claimed: Decimal::ZERO,
                accrued_yield: Decimal::ZERO,
                maturity_date: self.maturity_date
            };
        
            // Combine data from all YTs
            for yt_bucket in yt_buckets.non_fungibles::<YieldTokenData>().into_iter() {
        
                let local_id = yt_bucket.local_id();
                let yt_data: YieldTokenData = self.yt_rm.get_non_fungible_data(&local_id);

                let total_yield_owed = self.calc_total_yield_owed(&yt_data, yt_data.yt_amount);

                combined_data.yt_amount += yt_data.yt_amount;
                combined_data.yield_claimed += yt_data.yield_claimed;
                combined_data.accrued_yield += total_yield_owed;
            }
        
            // Mint new YT with combined data
            let new_yt_bucket = 
                self.yt_rm.mint_ruid_non_fungible(combined_data);
        
            // Burn old YTs
            yt_buckets.burn();
        
            new_yt_bucket
        }

        /// Calculates earned yield of YT.
        /// 
        /// # Mechanics
        /// 
        /// 1. Calculates redemption value of underlying_asset_tracked_amount tracked (
        /// this value will grow over time as yield are distributed)
        /// 2. Calculates the difference between redemption value of underlying_asset_tracked_amount
        /// and yt_amount (redemption value of underlying_asset_tracked_amount at start).
        /// This is the yield owed to the user denominated in the redemption value of the
        /// underlying asset.
        /// 3. Adds unclaimed_yield to yield_owed to get the total yield owed to the user.

        ///
        /// # Arguments
        ///
        /// * `data`: [`&YieldTokenData`] - The `NonFungibleData` of YT.
        ///
        /// # Returns
        ///
        /// * [`Decimal`] - The calculated earned yield from YT for the current period.
        fn calc_total_yield_owed(
            &self,
            yt_data: &YieldTokenData,
            yt_amount: Decimal,
        ) -> Decimal {

            self.calc_yield_owed(&yt_data, yt_amount)
            .checked_add(PreciseDecimal::from(yt_data.accrued_yield))
            .and_then(
                |amount| 
                amount.checked_round(
                    self.underlying_asset_divisibility(),
                    RoundingMode::ToNearestMidpointTowardZero
                )
            )
            .and_then(
                |amount|
                Decimal::try_from(amount).ok()
            )
            .unwrap()
        }

        fn calc_yield_owed(
            &self,
            yt_data: &YieldTokenData,
            yt_amount: Decimal,
        ) -> PreciseDecimal {

            let current_redemption_factor = 
                PreciseDecimal::from(self.redemption_factor);

            let last_redemption_factor = 
                PreciseDecimal::from(yt_data.last_claim_redemption_factor);

            let redemption_factor = 
                current_redemption_factor
                .checked_div(last_redemption_factor)
                .and_then(|factor| 
                    factor.checked_sub(PreciseDecimal::ONE)
                )
                .expect("[calc_yield_owed] Overflow in redemption factor calculation");

            // Adding accrued yield due to emissions auto compounding
            let yield_owed = 
                PreciseDecimal::from(yt_amount)
                .checked_add(PreciseDecimal::from(yt_data.accrued_yield))
                .and_then(
                    |amount|
                    amount.checked_mul(redemption_factor)
                )
                .expect("[calc_yield_owed] Overflow in yield owed calculation");

            return yield_owed
        }

        fn handle_excess_pt_bucket(
            &self,
            pt_bucket: &mut FungibleBucket,
            yt_amount: &Decimal,
        ) -> Option<FungibleBucket> {
            if pt_bucket.amount() > *yt_amount {
                let excess_pt_amount = 
                    pt_bucket.amount()
                    .checked_sub(*yt_amount)
                    .map(
                        |amount|
                        if amount.is_negative() {
                            Decimal::ZERO
                        } else {
                            amount
                        }
                    )
                    .unwrap();

                Some(pt_bucket.take(excess_pt_amount))
            } else {
                None
            }
        }

        fn determine_redemption_strategy(
            &self,
            yt_amount_to_redeem: Decimal,
            yt_data: &YieldTokenData,
        ) -> RedemptionStrategy {
            if yt_amount_to_redeem == yt_data.yt_amount {
                RedemptionStrategy::FullRedemption
            } else if self.is_market_expired() {
                RedemptionStrategy::ExpiredMarket
            } else {
                RedemptionStrategy::PartialRedemption
            }
        }

        fn handle_full_redemption(
            &self,
            yt_data: &YieldTokenData,
            pt_amount: Decimal,
        ) -> (Decimal, Decimal) {
            assert_eq!(pt_amount, yt_data.yt_amount);

            let yield_owed = 
                self.calc_total_yield_owed(yt_data, yt_data.yt_amount);

            let total_redemption_value_with_yield =
                yt_data.yt_amount
                .checked_add(yield_owed)
                .unwrap();

            (Decimal::ZERO, total_redemption_value_with_yield)
        }

        fn handle_partial_redemption(
            &self,
            yt_data: &YieldTokenData,
            pt_amount: Decimal,
        ) -> (Decimal, Decimal) {
            // Calculates proportional yield, which includes yield growth
            // from accrued yield (if any)
            let proportional_yield_owed =
                self.calc_yield_owed(yt_data, pt_amount)
                .checked_round(
                    self.underlying_asset_divisibility(),
                    RoundingMode::ToNearestMidpointTowardZero
                )
                .and_then(
                    |amount|
                    Decimal::try_from(amount).ok()
                )
                .unwrap();

            let total_redemption_value = 
                pt_amount
                .checked_add(proportional_yield_owed)
                .unwrap(); 

            // Calculates total yield, which includes 
            let accrued_yield = 
                self.calc_total_yield_owed(yt_data, yt_data.yt_amount)
                .checked_sub(proportional_yield_owed)
                .unwrap();

            (accrued_yield, total_redemption_value)
        }

        fn handle_expired_market(
            &self,
            yt_data: &YieldTokenData,
            pt_amount: Decimal,
        ) -> (Decimal, Decimal) {
            let yield_owed = 
                self.calc_total_yield_owed(yt_data, yt_data.yt_amount);

            let total_redemption_value_with_yield =
                pt_amount
                .checked_add(yield_owed)
                .unwrap();

            (Decimal::ZERO, total_redemption_value_with_yield)
        }

        fn charge_late_fee(
            &mut self,
            mut asset_owed_bucket: FungibleBucket,
        ) -> FungibleBucket {
            let fee_amount = 
                asset_owed_bucket.amount()
                .checked_mul(self.late_fee)
                .unwrap();

            let late_fee_bucket = 
                asset_owed_bucket
                .take(fee_amount);
            
            self.fee_vault.put(late_fee_bucket);

            asset_owed_bucket
        }
        
        pub fn calc_yield_owed_pub(
            &mut self,
            non_fungible_local_id: NonFungibleLocalId,
        ) -> Decimal {
            self.update_redemption_factor();

            let yt_data: YieldTokenData = 
                self.yt_rm
                    .get_non_fungible_data(&non_fungible_local_id);

            let yield_owed = 
                self.calc_total_yield_owed(&yt_data, yt_data.yt_amount);

            return yield_owed
        }

        pub fn calc_yield_owed_in_underlying(
            &mut self,
            non_fungible_local_id: NonFungibleLocalId,
        ) -> Decimal {

            let yield_owed_in_xrd = 
                self.calc_yield_owed_pub(non_fungible_local_id);
            
            self.underlying_asset_pool.calc_asset_owed_amount(yield_owed_in_xrd)
        }

        pub fn update_redemption_factor(&mut self) {
            // Get the current time.
            let current_time = UtcDateTime::from_instant(
                &Clock::current_time_rounded_to_seconds()
            ).unwrap();
    
            // If we are past maturity and we haven't locked in the index yet...
            if current_time >= self.maturity_date {
                if !self.locked_redemption_factor {
                    // Lock in the redemption factor at maturity.
                    self.redemption_factor = 
                        self.underlying_asset_pool
                        .get_underlying_asset_redemption_factor();
                    self.locked_redemption_factor = true;
                    self.last_redemption_factor_updated = current_time;
                }
                // Do nothing else if already locked.
                return;
            }
    
            // Otherwise (before maturity) update as usual.
            if current_time > self.last_redemption_factor_updated && 
                self.asset_vault.amount().is_positive() 
            {
                self.redemption_factor = 
                    self.underlying_asset_pool
                    .get_underlying_asset_redemption_factor();
                self.last_redemption_factor_updated = current_time;
            }
        }
         
        fn update_yield_token_data(
            &mut self, 
            id: &NonFungibleLocalId, 
            updated: &YieldTokenData
        ) {
            let changes = self.get_changed_fields(id, updated);
        
            for (field_name, new_value) in changes {
                self.yt_rm.update_non_fungible_data(id, &field_name, new_value);
            }
        }

        fn get_changed_fields(
            &self, 
            id: &NonFungibleLocalId, 
            updated: &YieldTokenData
        ) -> HashMap<String, Decimal> {
            let mut changes = HashMap::new();
            let original: YieldTokenData = self.yt_rm.get_non_fungible_data(id);
        
            let fields: [(String, Decimal, Decimal); 4] = [
                ("yt_amount".to_string(), original.yt_amount, updated.yt_amount),
                ("last_claim_redemption_factor".to_string(), original.last_claim_redemption_factor, updated.last_claim_redemption_factor),
                ("yield_claimed".to_string(), original.yield_claimed, updated.yield_claimed),
                ("accrued_yield".to_string(), original.accrued_yield, updated.accrued_yield),
            ];
        
            for (field_name, original_value, updated_value) in fields {
                if original_value != updated_value {
                    changes.insert(field_name, updated_value);
                }
            }
        
            changes
        }

        fn underlying_asset_divisibility(&self) -> u8 {
            self.asset_vault
            .resource_manager()
            .resource_type()
            .divisibility()
            .unwrap()
        }

        pub fn get_pt_redemption_value(
            &self,
            pt_amount: Decimal
        ) -> Decimal {
            self.underlying_asset_pool.calc_asset_owed_amount(pt_amount)
        }

        /// Checks whether maturity date has been reached.
        fn is_market_expired(&self) -> bool {
            Clock::current_time_comparison(
                self.maturity_date.to_instant(), 
                TimePrecision::Second, 
                TimeComparisonOperator::Gte
            )
        }

        fn is_one_day_after_maturity(&self) -> bool {
            let one_day_after_maturity = 
                self.maturity_date.add_days(1)
                .unwrap();

            Clock::current_time_comparison(
                one_day_after_maturity.to_instant(), 
                TimePrecision::Second, 
                TimeComparisonOperator::Gte
            )
        }

        pub fn change_redemption_factor(
            &mut self,
            new_redemption_factor: Decimal,
        ) {
            self.redemption_factor = new_redemption_factor;
        }

        pub fn change_adapter(
            &mut self,
            new_adapter: ComponentAddress,
        ) {
            self.underlying_asset_pool = 
                AssetPool::CustomPool(new_adapter.into());
        }

        pub fn change_maturity_date(
            &mut self,
            new_maturity_date: UtcDateTime
        ) {
            self.maturity_date = new_maturity_date;
            Runtime::global_component().set_metadata(
                "maturity_date", 
                new_maturity_date.to_string()
            );
        }

        pub fn initiate_migration(
            &mut self,
            migration_initiated: bool,
            migration_date: UtcDateTime,
            recipient: ComponentAddress,
        ) {
            self.set_prism_splitter_is_active(false);
            self.migration_state = MigrationState {
                migration_initiated,
                migration_date: Some(migration_date),
                recipient: Some(recipient),
                transaction_hash: None,
            };

            Runtime::emit_event(
                MigrationEvent {
                    migration_initiated,
                    migration_date: Some(migration_date),
                    recipient: Some(recipient),
                    transaction_hash: None,
                }
            );
        }

        pub fn cancel_migration(
            &mut self,
        ) {
            self.set_prism_splitter_is_active(true);
            self.migration_state = MigrationState {
                migration_initiated: false,
                migration_date: None,
                recipient: None,
                transaction_hash: None,
            };

            Runtime::emit_event(
                MigrationEvent {
                    migration_initiated: false,
                    migration_date: None,
                    recipient: None,
                    transaction_hash: None,
                }
            );
        }

        pub fn migrate_funds_to_new_prism_splitter(&mut self) {
            assert!(self.migration_state.migration_initiated);

            let is_migration_date_passed =
                Clock::current_time_comparison(
                    self.migration_state.migration_date.unwrap().to_instant(), 
                    TimePrecision::Second, 
                    TimeComparisonOperator::Gte
                );

            assert!(
                is_migration_date_passed, 
                "Migration not allowed yet"
            );

            let new_prism_splitter_address = 
                self.migration_state.recipient.unwrap();

            let access_rule = 
                rule!(require(global_caller(new_prism_splitter_address)));

            self.update_protocol_resource_roles(access_rule);

            let transaction_hash = Runtime::transaction_hash();

            self.migration_state.transaction_hash = 
                Some(transaction_hash);

            let asset_bucket = self.asset_vault.take_all();

            ScryptoVmV1Api::object_call(
                new_prism_splitter_address.as_node_id(), 
                "deposit_to_asset_vault", 
                scrypto_args!(asset_bucket)
            );

            Runtime::emit_event(
                MigrationExecutedEvent {
                    migration_date: UtcDateTime::from_instant(
                        &Clock::current_time_rounded_to_seconds())
                        .unwrap(),
                    recipient: new_prism_splitter_address,
                    transaction_hash: transaction_hash,
                }
            );
        }

        pub fn get_migration_state(&self) -> MigrationState {
            self.migration_state
        }

        pub fn get_prism_splitter_is_active(&self) -> bool {
            self.prism_splitter_is_active
        }

        pub fn get_late_fee(&self) -> Decimal {
            self.late_fee
        }

        pub fn set_prism_splitter_is_active(
            &mut self,
            status: bool,
        ) {
            self.prism_splitter_is_active = status;
        }

        fn withdraw_from_asset_vault(
            &mut self,
            amount: Decimal,
        ) -> FungibleBucket {
            self.asset_vault
            .take_advanced(
                amount,
                WithdrawStrategy::Rounded(RoundingMode::ToNearestMidpointToEven)
            )
        }

        pub fn deposit_to_asset_vault(
            &mut self, 
            asset_bucket: FungibleBucket
        ) {
            self.asset_vault.put(asset_bucket);
        } 

        pub fn update_protocol_resource_roles(
            &mut self,
            access_rule: AccessRule
        ) {
            self.pt_rm.set_role("minter", access_rule.clone());
            self.pt_rm.set_role("burner", access_rule.clone());
            self.yt_rm.set_role("minter", access_rule.clone());
            self.yt_rm.set_role("non_fungible_data_updater", access_rule);
        }

        pub fn update_protocol_rm(
            &mut self,
            pt_rm: ResourceAddress,
            yt_rm: ResourceAddress,
        ) {
            self.pt_rm = FungibleResourceManager::from(pt_rm);
            self.yt_rm = NonFungibleResourceManager::from(yt_rm);
        }

        pub fn change_late_fee(
            &mut self,
            late_fee: Decimal,
        ) {
            self.late_fee = late_fee;
        }
    }

    impl PrismSplitterAdapterInterfaceTrait for PrismSplitterV2 {
        /// Tokenizes the Asset to its PT and YT.
        /// 
        /// # Mechanics
        /// 1. Checks if the market is expired. If it is, it panics. If it is not, it continues.
        /// 2. Checks if the input asset is an Asset. If it is not, it panics. If it is, it continues.
        /// 3. Checks if the input asset amount is greater than zero. If it is not, it panics. 
        /// If it is, it continues.
        /// 4. Calculates the redemption value of the input asset amount. This is the amount of PT
        /// that will be minted as this is the principal value of the yield bearing asset.
        /// 5. Calls handle_optional_yt_bucket to mint or update the YT based on whether the caller
        /// provided a YT bucket or not. If the caller does not have YT, mint new YT. If the caller 
        /// does, update the existing YT.
        /// 6. Return PT and YT to caller.
        ///
        /// # Arguments
        ///
        /// * `asset_bucket`: [`FungibleBucket`] - A fungible bucket of Asset tokens to tokenize.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A fungible bucket of PT.
        /// * [`NonFungibleBucket`] - A non fungible bucket of YT.
        fn tokenize(
            &mut self, 
            asset_bucket: FungibleBucket,
            optional_yt_bucket: Option<NonFungibleBucket>,
        ) -> (FungibleBucket, NonFungibleBucket) {
            assert_eq!(self.is_market_expired(), false);
            assert_eq!(
                asset_bucket.resource_address(), 
                self.asset_vault.resource_address()
            );
            assert_eq!(asset_bucket.is_empty(), false);
            assert!(self.prism_splitter_is_active);
            self.update_redemption_factor();

            let asset_amount = asset_bucket.amount();

            let redemption_value_of_underlying_asset = 
                self.underlying_asset_pool
                    .get_redemption_value(asset_amount);

            let pt_bucket = 
                self.pt_rm
                    .mint(redemption_value_of_underlying_asset);

            self.deposit_to_asset_vault(asset_bucket);

            let (yt_update_or_mint, yt_bucket) = 
                self.handle_optional_yt_bucket(
                    optional_yt_bucket, 
                    asset_amount, 
                    redemption_value_of_underlying_asset
                );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            Runtime::emit_event(
                TokenizeEvent {
                    amount_tokenized: asset_amount,
                    pt_amount_minted: pt_bucket.amount(),
                    yt_update_or_mint: yt_update_or_mint,
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            return (pt_bucket, yt_bucket)
        }

        /// Redeems the underlying Asset from PT and YT.
        ///
        /// # Arguments
        ///
        /// * `pt_bucket`: [`FungibleBucket`] - A fungible bucket of PT.
        /// * `yt_bucket`: [`NonFungibleBucket`] - A non fungible bucket of YT.
        /// * `yt_redeem_amount`: [`Decimal`] - Desired amount of YT to redeem.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A fungible bucket of the owed Asset.
        /// * [`Option<NonFungibleBucket>`] - Returns a non fungible bucket of YT
        /// if not all is redeemed.
        fn redeem(
            &mut self, 
            mut pt_bucket: FungibleBucket, 
            yt_bucket: NonFungibleBucket, 
            yt_amount_to_redeem: Decimal, 
        ) 
        -> (
            FungibleBucket, 
            Option<NonFungibleBucket>,
            Option<FungibleBucket>,
            ) 
        {
            // Assert PT & YT sent is the same as PT & YT associated to this PrismSplitterV2
            assert_eq!(pt_bucket.resource_address(), self.pt_rm.address());
            assert_eq!(yt_bucket.resource_address(), self.yt_rm.address());
            assert_eq!(yt_bucket.amount(), Decimal::ONE, "Can only have one YT NFT for now");
            assert_eq!(pt_bucket.is_empty(), false);
            assert!(self.prism_splitter_is_active);
            self.update_redemption_factor();
    
            let yt_id = yt_bucket.non_fungible_local_id();
            let mut yt_data: YieldTokenData = yt_bucket.non_fungible().data();  

            // Checks if there are excess PT, which is determined by the maximum
            // redemption of the YT.
            let optional_excess_pt_bucket: Option<FungibleBucket> = 
                self.handle_excess_pt_bucket(
                    &mut pt_bucket, 
                    &yt_data.yt_amount
                );

            assert!(
                yt_data.yt_amount >= yt_amount_to_redeem,
                "[redeem] Insufficient YT Amount"
            );

            assert_eq!(
                pt_bucket.amount(), yt_amount_to_redeem,
                "[redeem] PT and YT amount needs to be the same."
            );

            let redemption_strategy = 
                self.determine_redemption_strategy(
                    yt_amount_to_redeem, 
                    &yt_data
                );

            let (
                new_accrued_yield,
                total_redemption_value_with_yield
            ) = match redemption_strategy {
                RedemptionStrategy::FullRedemption => {
                    self.handle_full_redemption(
                        &yt_data, 
                        pt_bucket.amount()
                    )
                },
                RedemptionStrategy::PartialRedemption => {
                    self.handle_partial_redemption(
                        &yt_data,
                        pt_bucket.amount()
                    )
                },
                RedemptionStrategy::ExpiredMarket => {
                    self.handle_expired_market(
                        &yt_data,
                        pt_bucket.amount()
                    )
                }
            };

            let asset_owed_amount = 
                self.underlying_asset_pool
                .calc_asset_owed_amount(total_redemption_value_with_yield);

            let mut asset_owed_bucket =
                self.withdraw_from_asset_vault(asset_owed_amount);

            asset_owed_bucket = if self.is_one_day_after_maturity() {
                self.charge_late_fee(asset_owed_bucket)
            } else {
                asset_owed_bucket
            };

            let pt_amount_burned = pt_bucket.amount();
            pt_bucket.burn();

            let mut optional_yt_bucket = Some(yt_bucket);

            let (
                yt_update_or_burn,
                result_optional_yt_bucket
            ) = match redemption_strategy {
                RedemptionStrategy::FullRedemption => {
                    if let Some(bucket) = optional_yt_bucket.take() {
                        bucket.burn();
                    }
                    (UpdateOrBurn::Burn, None)
                }
                RedemptionStrategy::PartialRedemption => {
                    let new_redemption_value_at_start = 
                       yt_data.yt_amount
                       .checked_sub(yt_amount_to_redeem)
                       .map(
                        |amount|
                            if amount.is_negative() {
                                Decimal::ZERO 
                            } else {
                                amount
                            }
                       )
                       .unwrap();

                    let new_last_claim_redemption_factor = 
                        self.redemption_factor;

                    //-----------------------------------------------------------------------
                    // STATE CHANGES
                    //-----------------------------------------------------------------------
                    yt_data.yt_amount = new_redemption_value_at_start;
                    yt_data.accrued_yield = new_accrued_yield;
                    yt_data.last_claim_redemption_factor = new_last_claim_redemption_factor;

                    self.update_yield_token_data(&yt_id, &yt_data);
                    //-----------------------------------------------------------------------
                    // STATE CHANGES
                    //-----------------------------------------------------------------------

                    (UpdateOrBurn::Update(yt_id, yt_data), optional_yt_bucket)
                },
                RedemptionStrategy::ExpiredMarket => {
                    if let Some(bucket) = optional_yt_bucket.take() {
                        bucket.burn();
                    }
                    (UpdateOrBurn::Burn, None)
                }
            };

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            Runtime::emit_event(
                RedeemEvent {
                    asset_amount_owed: asset_owed_bucket.amount(),
                    pt_amount_burned,
                    yt_update_or_burn,
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            (asset_owed_bucket, result_optional_yt_bucket, optional_excess_pt_bucket)
        }

        /// Claims owed yield for the period.
        /// 
        /// # Mechanics
        ///
        /// 1. Checks proof of YT to ensure the YT belongs to the user.
        /// 2. Retrieve YieldTokenData from YT.
        /// 3. Calls calc_yield_owed to get owed amount denominated in the 
        /// redemption value of the underlying asset.
        /// 4. Calculates the amount to return to user denominated in the underlying asset
        /// using calc_asset_owed_amount.
        /// 5. Calculate new underlying asset amount tracked as this will be deducted when
        /// returning yield owed to user.
        /// 6. Update YieldTokenData state (underlying_asset_tracked_amount & yield_claimed)
        /// 7. Take the amount owed from vault and return to user.
        /// 
        /// # Arguments
        ///
        /// * `yt_proof`: [`NonFungibleProof`] - A non fungible proof of YT.
        ///
        /// # Returns
        ///
        /// * [`Bucket`] - A bucket of the Unstake NFT.
        /// Note: https://docs.radixdlt.com/docs/validator#unstake-nft
        fn claim_yield(
            &mut self, 
            yt_bucket: NonFungibleBucket,
        ) -> (FungibleBucket, Option<NonFungibleBucket>) {
            assert_eq!(yt_bucket.resource_address(), self.yt_rm.address());
            assert_eq!(yt_bucket.amount(), Decimal::ONE, "Can only have one YT NFT for now");
            assert!(self.prism_splitter_is_active);
            self.update_redemption_factor();

            let yt_id = yt_bucket.non_fungible_local_id();
            let mut yt_data: YieldTokenData = yt_bucket.non_fungible().data();

            let yield_owed = 
                self.calc_total_yield_owed(&yt_data, yt_data.yt_amount);

            let required_underlying_asset_for_yield_owed =
                self.underlying_asset_pool.calc_asset_owed_amount(yield_owed);

            let mut asset_owed_bucket = 
                self.withdraw_from_asset_vault(required_underlying_asset_for_yield_owed);
            
            asset_owed_bucket = if self.is_one_day_after_maturity() {
                self.charge_late_fee(asset_owed_bucket)
            } else {
                asset_owed_bucket
            };

            let optional_yt_bucket = 
                if !self.is_market_expired() {

                    let new_yield_claimed = 
                        yt_data.yield_claimed
                        .checked_add(yield_owed)
                        .unwrap();
    
                    let new_accrued_yield = 
                        yt_data.accrued_yield
                        .checked_sub(yt_data.accrued_yield)
                        .map(
                            |amount|
                            if amount.is_negative() {
                                Decimal::ZERO
                            } else {
                                amount
                            }
                        )
                        .unwrap();
    
                    let new_last_claim_redemption_factor = 
                        self.redemption_factor;
                    //-----------------------------------------------------------------------
                    // STATE CHANGES
                    //-----------------------------------------------------------------------
                    yt_data.yield_claimed = new_yield_claimed;
                    yt_data.accrued_yield = new_accrued_yield;
                    yt_data.last_claim_redemption_factor = new_last_claim_redemption_factor;
                    
                    self.update_yield_token_data(&yt_id, &yt_data);
                    //-----------------------------------------------------------------------
                    // STATE CHANGES
                    //-----------------------------------------------------------------------

                    Some(yt_bucket)
                } else {
                    yt_bucket.burn();
                    None
                };

            Runtime::emit_event(
                ClaimEvent {
                    non_fungible_local_id: yt_id,
                    yt_data,
                    current_redemption_factor: self.redemption_factor,
                    asset_amount_owed: asset_owed_bucket.amount(),
                }
            );

            return (asset_owed_bucket, optional_yt_bucket)
        }

        fn get_underlying_asset_redemption_value(
            &self,
            amount: Decimal,
        ) -> Decimal {
            self.underlying_asset_pool.get_redemption_value(amount)
        }

        fn get_underlying_asset_redemption_factor(
            &self
        ) -> Decimal {
            self.underlying_asset_pool.get_underlying_asset_redemption_factor()
        }

        fn calc_asset_owed_amount(
            &self,
            amount: Decimal,
        ) -> Decimal {
            self.underlying_asset_pool.calc_asset_owed_amount(amount)
        }

        fn pt_address(&self) -> ResourceAddress {
            self.pt_rm.address()
        }

        fn yt_address(&self) -> ResourceAddress {
            self.yt_rm.address()
        }

        fn underlying_asset(&self) -> ResourceAddress {
            self.asset_vault.resource_address()
        }

        fn protocol_resources(&self) -> (ResourceAddress, ResourceAddress) {
            (self.pt_rm.address(), self.yt_rm.address())
        }

        fn maturity_date(&self) -> UtcDateTime {
            self.maturity_date
        }
    }
}

#[derive(ScryptoSbor)]
pub enum PoolType {
    Validator,          
    LiquidityPool,      
    CustomPool(ComponentAddress), 
}

#[derive(ScryptoSbor, PartialEq, Debug)]
pub enum AssetPool {
    Validator(ValidatorWrapper),
    LiquidityPool(OneResourcePoolWrapper),
    CustomPool(PoolAdapter),
}

impl AssetPool {
    pub fn get_redemption_value(&self, amount: Decimal) -> Decimal {
        match self {
            AssetPool::Validator(validator) => validator.get_redemption_value(amount),
            AssetPool::LiquidityPool(pool) => pool.get_redemption_value(amount),
            AssetPool::CustomPool(pool) => pool.get_redemption_value(amount),
        }
    }

    pub fn calc_asset_owed_amount(&self, amount: Decimal) -> Decimal {
        match self {
            AssetPool::Validator(validator) => validator.calc_asset_owed_amount(amount),
            AssetPool::LiquidityPool(pool) => pool.calc_asset_owed_amount(amount),
            AssetPool::CustomPool(pool) => pool.calc_asset_owed_amount(amount),
        }
    }

    pub fn get_underlying_asset_redemption_factor(&self) -> Decimal {
        match self {
            AssetPool::Validator(validator) => validator.get_redemption_factor(),
            AssetPool::LiquidityPool(pool) => pool.get_redemption_factor(),
            AssetPool::CustomPool(pool) => pool.get_redemption_factor(),
        }
    }

    pub fn total_stake_amount(&self) -> Decimal {
        match self {
            AssetPool::Validator(validator) => validator.total_stake_amount(),
            AssetPool::LiquidityPool(pool) => pool.total_stake_amount(),
            AssetPool::CustomPool(pool) => pool.total_stake_amount(),
        }
    }

    pub fn total_stake_unit_supply(&self) -> Decimal {
        match self {
            AssetPool::Validator(validator) => validator.total_stake_unit_supply(),
            AssetPool::LiquidityPool(pool) => pool.total_stake_unit_supply(),
            AssetPool::CustomPool(pool) => pool.total_stake_unit_supply(),
        }
    }

    pub fn stake_unit_resource_address(&self) -> ResourceAddress {
        match self {
            AssetPool::Validator(validator) => validator.stake_unit_resource_address(),
            AssetPool::LiquidityPool(pool) => pool.stake_unit_resource_address(),
            AssetPool::CustomPool(pool) => pool.stake_unit_resource_address(),
        }
    }

    pub fn pool_address(&self) -> ComponentAddress {
        match self {
            AssetPool::Validator(validator) => validator.pool_address(),
            AssetPool::LiquidityPool(pool) => pool.pool_address(),
            AssetPool::CustomPool(pool) => pool.pool_address(),
        }
    }
}

pub fn get_pool_component_address(pool_unit: ResourceAddress) -> ComponentAddress {
    let pool_unit_rm = ResourceManager::from(pool_unit);

    let global_address: GlobalAddress = 
        pool_unit_rm
        .get_metadata("pool")
        .unwrap()
        .unwrap();

    ComponentAddress::try_from(global_address).ok().unwrap()
}

pub fn retrieve_metadata(
    resource_manager: ResourceManager
) -> (String, String, UncheckedUrl) {

    let market_name: String =     
        resource_manager
        .get_metadata("name")
        .unwrap_or(Some("".to_string()))
        .unwrap_or("".to_string());
    let market_symbol: String = 
        resource_manager
        .get_metadata("symbol")
        .unwrap_or(Some("".to_string()))
        .unwrap_or("".to_string());
    let market_icon: UncheckedUrl = 
        resource_manager
        .get_metadata("icon_url")
        .unwrap_or(Some(UncheckedUrl::of("https://www.prismterminal.com/assets/glowlogo.svg")))
        .unwrap_or(UncheckedUrl::of("https://www.prismterminal.com/assets/glowlogo.svg"));

    (market_name, market_symbol, market_icon)
}
