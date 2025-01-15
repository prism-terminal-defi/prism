use scrypto::prelude::*;
use scrypto_math::*;
use crate::structs::*;
use yield_tokenizer::structs::YieldTokenData;
use prism_calculations::liquidity_curve::*;
use crate::events::*;

/// 365 days in seconds
const PERIOD_SIZE: Decimal = dec!(31536000);

#[blueprint]
#[events(InstantiateAMMEvent, SwapEvent)]
mod yield_amm {
    // The associated YieldRefractor package and component which is used to verify associated PT, YT, and 
    // Asset asset. It is also used to perform YT <---> Asset swaps.
    extern_blueprint! {
        // "package_sim1p4nhxvep6a58e88tysfu0zkha3nlmmcp6j8y5gvvrhl5aw47jfsxlt",
        // Stokenet
        "package_tdx_2_1pkdn5z6yfkwnwjfwkg2es3yxufe7pqrq2xjan6agvgm06s0mpxv0kq",
        YieldRefractor {
            fn tokenize(
                &mut self, 
                amount: FungibleBucket,
                optional_yt_bucket: Option<NonFungibleBucket>,
            ) -> (FungibleBucket, NonFungibleBucket);
            fn redeem(
                &mut self, 
                principal_token: FungibleBucket, 
                yield_token: NonFungibleBucket,
                yt_amount_to_redeem: Decimal,
            ) -> 
                (
                    FungibleBucket, 
                    Option<NonFungibleBucket>,
                    Option<FungibleBucket>,
                );
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

    enable_function_auth! {
        instantiate_yield_amm => rule!(allow_all);
        retrieve_metadata => rule!(allow_all);
    }

    enable_method_auth! {
        methods {
            set_initial_ln_implied_rate => restrict_to: [OWNER];
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
            change_market_status => restrict_to: [OWNER];
            force_change_last_implied_rate => restrict_to: [OWNER];
            change_scalar_root => restrict_to: [OWNER];
        }
    }
    pub struct YieldAMM {
        /// The native pool component which manages liquidity reserves. 
        pub pool_component: Global<TwoResourcePool>,
        pub yield_tokenizer_component: Global<YieldRefractor>,
        /// The initial scalar root of the market. This is used to calculate
        /// the scalar value. It determins the slope of the curve and becomes
        /// less sensitive as the market approaches maturity. The higher the 
        /// scalar value the more flat the curve is, the lower the scalar value
        /// the more steep the curve is.
        pub market_fee: MarketFee,
        pub market_state: MarketState,
        pub market_info: MarketInfo,
        pub market_is_active: bool,
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
            scalar_root: Decimal,
            market_fee_input: MarketFeeInput,
            yield_tokenizer_address: ComponentAddress
        ) -> Global<YieldAMM> {
            assert!(scalar_root > Decimal::ZERO);
            assert!(market_fee_input.fee_rate > Decimal::ZERO);
            assert!(
                market_fee_input.reserve_fee_percent > Decimal::ZERO 
                && market_fee_input.reserve_fee_percent < Decimal::ONE
            );
            // Should check whether market has expired

            let (address_reservation, component_address) =
                Runtime::allocate_component_address(YieldAMM::blueprint_id());
            let global_component_caller_badge =
                NonFungibleGlobalId::global_caller_badge(component_address);
        
            let yield_tokenizer_component: Global<YieldRefractor> = yield_tokenizer_address.into();

            let (market_name, market_symbol, market_icon) =
                Self::retrieve_metadata(yield_tokenizer_component);

            let underlying_asset_address = 
                yield_tokenizer_component.underlying_asset();
            
            let (pt_address, yt_address) = 
                yield_tokenizer_component.protocol_resources();

            let maturity_date = yield_tokenizer_component.maturity_date();

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
                .unwrap(); 
            
            ResourceManager::from(pool_unit_address)
                .set_metadata("name", format!("LP {}", market_name));
            ResourceManager::from(pool_unit_address)
                .set_metadata("symbol", format!("lp{}", market_symbol));
            ResourceManager::from(pool_unit_address)
                .set_metadata("icon_url", market_icon);

            let market_state = MarketState {
                total_pt: Decimal::ZERO,
                total_asset: Decimal::ZERO,
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
                    .unwrap()
                );

            let market_fee = MarketFee {
                fee_rate,
                reserve_fee_percent: market_fee_input.reserve_fee_percent
            };

            Runtime::emit_event(
                InstantiateAMMEvent {
                    market_state: market_state.clone(),
                    market_fee: market_fee.clone()
                }
            );

            Self {
                pool_component,
                yield_tokenizer_component,
                market_fee,
                market_state,
                market_info: market_info.clone(),
                market_is_active: true,
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
                    "market_name" => "Yield Tokenizer", updatable;
                    "icon_url" => "", updatable;
                    "description" => "", updatable;
                    "market_resources" => vec![
                        market_info.underlying_asset_address,
                        market_info.pt_address,
                        market_info.yt_address,
                    ], locked;
                    "pool_unit" => market_info.pool_unit_address, locked;
                    "maturity_date" => maturity_date.to_string(), locked;
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
                    change_market_status => Free, updatable;
                    force_change_last_implied_rate => Free, updatable;
                    change_scalar_root => Free, updatable;
                }
            })
            .with_address(address_reservation)
            .globalize()
        }

