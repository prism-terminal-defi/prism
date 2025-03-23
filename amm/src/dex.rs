use scrypto::prelude::*;
use scrypto_math::*;
use crate::structs::*;
use prism_calculations::liquidity_curve::*;
use crate::events::*;

/// 365 days in seconds
const PERIOD_SIZE: Decimal = dec!(31536000);

#[blueprint]
#[events(InstantiateAMMEvent, SwapEvent)]
mod yield_amm {
    // The associated PrismSplitterV2 package and component which is used to verify associated PT, YT, and 
    // Asset asset. It is also used to perform YT <---> Asset swaps.
    extern_blueprint! {
        // "package_sim1p4rcnrz0sjnh0e9klyf7atedfmtgghkdn5fd5tefpyrtt7tcjwv7th",
        // Stokenet
        // "package_tdx_2_1p59fttwdx3s8hc5l7krjqaeslukvmsepq6cjyvmfkn8ugu5c02ghn4",
        // Mainnet
        "package_rdx1pkg6mcr85erca2q8tm5gpe2vws93xw3d8yldkq7zjt95dr7cmp6u27",
        PrismSplitterV2 {
            fn tokenize(
                &mut self, 
                amount: FungibleBucket,
                optional_yt_bucket: Option<NonFungibleBucket>,
            ) -> (FungibleBucket, NonFungibleBucket);
            fn redeem(
                &mut self, 
                pt_bucket: FungibleBucket, 
                yt_bucket: NonFungibleBucket,
                yt_amount_to_redeem: Decimal,
            ) -> 
                (
                    FungibleBucket, 
                    Option<NonFungibleBucket>,
                    Option<FungibleBucket>,
                );
            fn claim_yield(&mut self, yt_bucket: NonFungibleBucket) -> (FungibleBucket, Option<NonFungibleBucket>);
            fn get_pt_redemption_value(&self, amount: Decimal) -> Decimal;
            fn get_underlying_asset_redemption_value(&self, amount: Decimal) -> Decimal;
            fn get_underlying_asset_redemption_factor(&self) -> Decimal;
            fn pt_address(&self) -> ResourceAddress;
            fn yt_address(&self) -> ResourceAddress;
            fn underlying_asset(&self) -> ResourceAddress;
            fn maturity_date(&self) -> UtcDateTime;
            fn protocol_resources(&self) -> (ResourceAddress, ResourceAddress);
        }
    }

    const OWNER_BADGE_RM: ResourceManager = 
        resource_manager!("resource_rdx1tk4zl8p0wzh0g3f39adzv37xg7jmgm0th7q6ud78wv48nffzlsvrch");

    enable_function_auth! {
        instantiate_yield_amm => rule!(require(OWNER_BADGE_RM.address()));
        instantiate_yield_amm_with_existing => rule!(require(OWNER_BADGE_RM.address()));
        // instantiate_yield_amm => rule!(allow_all);
        // instantiate_yield_amm_with_existing => rule!(allow_all);
        retrieve_metadata => rule!(allow_all);
    }

    enable_method_auth! {
        methods {
            set_initial_ln_implied_rate => restrict_to: [OWNER, SELF];
            get_market_implied_rate => PUBLIC;
            get_vault_reserves => PUBLIC;
            get_market_state => PUBLIC;
            add_liquidity => PUBLIC;
            remove_liquidity => PUBLIC;
            swap_exact_pt_for_asset => PUBLIC;
            swap_exact_asset_for_pt => PUBLIC;
            swap_exact_asset_for_yt => PUBLIC;
            swap_exact_yt_for_asset => PUBLIC;
            compute_market => PUBLIC;
            time_to_expiry => PUBLIC;
            check_maturity => PUBLIC;
            create_pool_manager_proof => restrict_to: [SELF, OWNER];
            withdraw_pool_manager_badge => restrict_to: [OWNER];
            change_maturity_date => restrict_to: [OWNER];
            change_market_status => restrict_to: [OWNER];
            force_change_last_implied_rate => restrict_to: [OWNER];
            change_scalar_root => restrict_to: [OWNER];
            change_prism_splitter => restrict_to: [OWNER];
            change_pool_component => restrict_to: [OWNER];
        }
    }
    pub struct YieldAMM {
        /// The native pool component which manages liquidity reserves. 
        pub pool_component: Global<TwoResourcePool>,
        pub pool_manager_access_controller: Global<AccessController>,
        pub prism_splitter_component: ComponentAddress,
        /// The initial scalar root of the market. This is used to calculate
        /// the scalar value. It determins the slope of the curve and becomes
        /// less sensitive as the market approaches maturity. The higher the 
        /// scalar value the more flat the curve is, the lower the scalar value
        /// the more steep the curve is.
        pub market_fee: MarketFee,
        pub market_state: MarketState,
        pub market_info: MarketInfo,
        pub pool_stat: PoolStat,
        pub market_is_active: bool,
        pub pool_manager_vault: FungibleVault,
    }