        pub fn retrieve_metadata(
            yield_tokenizer_component: Global<YieldRefractor>,
        ) -> (String, String, UncheckedUrl) {
        
            let market_name: String =     
                yield_tokenizer_component
                .get_metadata("market_name")
                .unwrap_or(Some("".to_string()))
                .unwrap_or("".to_string());
            let market_symbol: String = 
                yield_tokenizer_component
                .get_metadata("market_symbol")
                .unwrap_or(Some("".to_string()))
                .unwrap_or("".to_string());
            let market_icon: UncheckedUrl = 
                yield_tokenizer_component
                .get_metadata("market_icon")
                .unwrap_or(Some(UncheckedUrl::of("https://placeholder.com")))
                .unwrap_or(UncheckedUrl::of("https://placeholder.com"));
        
            (market_name, market_symbol, market_icon)
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

            let market_state = self.get_market_state();

            let rate_scalar = 
                calc_rate_scalar(
                    market_state.scalar_root,
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
        
        pub fn get_vault_reserves(&self) -> IndexMap<ResourceAddress, Decimal> {
            self.pool_component.get_vault_amounts()
        }

        fn update_market_state(&mut self) {
            let reserves = 
                self.pool_component
                    .get_vault_amounts();

            self.market_state.total_pt = reserves[0];
            self.market_state.total_asset = reserves[1];
        }

        pub fn get_market_state(&mut self) -> MarketState {
            self.update_market_state();

            return self.market_state.clone()
        }

        /// Adds liquidity to pool reserves.
        /// 
        /// # Arguments
        ///
        /// * `asset_buckets`: [`FungibleBucket`] - A fungible bucket of Asset token supply.
        /// * `principal_token`: [`FungibleBucket`] - A fungible bucket of principal token supply.
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

            let (pool_unit, remainder) = 
                self.pool_component
                    .contribute(
                        (
                            pt_bucket.into(),
                            asset_bucket.into(), 
                        )
                    );

            self.update_market_state();

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
                
            self.update_market_state();

            return (pt_bucket, asset_bucket)
        }

        /// Swaps the given PT for Asset tokens.
        /// 
        /// # Arguments
        ///
        /// * `principal_token`: [`FungibleBucket`] - A fungible bucket of PT tokens to
        /// to swap for Asset. 
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of Asset tokens.
        pub fn swap_exact_pt_for_asset(
            &mut self, 
            principal_token: FungibleBucket
        ) -> FungibleBucket {
            self.assert_market_not_expired();
            self.assert_market_is_active();

            let pt_amount_in = principal_token.amount();
        
            assert_eq!(
                principal_token.resource_address(), 
                self.market_info.pt_address
            );
            
            info!("[swap_exact_pt_for_asset] Calculating state of the market...");
            
            let market_state = self.get_market_state();
            
            let time_to_expiry = self.time_to_expiry();

            info!(
                "[swap_exact_pt_for_asset] Market State: {:?}", 
                market_state
            );

            // Calcs the rate scalar and rate anchor with the current market state
            info!("[swap_exact_pt_for_asset] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            info!("[swap_exact_pt_for_asset] Calculating trade...");
            // Calcs the the swap
            let (
                asset_to_account,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade( 
                        principal_token.amount().checked_neg().unwrap(), 
                        time_to_expiry,
                        &market_state,
                        &market_compute,
                    );  

            info!(
                "[swap_exact_pt_for_asset] Net Asset to Return: {:?}", 
                asset_to_account
            );

            let all_in_exchange_rate = 
                principal_token.amount()
                .checked_div(asset_to_account)
                .unwrap();

            info!(
                "[swap_exact_pt_for_asset] All-in Exchange rate: {:?}", 
                all_in_exchange_rate
            );

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            self.pool_component.protected_deposit(principal_token.into());

            let owed_asset_bucket = self.pool_component.protected_withdraw(
                self.market_info.underlying_asset_address, 
                asset_to_account, 
                WithdrawStrategy::Rounded(RoundingMode::ToZero)
            );

            info!("[swap_exact_pt_for_asset] Updating implied rate...");
            info!(
                "[swap_exact_pt_for_asset] Implied Rate Before Trade: {:?}",
                self.market_state.last_ln_implied_rate.exp().unwrap()
            );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );

            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;

            let side = if new_implied_rate > trade_implied_rate {
                "Long Yield" 
            } else {
                "Short Yield"
            };

            info!(
                "[swap_exact_pt_for_asset] Implied Rate After Trade: {:?}",
                new_implied_rate.exp().unwrap()
            );

            info!(
                "[swap_exact_pt_for_asset] New Implied Rate Movement Decrease: {:?}",
                self.market_state.last_ln_implied_rate
                .checked_sub(new_implied_rate)
                .unwrap()
                .is_negative()
            );

            self.update_market_state();
            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            info!("[swap_exact_pt_for_asset] All In Exchange Rate: {:?}", all_in_exchange_rate);
            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            let trade_volume = 
                self.yield_tokenizer_component
                .get_pt_redemption_value(pt_amount_in);

            info!("[swap_exact_pt_for_asset] Effective Implied Rate: {:?}", effective_implied_rate);
            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.pt_address,
                    sell_size: pt_amount_in,
                    resource_bought: self.market_info.underlying_asset_address,
                    buy_size: owed_asset_bucket.amount(),
                    trade_volume: trade_volume,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap(),
                    new_implied_rate: new_implied_rate.exp().unwrap(),
                    output: owed_asset_bucket.amount(),
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

            let time_to_expiry = self.time_to_expiry();
            let market_state = self.get_market_state();

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

            info!("[swap_exact_asset_for_pt] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Calcs the swap
            info!("[swap_exact_asset_for_pt] Calculating trade...");
            let (
                required_asset_amount,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade( 
                    desired_pt_amount,
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            // Assert the amount of Asset sent in is at least equal to the required
            // Asset needed for the desired PT amount.
            assert!(
                asset_bucket.amount() >= required_asset_amount,
                "Asset amount: {:?}
                Required asset amount: {:?}",
                asset_bucket.amount(),
                required_asset_amount
            );

            let all_in_exchange_rate =
                desired_pt_amount
                .checked_div(required_asset_amount)
                .unwrap();

            info!(
                "[swap_exact_asset_for_pt] All-in Exchange rate: {:?}", 
                desired_pt_amount.checked_div(required_asset_amount).unwrap()
            );

            // Only need to take the required Asset, return the rest.
            let required_asset_bucket = asset_bucket.take(required_asset_amount);

            info!(
                "[swap_exact_asset_for_pt] Required Asset: {:?}", 
                required_asset_bucket.amount()
            );

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------
            
            self.pool_component.protected_deposit(required_asset_bucket.into());

            
            let owed_pt_bucket = 
                self.pool_component.protected_withdraw(
                    self.market_info.pt_address, 
                    desired_pt_amount, 
                    WithdrawStrategy::Rounded(RoundingMode::ToZero)
                );

            // Saves the new implied rate of the trade.
            info!("[swap_exact_yt_for_asset] Updating implied rate...");
            info!(
                "[swap_exact_yt_for_asset] Implied Rate Before Trade: {:?}",
                self.market_state.last_ln_implied_rate.exp().unwrap()
            );

            info!("[swap_exact_yt_for_asset] 
                    New Total PT: {:?}
                    New Total Asset: {:?}",
                    market_state.total_pt,
                    market_state.total_asset
                );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );

            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;

            let side = if new_implied_rate > trade_implied_rate {
                "Long Yield"
            } else {
                "Short Yield"
            };

            info!(
                "[swap_exact_yt_for_asset] Implied Rate After Trade: {:?}",
                new_implied_rate.exp().unwrap()
            );

            self.update_market_state();
            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            info!(
                "[swap_exact_asset_for_pt] Output PT Redemption Value: {:?}",
                self.yield_tokenizer_component.get_pt_redemption_value(owed_pt_bucket.amount())
            );


            info!("[swap_exact_pt_for_asset] All in Exchange Rate: {:?}", all_in_exchange_rate);               
            info!("[swap_exact_pt_for_asset] Effective Implied Rate: {:?}", effective_implied_rate);
            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.underlying_asset_address,
                    sell_size: required_asset_amount,
                    resource_bought: self.market_info.pt_address,
                    buy_size: owed_pt_bucket.amount(),
                    trade_volume: required_asset_amount,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap(),
                    new_implied_rate: new_implied_rate.exp().unwrap(),
                    output: owed_pt_bucket.amount(),
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            info!("[swap_exact_asset_for_pt] Owed PT: {:?}", owed_pt_bucket.amount());
            info!("[swap_exact_asset_for_pt] Remaining Asset: {:?}", asset_bucket.amount());

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
        ) 
        -> NonFungibleBucket 
        {
            self.assert_market_not_expired();
            self.assert_market_is_active();

            assert_eq!(
                asset_bucket.resource_address(),
                self.market_info.underlying_asset_address
            );

            let asset_amount = asset_bucket.amount();

            let time_to_expiry = self.time_to_expiry();
            let market_state = self.get_market_state();
            let market_compute = self.compute_market(
                // Can't market state be a reference when calc compute?
                market_state.clone(),
                time_to_expiry
            );

            info!("[swap_exact_asset_for_yt] Guess PT In: {:?}", guess_amount_to_swap_in);

            let required_asset_to_borrow =
                guess_amount_to_swap_in
                .checked_sub(asset_bucket.amount())
                .unwrap(); 

            info!("[swap_exact_asset_for_yt] Required Asset to borrow: {:?}", required_asset_to_borrow);
            
            let (
                asset_to_borrow,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade(
                    guess_amount_to_swap_in.checked_neg().unwrap(), 
                    time_to_expiry, 
                    &market_state,
                    &market_compute, 
                );

            info!("[flash_swap] Asset To Borrow Amount: {:?}", asset_to_borrow);
            
            // Not sure or don't remember when asset_to_borrow needs to be >= required_asset_to_borrow
            assert!(
                asset_to_borrow >= required_asset_to_borrow 
            );


            let all_in_exchange_rate_asset_to_pt = 
                asset_to_borrow
                // asset_to_borrow + asset_amount should = PT Guess In or thereabouts.
                // The reason we don't use guess PT In is because we actually pay back more
                // in PT for the trade since we get more assets for the PT Guess in than the
                // Algo provideded.
                .checked_div(
                    asset_to_borrow
                    .checked_add(asset_amount)
                    .unwrap()
                ) 
                .unwrap();

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            let asset_to_flash_swap = 
                self.pool_component.protected_withdraw(
                    asset_bucket.resource_address(), 
                    asset_to_borrow,
                    WithdrawStrategy::Exact
                );

            info!(
                "[flash_swap] Asset to Flash Swap Amount: {:?}", 
                asset_to_flash_swap.amount()
            );

            // Combined asset
            asset_bucket.put(asset_to_flash_swap);

            let (
                yt_to_return, 
                yt_amount_diff, 
                pt_to_pay_back
            ) = {
                let initial_yt_data = 
                    optional_yt_bucket
                    .as_ref()
                    .map(|yt_bucket| {
                        yt_bucket.non_fungible::<YieldTokenData>().data()
                });
            
                let (
                    pt_to_pay_back, 
                    yt_to_return
                ) = self.yield_tokenizer_component
                    .tokenize(asset_bucket, optional_yt_bucket);
            
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
            };

            info!(
                "[swap_exact_asset_for_yt] PT Returned from Splitting: {:?}", 
                pt_to_pay_back.amount()
            );

            info!(
                "[swap_exact_asset_for_yt] YT Returned from Splitting: {:?}", 
                yt_amount_diff
            );

            let all_in_exchange_rate_asset_to_yt =
                asset_amount
                .checked_div(yt_amount_diff)
                .unwrap();

            let all_in_exchange_rate =
                Decimal::ONE
                .checked_div(
                    Decimal::ONE
                    .checked_sub(all_in_exchange_rate_asset_to_yt)
                    .unwrap()
                )
                .unwrap();

            info!(
                "[swap_exact_asset_for_yt] All-in Exchange Rate of Asset/PT: {:?}",
                all_in_exchange_rate_asset_to_pt  
            );
            
            info!(
                "[swap_exact_asset_for_yt] All-in Exchange Rate of YT/Asset: {:?}",
                all_in_exchange_rate_asset_to_yt
            );

            info!(
                "[swap_exact_asset_for_yt] Combined Exchange Rate: {:?}",
                all_in_exchange_rate_asset_to_yt
                .checked_add(all_in_exchange_rate_asset_to_pt)
                .unwrap()
            );

            // May remove after testing
            assert!(
                pt_to_pay_back.amount() >= guess_amount_to_swap_in
            );

            self.pool_component.protected_deposit(pt_to_pay_back.into());
        
            let new_implied_rate =
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );
    
            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;

            let side = if new_implied_rate > trade_implied_rate {
                "Long Yield"
            } else {
                "Short Yield"
            };

            self.update_market_state();
            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            info!("[swap_exact_pt_for_asset] All in Exchange Rate: {:?}", all_in_exchange_rate);               
            info!("[swap_exact_pt_for_asset] Effective Implied Rate: {:?}", effective_implied_rate);
            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.underlying_asset_address,
                    sell_size: asset_amount,
                    resource_bought: self.market_info.yt_address,
                    buy_size: yt_amount_diff,
                    trade_volume: asset_amount,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap(),
                    new_implied_rate: new_implied_rate.exp().unwrap(),
                    output: yt_amount_diff,
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
        /// * `yield_token`: [`FungibleBucket`] - A fungible bucket of Asset tokens to
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
            yield_token: NonFungibleBucket,
            amount_yt_to_swap_in: Decimal,
        ) 
            -> (
                FungibleBucket, 
                Option<NonFungibleBucket>,
            ) 
        {
            self.assert_market_not_expired();
            self.assert_market_is_active();

            assert_eq!(
                yield_token.resource_address(), 
                self.market_info.yt_address
            );
            
            // Need to borrow the same amount of PT as YT to redeem Asset
            let data: YieldTokenData = yield_token.non_fungible().data();
            
            assert!(
                data.yt_amount >= amount_yt_to_swap_in,
                "Insufficient YT Amount"
            );

            let pt_to_withdraw = amount_yt_to_swap_in;

            info!(
                "[swap_exact_yt_for_asset] Simulating trade to calculate
                Required Asset to pay back for withdrawn PT"
            );
            info!("[swap_exact_yt_for_asset] Calculating state of the market...");
            let market_state = self.get_market_state();
            let time_to_expiry = self.time_to_expiry();

            info!("[swap_exact_yt_for_asset] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            info!("[swap_exact_yt_for_asset] Calculating trade...");
            let (
                asset_owed_for_pt_flash_swap,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            ) = self.calc_trade(
                    pt_to_withdraw,
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            info!(
                "[swap_exact_yt_for_asset] 
                Required Asset to pay back (Net Asset Returned): {:?}
                for desired PT amount: {:?}",
                asset_owed_for_pt_flash_swap,
                pt_to_withdraw
            );

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            info!("[swap_exact_yt_for_asset] Withdrawing desired PT to combine with YT...");
            let withdrawn_pt = 
                self.pool_component.protected_withdraw(
                    self.market_info.pt_address, 
                    pt_to_withdraw, 
                    WithdrawStrategy::Exact
                );

            info!("[swap_exact_yt_for_asset] Redeeming Asset from PT & SY...");    
            // Combine PT and YT to redeem Asset
            let (
                mut asset_bucket, 
                optional_yt_bucket,
                optional_pt_bucket,
            ) = self.yield_tokenizer_component
                    .redeem(
                        withdrawn_pt, 
                        yield_token, 
                        amount_yt_to_swap_in
                    );

            info!(
                "[swap_exact_yt_for_asset] Asset Redeemed: {:?}",
                asset_bucket.amount()
            );
            
            info!(
                "[swap_exact_yt_for_asset] Excess YT: {:?}",
                optional_yt_bucket.as_ref().map(|bucket| bucket.non_fungible::<YieldTokenData>().data())
            );
            
            info!(
                "[swap_exact_yt_for_asset] Excess PT: {:?}",
                optional_pt_bucket.as_ref().map(|bucket| bucket.amount().clone())
            );
        
            let asset_owed = 
                asset_bucket
                .take(asset_owed_for_pt_flash_swap);

            let yt_exchange_rate =
                asset_bucket.amount()
                .checked_div(amount_yt_to_swap_in)
                .unwrap();

            info!(
                "[swap_exact_yt_for_asset] All-in Exchange rate of Asset/PT: {:?}",
                asset_owed_for_pt_flash_swap.checked_div(pt_to_withdraw).unwrap()
            );

            info!(
                "[swap_exact_yt_for_asset] All-in Exchange rate of Asset/YT: {:?}",
                yt_exchange_rate
            );

            let all_in_exchange_rate =
                Decimal::ONE
                .checked_div(
                    Decimal::ONE
                    .checked_sub(yt_exchange_rate)
                    .unwrap()
                )
                .unwrap();

            self.pool_component.protected_deposit(asset_owed.into());
            // Any excess PT is paid back, pool always wins.
            if let Some(excess_pt_bucket) = optional_pt_bucket {
                self.pool_component.protected_deposit(excess_pt_bucket);
            }

            info!("[swap_exact_yt_for_asset] Updating implied rate...");
            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                );

            let trade_implied_rate = 
                self.market_state.last_ln_implied_rate;

            let side = if new_implied_rate > trade_implied_rate {
                "Long Yield"
            } else {
                "Short Yield"
            };

            self.update_market_state();
            self.market_state.last_ln_implied_rate = new_implied_rate;

            //-----------------------------------------------------------------------
            // STATE CHANGES
            //-----------------------------------------------------------------------

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------
            let effective_implied_rate =
                self.all_in_exchange_rate_to_implied_rate(
                    all_in_exchange_rate, 
                    time_to_expiry
                );

            info!("[swap_exact_pt_for_asset] All in Exchange Rate: {:?}", all_in_exchange_rate);               
            info!("[swap_exact_pt_for_asset] Effective Implied Rate: {:?}", effective_implied_rate);
            Runtime::emit_event(
                SwapEvent {
                    timestamp: self.current_time(),
                    resource_sold: self.market_info.yt_address,
                    sell_size: amount_yt_to_swap_in,
                    resource_bought: self.market_info.underlying_asset_address,
                    buy_size: asset_bucket.amount(),
                    trade_volume: amount_yt_to_swap_in,
                    side: side.to_string(),
                    exchange_rate_before_fees: pre_fee_exchange_rate,
                    exchange_rate_after_fees: all_in_exchange_rate,
                    reserve_fees: net_asset_fee_to_reserve,
                    trading_fees,
                    total_fees,
                    effective_implied_rate,
                    trade_implied_rate: trade_implied_rate.exp().unwrap(),
                    new_implied_rate: new_implied_rate.exp().unwrap(),
                    output: asset_bucket.amount(),
                }
            );

            //-----------------------------------------------------------------------
            // EVENTS
            //-----------------------------------------------------------------------

            info!(
                "[swap_exact_yt_for_asset] Asset Returned: {:?}", 
                asset_bucket.amount()
            );

            (asset_bucket, optional_yt_bucket)
        }

        pub fn compute_market(
            &self,
            market_state: MarketState,
            time_to_expiry: i64
        ) -> MarketCompute {

            let proportion = calc_proportion(
                dec!(0),
                market_state.total_pt,
                market_state.total_asset
            );

            let rate_scalar = calc_rate_scalar(
                market_state.scalar_root, 
                time_to_expiry
            );

            let rate_anchor = calc_rate_anchor(
                market_state.last_ln_implied_rate,
                proportion,
                time_to_expiry,
                rate_scalar
            )
            .expect("InvalidExchangeRate");

            info!(
                "[compute_market] 
                Pre-trade Proportion: {:?}
                Rate Scalar: {:?}
                Rate Anchor: {:?}",
                proportion,
                rate_scalar,
                rate_anchor
            );

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
            market_state: &MarketState,
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

            let proportion = 
                calc_proportion(
                    net_pt_amount,
                    market_state.total_pt,
                    market_state.total_asset
                );

            info!("[calc_trade] Trade Proportion: {:?}", proportion);
            
            // Calcs exchange rate based on size of the trade (change)
            let pre_fee_exchange_rate = 
                calc_exchange_rate(
                    proportion,
                    market_compute.rate_anchor,
                    market_compute.rate_scalar
                )
                .expect("InvalidExchangeRate");
            
            info!(
                "[calc_trade] Exchange Rate Before Fees: {:?}", 
                pre_fee_exchange_rate
            );

            // Retrieve amount returned by applying the exchange rate
            // against asset swapped in (before fees are applied)
            let pre_fee_amount = 
                net_pt_amount
                .checked_div(pre_fee_exchange_rate)
                .and_then(
                    |amount|
                    amount.checked_neg()
                )
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::AwayFromZero
                    )
                )
                .expect("OverflowError");

            info!(
                "[calc_trade] Amount to Return Before Fees: {:?}", 
                pre_fee_amount
            );

            let total_fees = 
                calc_fee(
                    self.market_fee.fee_rate,
                    time_to_expiry,
                    net_pt_amount,
                    pre_fee_exchange_rate,
                    pre_fee_amount
                )
                .expect("InvalidExchangeRate");

            info!("[calc_trade] Base Fee: {:?}", total_fees);

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
                        RoundingMode::AwayFromZero
                    )
                )
                .expect("OverflowError");

            info!(
                "[calc_trade] Reserve Fee: {:?}", 
                net_asset_fee_to_reserve
            );

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
                        RoundingMode::AwayFromZero
                    )
                )
                .expect("OverflowError");