    impl YieldAMM {
        /// Instantiates a Yield AMM DEX. The basic implementation of the DEX only allows one
        /// asset pair to be traded, 
        pub fn instantiate_yield_amm(
            /* Rules */
            owner_role_node: CompositeRequirement,
            /* Initial market values */
            // The initial scalar root of the market which determines the initial
            // steepness of the curve (high slippage at the ends of the curve).
            initial_rate_anchor: PreciseDecimal,
            scalar_root: Decimal,
            market_fee_input: MarketFeeInput,
            prism_splitter_address: ComponentAddress,
            pool_manager_access_controller: Global<AccessController>,
            dapp_definition: ComponentAddress,
            pool_manager_badge: FungibleBucket,
            address_reservation: Option<GlobalAddressReservation>,
        ) -> Global<YieldAMM> {
            assert!(scalar_root > Decimal::ZERO);
            assert!(market_fee_input.fee_rate > Decimal::ZERO);
            assert!(
                market_fee_input.reserve_fee_percent > Decimal::ZERO 
                && market_fee_input.reserve_fee_percent < Decimal::ONE
            );

            let (address_reservation, component_address) = match address_reservation {
                Some(address_reservation) => {
                    let component_address = 
                        ComponentAddress::try_from(
                            Runtime::get_reservation_address(&address_reservation))
                        .ok()
                        .expect("[instantiate_yield_amm] Failed to convert address reservation to component address");

                    (address_reservation, component_address)
                },
                None => Runtime::allocate_component_address(YieldAMM::blueprint_id()),
            };

            let global_component_caller_badge =
                NonFungibleGlobalId::global_caller_badge(component_address);
        
            let prism_splitter_component: Global<PrismSplitterV2> = 
                prism_splitter_address.into();

            let (market_name, market_symbol, market_icon) =
                Self::retrieve_metadata(prism_splitter_component);

            let underlying_asset_address = 
                prism_splitter_component.underlying_asset();
            
            let (pt_address, yt_address) = 
                prism_splitter_component.protocol_resources();

            let maturity_date = prism_splitter_component.maturity_date();

            let is_current_time_less_than_maturity_date = 
                Clock::current_time_comparison(
                    maturity_date.to_instant(), 
                    TimePrecisionV2::Second, 
                    TimeComparisonOperator::Lt
                );

            assert_eq!(
                is_current_time_less_than_maturity_date, 
                true,
                "Market has expired!"
            );

            let combined_rule_node = 
                owner_role_node
                .or(CompositeRequirement::from(global_component_caller_badge))
                .or(CompositeRequirement::from(Runtime::package_token()));

            let owner_role = 
                OwnerRole::Updatable(
                    AccessRule::from(
                        combined_rule_node.clone()
                    )
                );

            // Component pool to store DEX assets
            let pool_component = 
                Blueprint::<TwoResourcePool>::instantiate(
                owner_role.clone(),
                AccessRule::from(combined_rule_node),
                (pt_address, underlying_asset_address),
                None,
            );

            let pool_unit_global_address: GlobalAddress = 
                pool_component
                .get_metadata("pool_unit")
                .unwrap()
                .unwrap();

            let pool_unit_address = 
                ResourceAddress::try_from(pool_unit_global_address)
                .ok()
                .expect("[instantiate_yield_amm] Failed to convert pool unit global address to resource address"); 
            
            ResourceManager::from(pool_unit_address)
                .set_metadata("name", format!("LP {}", market_name));
            ResourceManager::from(pool_unit_address)
                .set_metadata("symbol", format!("lp{}", market_symbol));
            ResourceManager::from(pool_unit_address)
                .set_metadata("icon_url", market_icon.clone());
            ResourceManager::from(pool_unit_address)
                .set_metadata("dapp_definition", GlobalAddress::from(dapp_definition));

            let market_state = MarketState {
                total_pt: Decimal::ZERO,
                total_asset: Decimal::ZERO,
                initial_rate_anchor,
                scalar_root,
                last_ln_implied_rate: PreciseDecimal::ZERO,
            };

            let market_info = MarketInfo {
                maturity_date,
                underlying_asset_address,
                pt_address,
                yt_address,
                pool_unit_address,
            };

            let fee_rate = 
                PreciseDecimal::from(
                    market_fee_input.fee_rate
                    .ln()
                    .expect("[instantiate_yield_amm] Failed to calculate fee rate")
                );

            let market_fee = MarketFee {
                fee_rate,
                reserve_fee_percent: market_fee_input.reserve_fee_percent
            };

            let pool_stat = PoolStat {
                trading_fees_collected: PreciseDecimal::ZERO,
                reserve_fees_collected: PreciseDecimal::ZERO,
                total_fees_collected: PreciseDecimal::ZERO,
            };

            Runtime::emit_event(
                InstantiateAMMEvent {
                    market_state: market_state.clone(),
                    market_fee: market_fee.clone()
                }
            );

            Self {
                pool_component,
                prism_splitter_component: prism_splitter_component.address(),
                pool_manager_access_controller,
                market_fee,
                market_state,
                market_info: market_info.clone(),
                pool_stat,
                market_is_active: true,
                pool_manager_vault: FungibleVault::with_bucket(pool_manager_badge),
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .metadata(metadata! {
                roles {
                    metadata_locker => OWNER;
                    metadata_locker_updater => OWNER;
                    metadata_setter => OWNER;
                    metadata_setter_updater => OWNER;
                },
                init {
                    "market_name" => market_name, updatable;
                    "symbol" => market_symbol, updatable;
                    "icon_url" => market_icon, updatable;
                    "market_resources" => vec![
                        market_info.underlying_asset_address,
                        market_info.pt_address,
                        market_info.yt_address,
                    ], locked;
                    "pool_unit" => market_info.pool_unit_address, locked;
                    "maturity_date" => maturity_date.to_string(), updatable;
                    "dapp_definition" => dapp_definition, updatable;
                }
            })
            .enable_component_royalties(
                component_royalties! {
                roles {
                    royalty_setter => OWNER;
                    royalty_setter_updater => OWNER;
                    royalty_locker => OWNER;
                    royalty_locker_updater => OWNER;
                    royalty_claimer => OWNER;
                    royalty_claimer_updater => OWNER;
                },
                init {
                    create_pool_manager_proof => Free, updatable;
                    withdraw_pool_manager_badge => Free, updatable;
                    set_initial_ln_implied_rate => Free, updatable;
                    get_market_implied_rate => Free, updatable;
                    get_vault_reserves => Free, updatable;
                    get_market_state => Free, updatable;
                    add_liquidity => Free, updatable;
                    remove_liquidity => Free, updatable;
                    swap_exact_pt_for_asset => Free, updatable;
                    swap_exact_asset_for_pt => Free, updatable;
                    swap_exact_asset_for_yt => Free, updatable;
                    swap_exact_yt_for_asset => Free, updatable;
                    compute_market => Free, updatable;
                    time_to_expiry => Free, updatable;
                    check_maturity => Free, updatable;
                    change_maturity_date => Free, updatable;
                    change_market_status => Free, updatable;
                    force_change_last_implied_rate => Free, updatable;
                    change_scalar_root => Free, updatable;
                    change_prism_splitter => Free, updatable;
                    change_pool_component => Free, updatable;
                }
            })
            .with_address(address_reservation)
            .globalize()
        }

        pub fn instantiate_yield_amm_with_existing(
            owner_role_node: CompositeRequirement,
            last_ln_implied_rate: PreciseDecimal,
            scalar_root: Decimal,
            market_fee_input: MarketFeeInput,
            pool_component: Global<TwoResourcePool>,
            pool_manager_access_controller: Global<AccessController>,
            prism_splitter_address: ComponentAddress,
            dapp_definition: ComponentAddress,
            pool_manager_badge: FungibleBucket,
            address_reservation: Option<GlobalAddressReservation>,
        ) -> Global<YieldAMM> {
            assert!(scalar_root > Decimal::ZERO);
            assert!(market_fee_input.fee_rate > Decimal::ZERO);
            assert!(
                market_fee_input.reserve_fee_percent > Decimal::ZERO 
                && market_fee_input.reserve_fee_percent < Decimal::ONE
            );

            let (address_reservation, component_address) = match address_reservation {
                Some(address_reservation) => {
                    let component_address = 
                        ComponentAddress::try_from(
                            Runtime::get_reservation_address(&address_reservation))
                        .ok()
                        .expect("[instantiate_yield_amm] Failed to convert address reservation to component address");

                    (address_reservation, component_address)
                },
                None => Runtime::allocate_component_address(YieldAMM::blueprint_id()),
            };

            let global_component_caller_badge =
                NonFungibleGlobalId::global_caller_badge(component_address);
        
            let prism_splitter_component: Global<PrismSplitterV2> = 
                prism_splitter_address.into();

            let (market_name, market_symbol, market_icon) =
                Self::retrieve_metadata(prism_splitter_component);

            let underlying_asset_address = 
                prism_splitter_component.underlying_asset();
            
            let (pt_address, yt_address) = 
                prism_splitter_component.protocol_resources();

            let maturity_date = prism_splitter_component.maturity_date();

            let is_current_time_less_than_maturity_date = 
                Clock::current_time_comparison(
                    maturity_date.to_instant(), 
                    TimePrecisionV2::Second, 
                    TimeComparisonOperator::Lt
                );

            assert_eq!(
                is_current_time_less_than_maturity_date, 
                true,
                "Market has expired!"
            );

            let combined_rule_node = 
                owner_role_node
                .or(CompositeRequirement::from(global_component_caller_badge))
                .or(CompositeRequirement::from(Runtime::package_token()));

            let owner_role = 
                OwnerRole::Updatable(
                    AccessRule::from(
                        combined_rule_node.clone()
                    )
                );

            let pool_unit_global_address: GlobalAddress = 
                pool_component
                .get_metadata("pool_unit")
                .unwrap()
                .unwrap();

            let pool_unit_address = 
                ResourceAddress::try_from(pool_unit_global_address)
                .ok()
                .expect("[instantiate_yield_amm] Failed to convert pool unit global address to resource address"); 

            let market_state = MarketState {
                total_pt: Decimal::ZERO,
                total_asset: Decimal::ZERO,
                initial_rate_anchor: last_ln_implied_rate,
                scalar_root,
                last_ln_implied_rate: last_ln_implied_rate,
            };


            let market_info = MarketInfo {
                maturity_date,
                underlying_asset_address,
                pt_address,
                yt_address,
                pool_unit_address,
            };

            let fee_rate = 
                PreciseDecimal::from(
                    market_fee_input.fee_rate
                    .ln()
                    .expect("[instantiate_yield_amm] Failed to calculate fee rate")
                );
            
            let market_fee = MarketFee {
                fee_rate,
                reserve_fee_percent: market_fee_input.reserve_fee_percent
            };

            let pool_stat = PoolStat {
                trading_fees_collected: PreciseDecimal::ZERO,
                reserve_fees_collected: PreciseDecimal::ZERO,
                total_fees_collected: PreciseDecimal::ZERO,
            };

            Runtime::emit_event(
                InstantiateAMMEvent {
                    market_state: market_state.clone(),
                    market_fee: market_fee.clone()
                }
            );

            Self {
                pool_component,
                prism_splitter_component: prism_splitter_component.address(),
                pool_manager_access_controller,
                market_fee,
                market_state,
                market_info: market_info.clone(),
                pool_stat,
                market_is_active: true,
                pool_manager_vault: FungibleVault::with_bucket(pool_manager_badge),
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .metadata(metadata! {
                roles {
                    metadata_locker => OWNER;
                    metadata_locker_updater => OWNER;
                    metadata_setter => OWNER;
                    metadata_setter_updater => OWNER;
                },
                init {
                    "market_name" => market_name, updatable;
                    "symbol" => market_symbol, updatable;
                    "icon_url" => market_icon, updatable;
                    "market_resources" => vec![
                        market_info.underlying_asset_address,
                        market_info.pt_address,
                        market_info.yt_address,
                    ], locked;
                    "pool_unit" => market_info.pool_unit_address, locked;
                    "maturity_date" => maturity_date.to_string(), updatable;
                    "dapp_definition" => dapp_definition, updatable;
                }
            })
            .globalize()
        }

        pub fn retrieve_metadata(
            prism_splitter_component: Global<PrismSplitterV2>,
        ) -> (String, String, UncheckedUrl) {
        
            let market_name: String =     
                prism_splitter_component
                .get_metadata("market_name")
                .unwrap_or(Some("".to_string()))
                .unwrap_or("".to_string());
            let market_symbol: String = 
                prism_splitter_component
                .get_metadata("market_symbol")
                .unwrap_or(Some("".to_string()))
                .unwrap_or("".to_string());
            let market_icon: UncheckedUrl = 
                prism_splitter_component
                .get_metadata("market_icon")
                .unwrap_or(Some(UncheckedUrl::of("https://www.prismterminal.com/assets/glowlogo.svg")))
                .unwrap_or(UncheckedUrl::of("https://www.prismterminal.com/assets/glowlogo.svg"));
        
            (market_name, market_symbol, market_icon)
        }

        pub fn create_pool_manager_proof(&mut self) -> Proof {
            self.pool_manager_vault
            .authorize_with_amount(
                dec!(1), 
                || {
                    self.pool_manager_access_controller.create_proof()
                }
            )
        }

        pub fn withdraw_pool_manager_badge(&mut self) -> FungibleBucket {
            self.pool_manager_vault.take(dec!(1))
        }

        // First set the natural log of the implied rate here.
        // We also set optional inital anchor rate as the there isn't an anchor rate 
        // yet until we have the implied rate.
        // The initial anchor rate is determined by a guess on the interest rate 
        // which trading will be most capital efficient.
        pub fn set_initial_ln_implied_rate(
            &mut self, 
            initial_rate_anchor: PreciseDecimal,
        ) {
            assert_eq!(
                self.market_state.last_ln_implied_rate, 
                PreciseDecimal::ZERO,
                "Initial Ln Implied Rate has already been set"
            );

            let time_to_expiry = self.time_to_expiry();

            let rate_scalar = 
                calc_rate_scalar(
                    self.market_state.scalar_root,
                    time_to_expiry
                );

            let market_compute = 
                MarketCompute {
                    rate_scalar,
                    rate_anchor: initial_rate_anchor,
                };

            let new_implied_rate =
                self.get_ln_implied_rate( 
                        time_to_expiry,
                        market_compute,
                ); 

            self.market_state.last_ln_implied_rate = new_implied_rate;
        }

        pub fn get_market_implied_rate(&mut self) -> PreciseDecimal {
            self.market_state.last_ln_implied_rate.exp().unwrap()
        }
        
        pub fn get_vault_reserves(&self) -> PoolVaultReserves {
            let pool_reserves = self.pool_component.get_vault_amounts();

            let pt_amount = 
                pool_reserves
                .get::<ResourceAddress>(&self.market_info.pt_address)
                .unwrap_or(&Decimal::ZERO);

            let underlying_asset_amount =
                pool_reserves
                .get::<ResourceAddress>(&self.market_info.underlying_asset_address)
                .unwrap_or(&Decimal::ZERO);

            PoolVaultReserves {
                total_pt_amount: *pt_amount,
                total_underlying_asset_amount: *underlying_asset_amount,
            }
        }

        pub fn get_market_state(&mut self) -> MarketState {
            let pool_reserve_vaults = self.get_vault_reserves();

            let market_state = MarketState {
                total_pt: pool_reserve_vaults.total_pt_amount,
                total_asset: pool_reserve_vaults.total_underlying_asset_amount,
                initial_rate_anchor: self.market_state.initial_rate_anchor,
                scalar_root: self.market_state.scalar_root,
                last_ln_implied_rate: self.market_state.last_ln_implied_rate,
            };

            return market_state 
        }

        /// Adds liquidity to pool reserves.
        /// 
        /// # Arguments
        ///
        /// * `asset_buckets`: [`FungibleBucket`] - A fungible bucket of Asset token supply.
        /// * `pt_bucket`: [`FungibleBucket`] - A fungible bucket of principal token supply.
        ///
        /// # Returns
        /// 
        /// * [`Bucket`] - A bucket of `pool_unit`.
        /// * [`Option<Bucket>`] - An optional bucket of any remainder token.
        pub fn add_liquidity(
            &mut self, 
            pt_bucket: FungibleBucket,
            asset_bucket: FungibleBucket, 
        ) -> (
            FungibleBucket, 
            Option<FungibleBucket>, 
        ) {
            self.assert_market_not_expired();

            let pool_manager_proof = self.create_pool_manager_proof();

            LocalAuthZone::push(pool_manager_proof);

            let (pool_unit, remainder) = 
                self.pool_component
                    .contribute(
                        (pt_bucket.into(), asset_bucket.into())
                    );
                
            LocalAuthZone::drop_proofs();

            // Initialize Market State if not already initialized
            if self.market_state.last_ln_implied_rate.is_zero() {

                //-----------------------------------------------------------------------
                // STATE CHANGES
                //-----------------------------------------------------------------------

                self.set_initial_ln_implied_rate(
                    self.market_state.initial_rate_anchor
                );

                //-----------------------------------------------------------------------
                // STATE CHANGES
                //-----------------------------------------------------------------------

            };

            return (pool_unit, remainder)
        }

        /// Redeems pool units for the underlying pool assets.
        /// 
        /// # Arguments
        ///
        /// * `pool_units`: [`FungibleBucket`] - A fungible bucket of `pool_units` tokens to
        /// to redeem for underlying pool assets. 
        ///
        /// # Returns
        /// 
        /// * [`Bucket`] - A bucket of PT.
        /// * [`Bucket`] - A bucket of Asset tokens.
        pub fn remove_liquidity(
            &mut self, 
            pool_units: FungibleBucket
        ) -> (FungibleBucket, FungibleBucket) {
            let (pt_bucket, asset_bucket) = 
                self.pool_component
                    .redeem(pool_units.into());

            return (pt_bucket, asset_bucket)
        }

        /// Swaps the given PT for Asset tokens.
        /// 
        /// # Arguments
        ///
        /// * `pt_bucket`: [`FungibleBucket`] - A fungible bucket of PT tokens to
        /// to swap for Asset. 
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of Asset tokens.
        pub fn swap_exact_pt_for_asset(
            &mut self, 
            pt_bucket: FungibleBucket
        ) -> FungibleBucket {
            self.assert_market_not_expired();
            self.assert_market_is_active();

            let pt_amount_in = pt_bucket.amount();
        
            assert_eq!(
                pt_bucket.resource_address(), 
                self.market_info.pt_address
            );
            assert_eq!(pt_bucket.is_empty(), false);

            let time_to_expiry = self.time_to_expiry();

            // Calcs the rate scalar and rate anchor with the current market state
            let market_compute = self.compute_market(time_to_expiry);

            // Calcs the the swap
            let (
                asset_to_account,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade( 
                        pt_bucket.amount().checked_neg().unwrap(), 
                        time_to_expiry,
                        &market_compute,
                    );  

            let all_in_exchange_rate = 
                pt_bucket.amount()
                .checked_div(asset_to_account)
                .expect("[swap_exact_pt_for_asset] Overflow in all in exchange rate");

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            let pool_manager_proof = self.create_pool_manager_proof();
            LocalAuthZone::push(pool_manager_proof);
            self.pool_component.protected_deposit(pt_bucket.into());
            

            let owed_asset_bucket = 
                self.pool_component.protected_withdraw(
                    self.market_info.underlying_asset_address, 
                    asset_to_account, 
                    WithdrawStrategy::Rounded(RoundingMode::ToNearestMidpointToEven)
                );

            LocalAuthZone::drop_proofs();

            self.update_pool_stat(
                trading_fees,
                net_asset_fee_to_reserve,
                total_fees
            );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );

            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;


            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            let side = 
                if new_implied_rate > trade_implied_rate {
                    "Long Yield" 
                } else {
                    "Short Yield"
                };

            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.pt_address,
                    sell_size: pt_amount_in,
                    resource_bought: self.market_info.underlying_asset_address,
                    buy_size: owed_asset_bucket.amount(),
                    trade_volume: pt_amount_in,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap_or(PreciseDecimal::ZERO),
                    new_implied_rate: new_implied_rate.exp().unwrap_or(PreciseDecimal::ZERO),
                    output: owed_asset_bucket.amount(),
                    local_id: None,
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            owed_asset_bucket
        }

        /// Swaps the given PT for Asset tokens.
        ///
        /// # Arguments
        ///
        /// * `asset_bucket`: [`FungibleBucket`] - A fungible bucket of Asset tokens to
        /// swap for PT.
        /// * `desired_pt_amount`: [`Decimal`] - The amount of PT the user
        /// wants.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of PT.
        /// * [`FungibleBucket`] - A bucket of any remaining Asset tokens.
        /// 
        /// Notes:
        /// I believe it needs to be calculated this way because formula for trades is easier 
        /// based on PT being swapped in/ou but not for Assets.
        /// 
        /// Challengers have room for improvements to approximate required Asset better such that it equals
        /// the Asset sent in. 
        pub fn swap_exact_asset_for_pt(
            &mut self, 
            mut asset_bucket: FungibleBucket, 
            desired_pt_amount: Decimal
        ) -> (FungibleBucket, FungibleBucket) {
            self.assert_market_not_expired();
            self.assert_market_is_active();
            
            assert_eq!(
                asset_bucket.resource_address(), 
                self.market_info.underlying_asset_address
            );
            assert_eq!(asset_bucket.is_empty(), false);

            let time_to_expiry = self.time_to_expiry();

            // Calcs the rate scalar and rate anchor with the current market state
            // Important to calculate this before trade happens to ensure 
            // interest rate continuity and set the rate anchor.
            // Important for 3 reasons:
            // 1. Price Consistency:
            // Without interest rate continuity, the implied interest rate could jump suddenly between trades
            // For example, if two users make similar trades close in time, they should get similar rates
            // The rate_anchor adjustment ensures that each trade starts from the last established market rate
            // 2. Market Stability:
            // Interest rates in the market should change smoothly based on supply and demand
            // Sudden jumps in interest rates could:
            // Create arbitrage opportunities
            // Discourage trading due to unpredictable rates
            // Lead to market manipulation
            // 3. Fair Price Discovery: Consider this example:
            // Initial state:
            // - Market implied rate: 5%
            // - User A wants to trade 100 PT
            
            // Without continuity:
            // - The rate might reset arbitrarily
            // - User A's trade might suddenly see a different base rate
            // - The price impact wouldn't solely reflect their trade size
            
            // With continuity:
            // - The trade starts from the 5% rate
            // - Any change in rate is due to the trade's size
            // - The price impact is predictable and fair
            
            // 4. Predictable Slippage:
            // Traders need to estimate their trade's impact
            // Interest rate continuity ensures that:
            // The starting point is known (lastImpliedRate)
            // The price impact is solely from their trade size
            // Slippage calculations are reliable

            let market_compute = 
                self.compute_market(time_to_expiry);

            // Calcs the swap
            let (
                required_asset_amount,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade( 
                    desired_pt_amount,
                    time_to_expiry,
                    &market_compute,
                );

            // Might need to be required_asset_amount / desired_pt_amount
            let all_in_exchange_rate =
                desired_pt_amount
                .checked_div(required_asset_amount)
                .expect("[swap_exact_asset_for_pt] Overflow in all in exchange rate");

            // Only need to take the required Asset, return the rest.
            let required_asset_bucket = 
                asset_bucket.take(required_asset_amount);

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------
            let pool_manager_proof = self.create_pool_manager_proof();
            LocalAuthZone::push(pool_manager_proof);
            self.pool_component.protected_deposit(required_asset_bucket.into());
            
            let owed_pt_bucket = 
                self.pool_component.protected_withdraw(
                    self.market_info.pt_address, 
                    desired_pt_amount, 
                    WithdrawStrategy::Rounded(RoundingMode::ToNearestMidpointToEven)
                );
            LocalAuthZone::drop_proofs();

            // Saves the new implied rate of the trade.
            self.update_pool_stat(
                trading_fees,
                net_asset_fee_to_reserve,
                total_fees
            );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );

            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;


            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            let side = 
                if new_implied_rate > trade_implied_rate {
                    "Long Yield"
                } else {
                    "Short Yield"
                };

            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.underlying_asset_address,
                    sell_size: required_asset_amount,
                    resource_bought: self.market_info.pt_address,
                    buy_size: owed_pt_bucket.amount(),
                    trade_volume: owed_pt_bucket.amount(),
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap_or(PreciseDecimal::ZERO),
                    new_implied_rate: new_implied_rate.exp().unwrap_or(PreciseDecimal::ZERO),
                    output: owed_pt_bucket.amount(),
                    local_id: None,
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            (owed_pt_bucket, asset_bucket)
        }   

        /// Swaps the given Asset token for YT (Buying YT)
        /// 
        /// # Arguments
        ///
        /// * `bucket`: [`FungibleBucket`] - A fungible bucket of Asset tokens to
        /// swap for YT.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of YT.
        /// 
        /// Note:
        pub fn swap_exact_asset_for_yt(
            &mut self, 
            mut asset_bucket: FungibleBucket,
            guess_amount_to_swap_in: Decimal,
            optional_yt_bucket: Option<NonFungibleBucket>,
        )  -> NonFungibleBucket {
            self.assert_market_not_expired();
            self.assert_market_is_active();
        
            assert_eq!(
                asset_bucket.resource_address(),
                self.market_info.underlying_asset_address
            );
            assert_eq!(asset_bucket.is_empty(), false);

            let asset_amount = asset_bucket.amount();

            let time_to_expiry = self.time_to_expiry();
            let market_compute = self.compute_market(time_to_expiry);
            
            let (
                asset_to_borrow,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade(
                    guess_amount_to_swap_in.checked_neg().unwrap(), 
                    time_to_expiry, 
                    &market_compute, 
                );

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------
            let pool_manager_proof = self.create_pool_manager_proof();
            LocalAuthZone::push(pool_manager_proof);
            let asset_to_flash_swap = 
                self.pool_component.protected_withdraw(
                    asset_bucket.resource_address(), 
                    asset_to_borrow,
                    WithdrawStrategy::Rounded(RoundingMode::ToNearestMidpointToEven)
                );

            // Combined asset
            asset_bucket.put(asset_to_flash_swap);

            let (
                yt_to_return, 
                yt_amount_diff, 
                pt_bucket_to_pay_back
            ) = self.handle_optional_yt_bucket(
                optional_yt_bucket, 
                asset_bucket
            );

            let all_in_exchange_rate_asset_to_yt =
                asset_amount
                .checked_div(yt_amount_diff)
                .expect("[swap_exact_asset_for_yt] Overflow in all in exchange rate");

            // All in exchange rate in terms of PT
            let all_in_exchange_rate =
                Decimal::ONE
                .checked_div(
                    Decimal::ONE
                    .checked_sub(all_in_exchange_rate_asset_to_yt)
                    .unwrap()
                )
                .map(|x| 
                    if x.is_negative() {
                        dec!(1)
                    } else {
                        x
                    }
                )
                .unwrap_or(Decimal::ONE);

            let pt_amount_to_pay_back = pt_bucket_to_pay_back.amount();
            self.pool_component.protected_deposit(pt_bucket_to_pay_back.into());
            LocalAuthZone::drop_proofs();

            self.update_pool_stat(
                trading_fees,
                net_asset_fee_to_reserve,
                total_fees
            );

            let new_implied_rate =
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );
    
            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;

            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            let side = 
                if new_implied_rate > trade_implied_rate {
                    "Long Yield"
                } else {
                    "Short Yield"
                };

            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.underlying_asset_address,
                    sell_size: asset_amount,
                    resource_bought: self.market_info.yt_address,
                    buy_size: yt_amount_diff,
                    trade_volume: pt_amount_to_pay_back,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap_or(PreciseDecimal::ZERO),
                    new_implied_rate: new_implied_rate.exp().unwrap_or(PreciseDecimal::ZERO),
                    output: yt_amount_diff,
                    local_id: Some(yt_to_return.non_fungible_local_id()),
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            yt_to_return

        }

        /// Swaps the given YT for Asset tokens (Selling YT):
        ///
        /// 1. Seller sends YT into the swap contract.
        /// 2. Contract borrows an equivalent amount of PT from the pool.
        /// 3. The YTs and PTs are used to redeem Asset.
        /// 4. Contract calculates the required Asset to swap back to PT.
        /// 5. A portion of the Asset is sold to the pool for PT to return the amount from step 2.
        /// 6. The remaining Asset is sent to the seller.
        ///
        /// # Arguments
        ///
        /// * `yt_bucket`: [`FungibleBucket`] - A fungible bucket of Asset tokens to
        /// swap for YT.
        /// * `amount_yt_to_swap_in`: [Decimal] - Amount of YT to swap in.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of Asset.
        /// * [`Option<NonFungibleBucket>`] - A bucket of YT if not all were used.
        /// * [`Option<FungibleBucket>`] - A bucket of unused Asset.
        pub fn swap_exact_yt_for_asset(
            &mut self, 
            yt_bucket: NonFungibleBucket,
            amount_yt_to_swap_in: Decimal,
        ) 
            -> (
                FungibleBucket, 
                Option<NonFungibleBucket>,
            ) 
        {
            self.assert_market_not_expired();
            self.assert_market_is_active();

            assert_eq!(yt_bucket.resource_address(), self.market_info.yt_address);
            assert_eq!(yt_bucket.is_empty(), false);
            assert!(amount_yt_to_swap_in > Decimal::ZERO);
            assert_eq!(yt_bucket.amount(), Decimal::ONE);
            
            let data: YieldTokenData = yt_bucket.non_fungible().data();
            
            assert!(
                data.yt_amount >= amount_yt_to_swap_in,
                "Insufficient YT Amount"
            );

            let pt_to_withdraw = amount_yt_to_swap_in;

            let time_to_expiry = self.time_to_expiry();
            let market_compute = self.compute_market(time_to_expiry);

            let (
                asset_owed_for_pt_flash_swap,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade(
                    pt_to_withdraw,
                    time_to_expiry,
                    &market_compute,
                );

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            let pool_manager_proof = self.create_pool_manager_proof();
            LocalAuthZone::push(pool_manager_proof);
            let withdrawn_pt_bucket = 
                self.pool_component.protected_withdraw(
                    self.market_info.pt_address, 
                    pt_to_withdraw, 
                    WithdrawStrategy::Rounded(RoundingMode::ToNearestMidpointToEven)
                );

            // Combine PT and YT to redeem Asset
            let (
                mut redeemed_asset_bucket, 
                optional_yt_bucket,
                optional_pt_bucket,
            ) = self.get_prism_splitter_component()
                    .redeem(
                        withdrawn_pt_bucket, 
                        yt_bucket, 
                        amount_yt_to_swap_in
                    );

            // Potentially temporary to ensure the method break.
            // Would imply that no asset is returned if redeemed_asset_bucket is minimum.
            let adjusted_asset_owed_for_pt_flash_swap = 
                asset_owed_for_pt_flash_swap
                .min(redeemed_asset_bucket.amount());
        
            let asset_owed = 
                redeemed_asset_bucket
                .take(adjusted_asset_owed_for_pt_flash_swap);

            // Temporary - not even sure if we should calculate effective implied rate
            let yt_exchange_rate =
                redeemed_asset_bucket.amount()
                .checked_div(amount_yt_to_swap_in)
                .expect("[swap_exact_yt_for_asset] Overflow in yt exchange rate");

            let all_in_exchange_rate =
                Decimal::ONE
                .checked_div(
                    Decimal::ONE
                    .checked_sub(yt_exchange_rate)
                    .unwrap()
                )
                .map(|x| 
                    if x.is_negative() {
                        dec!(1)
                    } else {
                        x
                    }
                )
                .unwrap_or(Decimal::ONE);

            self.pool_component.protected_deposit(asset_owed.into());
            

            // Any excess PT is paid back, pool always wins.
            if let Some(excess_pt_bucket) = optional_pt_bucket {
                self.pool_component.protected_deposit(excess_pt_bucket);
            }

            LocalAuthZone::drop_proofs();

            self.update_pool_stat(
                trading_fees,
                net_asset_fee_to_reserve,
                total_fees
            );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );

            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;

            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            let side = 
                if new_implied_rate > trade_implied_rate {
                    "Long Yield"
                } else {
                    "Short Yield"
                };

            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            let local_id = 
                optional_yt_bucket
                .as_ref()
                .map(
                    |bucket| 
                    bucket.non_fungible_local_id()
                );

            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.yt_address,
                    sell_size: amount_yt_to_swap_in,
                    resource_bought: self.market_info.underlying_asset_address,
                    buy_size: redeemed_asset_bucket.amount(),
                    trade_volume: pt_to_withdraw,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap_or(PreciseDecimal::ONE),
                    new_implied_rate: new_implied_rate.exp().unwrap_or(PreciseDecimal::ONE),
                    output: redeemed_asset_bucket.amount(),
                    local_id: local_id
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            (redeemed_asset_bucket, optional_yt_bucket)
        }

        pub fn compute_market(
            &self,
            time_to_expiry: i64
        ) -> MarketCompute {

            let pool_vault_reserves = self.get_vault_reserves();

            let proportion = calc_proportion(
                Decimal::ZERO,
                pool_vault_reserves.total_pt_amount,
                pool_vault_reserves.total_underlying_asset_amount
            );

            let rate_scalar = calc_rate_scalar(
                self.market_state.scalar_root, 
                time_to_expiry
            );

            let rate_anchor = calc_rate_anchor(
                self.market_state.last_ln_implied_rate,
                proportion,
                time_to_expiry,
                rate_scalar
            )
            .expect("InvalidExchangeRate");

            MarketCompute {
                rate_scalar,
                rate_anchor,
            }
        }

        /// Calculates the the trade based on the direction of the trade.
        /// 
        /// This method retrieves the exchange rate, 
        fn calc_trade(
            &mut self,
            net_pt_amount: Decimal,
            time_to_expiry: i64,
            market_compute: &MarketCompute,
        ) -> (
            Decimal,
            PreciseDecimal,
            PreciseDecimal,
            PreciseDecimal,
            PreciseDecimal,
         ) {

            let resource_divisibility = 
                self.get_resource_divisibility();

            let pool_vault_reserves = self.get_vault_reserves();

            let proportion = 
                calc_proportion(
                    net_pt_amount,
                    pool_vault_reserves.total_pt_amount,
                    pool_vault_reserves.total_underlying_asset_amount,
                );
            
            // Calcs exchange rate based on size of the trade (change)
            let pre_fee_exchange_rate = 
                calc_exchange_rate(
                    proportion,
                    market_compute.rate_anchor,
                    market_compute.rate_scalar
                )
                .expect("InvalidExchangeRate");
            
            // Retrieve amount returned by applying the exchange rate
            // against asset swapped in (before fees are applied)
            let pre_fee_amount = 
                PreciseDecimal::from(net_pt_amount)
                .checked_div(pre_fee_exchange_rate)
                .and_then(
                    |amount|
                    amount.checked_neg()
                )
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::ToNearestMidpointToEven
                    )
                )
                .expect("OverflowError");

            let total_fees = 
                calc_fee(
                    self.market_fee.fee_rate,
                    time_to_expiry,
                    net_pt_amount,
                    pre_fee_exchange_rate,
                    pre_fee_amount
                )
                .expect("InvalidExchangeRate");

            // Fee allocated to the asset reserve
            // Portion of fees kept in the pool as additional liquidity
            // Helps maintain pool stability
            // Provides incentive for liquidity providers
            // Acts as a buffer against impermanent loss
            // Grows the pool's reserves over time
            let net_asset_fee_to_reserve =
                total_fees
                .checked_mul(self.market_fee.reserve_fee_percent)
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::ToNearestMidpointToEven
                    )
                )
                .expect("OverflowError");

            // Trading fee allocated to the reserve based on the direction
            // of the trade.
            // Main protocol revenue
            // Compensates for market making risk
            // Helps prevent market manipulation
            // Creates a cost for frequent trading
            let trading_fees = 
                total_fees
                .checked_sub(net_asset_fee_to_reserve)
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::ToNearestMidpointToEven
                    )
                )
                .expect("OverflowError");

            let net_amount = 
            // If this is [swap_exact_pt_to_asset] then pre_fee_asset_to_account is negative and
            // fee is positive so it actually adds to the net_asset_to_account.
                pre_fee_amount
                .checked_sub(trading_fees)
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::ToNearestMidpointToEven
                    )
                )
                .expect("OverflowError");

            // Net amount can be negative depending on direciton of the trade.
            // However, we want to have net amount to be positive to be able to 
            // perform the asset swap.
            let net_amount = if net_amount.is_negative() {
                // Asset ---> PT
                net_amount
                .checked_add(net_asset_fee_to_reserve)
                .and_then(|result| result.checked_abs())
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::ToNearestMidpointToEven
                    )
                )
                .expect("OverflowError")
            
            } else {
                // PT ---> Asset
                net_amount
                .checked_sub(net_asset_fee_to_reserve)
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::ToNearestMidpointToEven
                    )
                )
                .expect("OverflowError")
            };

            let net_amount =                 
                Decimal::try_from(net_amount)
                .ok()
                .unwrap();

            (
                net_amount,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            )
        }

        fn handle_optional_yt_bucket(
            &mut self,
            optional_yt_bucket: Option<NonFungibleBucket>,
            asset_bucket: FungibleBucket
        ) -> (NonFungibleBucket, Decimal, FungibleBucket) {

            let initial_yt_data = 
                optional_yt_bucket
                .as_ref()
                .map(|yt_bucket| {
                    yt_bucket.non_fungible::<YieldTokenData>().data()
                });
        
            let (
                pt_to_pay_back, 
                yt_to_return
            ) = self.get_prism_splitter_component()
                    .tokenize(
                        asset_bucket, 
                        optional_yt_bucket
                    );
        
            let new_yt_data = 
                yt_to_return.non_fungible::<YieldTokenData>().data();
        
            let diff = match initial_yt_data {
                Some(initial_data) => {
                    new_yt_data.yt_amount
                        .checked_sub(initial_data.yt_amount)
                        .expect("YT underlying asset amount should increase")
                },
                None => new_yt_data.yt_amount,
            };
        
            (yt_to_return, diff, pt_to_pay_back)
        }

        fn update_pool_stat(
            &mut self,
            trading_fees: PreciseDecimal,
            net_asset_fee_to_reserve: PreciseDecimal,
            total_fees: PreciseDecimal
        ) {
            let updated_trading_fees_collected= 
                self.pool_stat.trading_fees_collected
                .checked_add(trading_fees)
                .unwrap();

            let updated_reserve_fees_collected =
                self.pool_stat.reserve_fees_collected
                .checked_add(net_asset_fee_to_reserve)
                .unwrap();

            let updated_total_fees_collected =
               self.pool_stat.total_fees_collected
               .checked_add(total_fees)
               .unwrap();
            
            self.pool_stat.trading_fees_collected = updated_trading_fees_collected;
            self.pool_stat.reserve_fees_collected = updated_reserve_fees_collected;
            self.pool_stat.total_fees_collected = updated_total_fees_collected;
        }

        /// Retrieves current market implied rate.
        fn get_ln_implied_rate(
            &mut self, 
            time_to_expiry: i64, 
            market_compute: MarketCompute,
        ) -> PreciseDecimal {

            let pool_vault_reserves = 
                self.get_vault_reserves();

            let proportion = 
                calc_proportion(
                    Decimal::ZERO,
                    pool_vault_reserves.total_pt_amount,
                    pool_vault_reserves.total_underlying_asset_amount,
                );

            let exchange_rate = 
                calc_exchange_rate(
                    proportion,
                    market_compute.rate_anchor,
                    market_compute.rate_scalar
                )
                .expect("[get_ln_implied_rate] InvalidExchangeRate");

            // exchangeRate >= 1 so its ln >= 0
            let ln_exchange_rate = 
                // adjusted_exchange_rate
                exchange_rate
                .ln()
                .expect("[get_ln_implied_rate] Natural log of exchange rate should be positive");

            let ln_implied_rate = 
                ln_exchange_rate.checked_mul(PERIOD_SIZE)
                .and_then(|result| 
                    result.checked_div(time_to_expiry)
                )
                .expect("[get_ln_implied_rate] Overflow in ln implied rate");

            ln_implied_rate
        }

        fn all_in_exchange_rate_to_implied_rate(
            &self,
            exchange_rate: Decimal,
            time_to_expiry: i64,
        ) -> Decimal {

            // ln_exchange_rate
            exchange_rate
                .pow(
                    PERIOD_SIZE
                    .checked_div(time_to_expiry)
                    .expect("[all_in_exchange_rate_to_implied_rate] Overflow in implied rate calculation")
                )
                .and_then(|result| result.checked_sub(Decimal::ONE))
                .expect("[all_in_exchange_rate_to_implied_rate] Exchange rate is negative")
        }

        pub fn time_to_expiry(&self) -> i64 {
            self.market_info.maturity_date.to_instant().seconds_since_unix_epoch 
                - Clock::current_time_rounded_to_seconds().seconds_since_unix_epoch
        }

        fn current_time(&self) -> UtcDateTime {
            let current_time_instant = 
                Clock::current_time(TimePrecisionV2::Second);

            UtcDateTime::from_instant(
                &current_time_instant
            )
            .ok()
            .expect("[current_time] Failed to convert instant to UTC date time")
        }

        fn get_prism_splitter_component(&mut self) -> Global<PrismSplitterV2> {
            self.prism_splitter_component.into()
        }

        fn get_resource_divisibility(&self) -> u8 {
            ResourceManager::from(
                self.market_info.underlying_asset_address
            )
            .resource_type()
            .divisibility()
            .unwrap()
        }

        /// Checks whether maturity has lapsed
        pub fn check_maturity(&self) -> bool {
            Clock::current_time_comparison(
                self.market_info.maturity_date.to_instant(), 
                TimePrecision::Second, 
                TimeComparisonOperator::Gte
            )
        }

        fn assert_market_not_expired(&self) {
            assert_ne!(
                self.check_maturity(), 
                true, 
                "Market has reached its maturity"
            )
        }

        fn assert_market_is_active(&self) {
            assert_eq!(
                self.market_is_active, 
                true, 
                "Market is not active"
            )
        }

        pub fn change_maturity_date(
            &mut self,
            new_maturity_date: UtcDateTime
        ) {
            self.market_info.maturity_date = new_maturity_date;
            Runtime::global_component().set_metadata(
                "maturity_date", 
                new_maturity_date.to_string()
            );
        }

        pub fn change_market_status(
            &mut self,
            status: bool,
        ) {
            self.market_is_active = status;
        }

        // Maybe have two methods, one to have force override and another to use with get_ln_implied_rate.
        pub fn force_change_last_implied_rate(
            &mut self,
            last_implied_rate: PreciseDecimal
        ) {
            self.market_state.last_ln_implied_rate = last_implied_rate;
        }

        pub fn change_scalar_root(
            &mut self,
            scalar_root: Decimal
        ) {
            self.market_state.scalar_root = scalar_root;

        }

        pub fn change_prism_splitter(
            &mut self,
            prism_splitter: ComponentAddress
        ) {
            self.prism_splitter_component = prism_splitter;
        }

        pub fn change_pool_component(
            &mut self,
            pool_component: Global<TwoResourcePool>
        ) {
            self.pool_component = pool_component;
        }
    }
}