            info!("[Calc_trade] Trading Fee: {:?}", trading_fees);

            let net_amount = 
            // If this is [swap_exact_pt_to_asset] then pre_fee_asset_to_account is negative and
            // fee is positive so it actually adds to the net_asset_to_account.
                pre_fee_amount
                .checked_sub(trading_fees)
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::AwayFromZero
                    )
                )
                .expect("OverflowError");

            // Net amount can be negative depending on direciton of the trade.
            // However, we want to have net amount to be positive to be able to 
            // perform the asset swap.
            let net_amount = if net_amount < PreciseDecimal::ZERO {
                // Asset ---> PT
                info!("[calc_trade] Trade Direction: Asset ---> PT");
                net_amount
                .checked_add(net_asset_fee_to_reserve)
                .and_then(|result| result.checked_abs())
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::AwayFromZero
                    )
                )
                .expect("OverflowError")
            
            } else {
                // PT ---> Asset
                info!("[calc_trade] Trade Direction: PT ---> Asset");
                net_amount
                .checked_sub(net_asset_fee_to_reserve)
                .and_then(
                    |amount|
                    amount.checked_round(
                        resource_divisibility, 
                        RoundingMode::AwayFromZero
                    )
                )
                .expect("OverflowError")
            };

            let net_amount =                 
                Decimal::try_from(net_amount)
                .ok()
                .unwrap();

            info!(
                "[calc_trade] 
                Amount to Return After Fees: {:?}", 
                net_amount
            );
    

            (
                net_amount,
                pre_fee_exchange_rate,
                total_fees,
                net_asset_fee_to_reserve,
                trading_fees,
            )

        }

        /// Retrieves current market implied rate.
        fn get_ln_implied_rate(
            &mut self, 
            time_to_expiry: i64, 
            market_compute: MarketCompute,
        ) -> PreciseDecimal {

            let proportion = 
                calc_proportion(
                    dec!(0),
                    self.get_vault_reserves()[0],
                    self.get_vault_reserves()[1],
                );

            let exchange_rate = 
                calc_exchange_rate(
                    proportion,
                    market_compute.rate_anchor,
                    market_compute.rate_scalar
                )
                .expect("InvalidExchangeRate");

            info!("[get_ln_implied_rate] Exchange Rate: {:?}", exchange_rate);

            // exchangeRate >= 1 so its ln >= 0
            let ln_exchange_rate = 
                // adjusted_exchange_rate
                exchange_rate
                .ln()
                .unwrap();

            let ln_implied_rate = 
                ln_exchange_rate.checked_mul(PERIOD_SIZE)
                .and_then(|result| result.checked_div(time_to_expiry))
                .unwrap();

            ln_implied_rate
        }

        fn all_in_exchange_rate_to_implied_rate(
            &self,
            exchange_rate: Decimal,
            time_to_expiry: i64,
        ) -> Decimal {

            // let ln_exchange_rate = exchange_rate.ln().unwrap();

            // ln_exchange_rate
            exchange_rate
                .pow(
                    PERIOD_SIZE
                    .checked_div(time_to_expiry)
                    .unwrap()
                )
                .and_then(|result| result.checked_sub(Decimal::ONE))
                .unwrap()
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
            .unwrap()
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
    }
}


