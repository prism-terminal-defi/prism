use scrypto::prelude::*;
use scrypto_math::*;
use common::structs::*;
use off_ledger::liquidity_curve::*;
use crate::events::*;

/// 365 days in seconds
const PERIOD_SIZE: Decimal = dec!(31536000);

/// The transient flash loan NFT which has `NonFungibleData` to track the resource 
/// and amount of the flash loan. The data here must be enforced to ensure that
/// the flash loan NFT can be burnt and therefore guarantee repayment.
#[derive(ScryptoSbor, NonFungibleData)]
pub struct FlashLoanReceipt {
    pub resource: ResourceAddress,
    pub amount: Decimal,
}

#[blueprint]
#[events(InstantiateAMMEvent)]
mod yield_amm {
    // The associated YieldTokenizer package and component which is used to verify associated PT, YT, and 
    // LSU asset. It is also used to perform YT <---> LSU swaps.
    extern_blueprint! {
        "package_sim1p4nhxvep6a58e88tysfu0zkha3nlmmcp6j8y5gvvrhl5aw47jfsxlt",
        YieldTokenizer {
            fn tokenize_yield(
                &mut self, 
                amount: FungibleBucket
            ) -> (FungibleBucket, NonFungibleBucket);
            fn redeem(
                &mut self, 
                principal_token: FungibleBucket, 
                yield_token: NonFungibleBucket,
            ) -> 
                (
                    FungibleBucket, 
                    Option<NonFungibleBucket>,
                    Option<FungibleBucket>,
                );
            fn pt_address(&self) -> ResourceAddress;
            fn yt_address(&self) -> ResourceAddress;
            fn underlying_asset(&self) -> ResourceAddress;
            fn maturity_date(&self) -> UtcDateTime;
            fn asset_addresses(&self) -> (ResourceAddress, ResourceAddress);
        }
    }

    enable_function_auth! {
        instantiate_yield_amm => rule!(allow_all);
    }

    enable_method_auth! {
        methods {
            set_initial_ln_implied_rate => restrict_to: [OWNER];
            get_market_implied_rate => PUBLIC;
            get_vault_reserves => PUBLIC;
            get_market_state => PUBLIC;
            add_liquidity => PUBLIC;
            remove_liquidity => PUBLIC;
            swap_exact_pt_for_lsu => PUBLIC;
            swap_exact_lsu_for_pt => PUBLIC;
            swap_exact_lsu_for_yt => PUBLIC;
            swap_exact_yt_for_lsu => PUBLIC;
            swap_exact_yt_for_lsu2 => PUBLIC;
            compute_market => PUBLIC;
            time_to_expiry => PUBLIC;
            check_maturity => PUBLIC;
            change_market_status => restrict_to: [OWNER];
        }
    }
    pub struct YieldAMM {
        /// The native pool component which manages liquidity reserves. 
        pub pool_component: Global<TwoResourcePool>,
        pub yield_tokenizer_component: Global<YieldTokenizer>,
        /// The ResourceManager of the flash loan FlashLoanReceipt, which is used
        /// to ensure flash loans are repaid.
        pub flash_loan_rm: ResourceManager,
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
            owner_role_node: AccessRuleNode,
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
        
            let flash_loan_rm: ResourceManager = 
                ResourceBuilder::new_ruid_non_fungible::<FlashLoanReceipt>(OwnerRole::None)
                .metadata(metadata! {
                    init {
                        "name" => "Flash Loan FlashLoanReceipt", locked;
                    }
                })
                .mint_roles(mint_roles! {
                    minter => rule!(require(global_caller(component_address)));
                    minter_updater => rule!(deny_all);
                })
                .burn_roles(burn_roles! {
                    burner => rule!(require(global_caller(component_address)));
                    burner_updater => rule!(deny_all);
                })
                .deposit_roles(deposit_roles! {
                    depositor => rule!(deny_all);
                    depositor_updater => rule!(deny_all);
                })
                .create_with_no_initial_supply();

            let yield_tokenizer_component: Global<YieldTokenizer> = yield_tokenizer_address.into();

            let underlying_asset_address = 
                yield_tokenizer_component.underlying_asset();
            
            let (pt_address, yt_address) = 
                yield_tokenizer_component.asset_addresses();

            let maturity_date = yield_tokenizer_component.maturity_date();

            let owner_role = 
                OwnerRole::Updatable(AccessRule::from(owner_role_node.clone()));

            let combined_rule_node = 
                owner_role_node
                .or(AccessRuleNode::from(global_component_caller_badge));

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
                flash_loan_rm,
                market_fee,
                market_state,
                market_info,
                market_is_active: true,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .globalize()
        }

        // First set the natural log of the implied rate here.
        // We also set optional inital anchor rate as the there isn't an anchor rate 
        // yet until we have the implied rate.
        // The initial anchor rate is determined by a guess on the interest rate 
        // which trading will be most capital efficient.
        pub fn set_initial_ln_implied_rate(
            &mut self, 
            initial_rate_anchor: PreciseDecimal
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
                    rate_anchor: initial_rate_anchor
                };

            let new_implied_rate =
                self.get_ln_implied_rate( 
                        time_to_expiry,
                        market_compute,
                        market_state,
                    ); 

            self.market_state.last_ln_implied_rate = new_implied_rate;

            info!(
                "Implied Rate: {:?}", 
                self.market_state.last_ln_implied_rate.exp().unwrap()
            );
        }

        pub fn get_market_implied_rate(&mut self) -> PreciseDecimal {
            self.market_state.last_ln_implied_rate.exp().unwrap()
        }
        
        pub fn get_vault_reserves(&self) -> IndexMap<ResourceAddress, Decimal> {
            self.pool_component.get_vault_amounts()
        }

        pub fn get_market_state(&mut self) -> MarketState {
            let reserves = 
                self.pool_component.get_vault_amounts();

            let market_state = MarketState {
                total_pt: reserves[0],
                total_asset: reserves[1],
                scalar_root: self.market_state.scalar_root,
                last_ln_implied_rate: self.market_state.last_ln_implied_rate,
            };

            self.market_state = market_state;

            return self.market_state.clone()
        }

        /// Adds liquidity to pool reserves.
        /// 
        /// # Arguments
        ///
        /// * `lsu_tokens`: [`FungibleBucket`] - A fungible bucket of LSU token supply.
        /// * `principal_token`: [`FungibleBucket`] - A fungible bucket of principal token supply.
        ///
        /// # Returns
        /// 
        /// * [`Bucket`] - A bucket of `pool_unit`.
        /// * [`Option<Bucket>`] - An optional bucket of any remainder token.
        pub fn add_liquidity(
            &mut self, 
            mut pt_bucket: FungibleBucket,
            mut asset_bucket: FungibleBucket, 
        ) -> (
            Bucket, 
            Option<Bucket>, 
        ) {

            self.assert_market_not_expired();

            // Add initial liquidity to be 50/50?

            self.pool_component
                .contribute(
                    (
                        pt_bucket.into(),
                        asset_bucket.into(), 
                    )
                )
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
        /// * [`Bucket`] - A bucket of LSU tokens.
        pub fn remove_liquidity(
            &mut self, 
            pool_units: FungibleBucket
        ) -> (Bucket, Bucket) {
            self.pool_component
                .redeem(pool_units.into())
        }

        /// Swaps the given PT for LSU tokens.
        /// 
        /// # Arguments
        ///
        /// * `principal_token`: [`FungibleBucket`] - A fungible bucket of PT tokens to
        /// to swap for LSU. 
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of LSU tokens.
        pub fn swap_exact_pt_for_lsu(
            &mut self, 
            principal_token: FungibleBucket
        ) -> FungibleBucket {
            self.assert_market_not_expired();
        
            assert_eq!(
                principal_token.resource_address(), 
                self.market_info.pt_address
            );
            
            info!("[swap_exact_pt_for_lsu] Calculating state of the market...");
            
            let market_state = self.get_market_state();
            
            let time_to_expiry = self.time_to_expiry();
            info!("[swap_exact_lsu_for_pt] Time to expiry: {:?}", time_to_expiry);
            
            info!(
                "[swap_exact_pt_for_lsu] Market State: {:?}", 
                market_state
            );

            // Calcs the rate scalar and rate anchor with the current market state
            info!("[swap_exact_pt_for_lsu] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            info!("[swap_exact_pt_for_lsu] Calculating trade...");
            // Calcs the the swap
            let lsu_to_account = 
                self.calc_trade(
                    principal_token.amount().checked_neg().unwrap(), 
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            info!(
                "[swap_exact_pt_for_lsu] Net LSU to Return: {:?}", 
                lsu_to_account
            );

            info!(
                "[swap_exact_pt_for_lsu] All-in Exchange rate: {:?}", 
                principal_token.amount().checked_div(lsu_to_account).unwrap()
            );

            // *                    STATE CHANGES                       * //

            self.pool_component.protected_deposit(principal_token.into());

            let owed_lsu_bucket = self.pool_component.protected_withdraw(
                self.market_info.underlying_asset_address, 
                lsu_to_account, 
                WithdrawStrategy::Rounded(RoundingMode::ToZero)
            );

            info!("[swap_exact_pt_for_lsu] Updating implied rate...");
            info!(
                "[swap_exact_pt_for_lsu] Implied Rate Before Trade: {:?}",
                self.market_state.last_ln_implied_rate.exp().unwrap()
            );

            // let market_state = self.get_market_state();
            // let market_compute = 
            //     self.compute_market(
            //         market_state.clone(),
            //         time_to_expiry
            //     );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state
                );

            info!(
                "[swap_exact_pt_for_lsu] Implied Rate After Trade: {:?}",
                new_implied_rate.exp().unwrap()
            );

            // What does it mean when implied rate decrease/increase after a trade?
            info!(
                "[swap_exact_pt_for_lsu] New Implied Rate Movement Decrease: {:?}",
                self.market_state.last_ln_implied_rate
                .checked_sub(new_implied_rate)
                .unwrap()
                .is_negative()
            );

            self.market_state.last_ln_implied_rate = new_implied_rate;

            // *                    STATE CHANGES                       * //

            return owed_lsu_bucket.as_fungible()
        }

        /// Swaps the given PT for LSU tokens.
        ///
        /// # Arguments
        ///
        /// * `lsu_token`: [`FungibleBucket`] - A fungible bucket of LSU tokens to
        /// swap for PT.
        /// * `desired_pt_amount`: [`Decimal`] - The amount of PT the user
        /// wants.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of PT.
        /// * [`FungibleBucket`] - A bucket of any remaining LSU tokens.
        /// 
        /// Notes:
        /// I believe it needs to be calculated this way because formula for trades is easier 
        /// based on PT being swapped in/ou but not for LSUs.
        /// 
        /// Challengers have room for improvements to approximate required LSU better such that it equals
        /// the LSU sent in. 
        pub fn swap_exact_lsu_for_pt(
            &mut self, 
            mut lsu_token: FungibleBucket, 
            desired_pt_amount: Decimal
        ) -> (FungibleBucket, FungibleBucket) {
            self.assert_market_not_expired();
            
            assert_eq!(
                lsu_token.resource_address(), 
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

            info!("[swap_exact_lsu_for_pt] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Calcs the swap
            info!("[swap_exact_lsu_for_pt] Calculating trade...");
            let required_lsu = 
                self.calc_trade(
                    desired_pt_amount,
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            // Assert the amount of LSU sent in is at least equal to the required
            // LSU needed for the desired PT amount.
            assert!(
                lsu_token.amount() >= required_lsu,
                "Asset amount: {:?}
                Required asset amount: {:?}",
                lsu_token.amount(),
                required_lsu
            );

            info!(
                "[swap_exact_lsu_for_pt] All-in Exchange rate: {:?}", 
                desired_pt_amount.checked_div(required_lsu).unwrap()
            );

            // Only need to take the required LSU, return the rest.
            let required_lsu_bucket = lsu_token.take(required_lsu);

            info!(
                "[swap_exact_lsu_for_pt] Required LSU: {:?}", 
                required_lsu_bucket.amount()
            );

            // *                    STATE CHANGES                       * //
            
            self.pool_component.protected_deposit(required_lsu_bucket.into());

            
            let owed_pt_bucket = 
                self.pool_component.protected_withdraw(
                    self.market_info.pt_address, 
                    desired_pt_amount, 
                    WithdrawStrategy::Rounded(RoundingMode::ToZero)
                );

            // Saves the new implied rate of the trade.
            info!("[swap_exact_yt_for_lsu] Updating implied rate...");
            info!(
                "[swap_exact_yt_for_lsu] Implied Rate Before Trade: {:?}",
                self.market_state.last_ln_implied_rate
            );

            info!("[swap_exact_yt_for_lsu] 
                    New Total PT: {:?}
                    New Total Asset: {:?}",
                    market_state.total_pt,
                    market_state.total_asset
                );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state
                );

            info!(
                "[swap_exact_yt_for_lsu] Implied Rate After Trade: {:?}",
                new_implied_rate
            );

            info!(
                "[swap_exact_yt_for_lsu] New Implied Rate Movement Decrease: {:?}",
                self.market_state.last_ln_implied_rate
                .checked_sub(new_implied_rate)
                .unwrap()
                .is_negative()
            );

            self.market_state.last_ln_implied_rate = new_implied_rate;

            // *                    STATE CHANGES                       * //

            info!("[swap_exact_lsu_for_pt] Owed PT: {:?}", owed_pt_bucket.amount());
            info!("[swap_exact_lsu_for_pt] Remaining LSU: {:?}", lsu_token.amount());

            return (owed_pt_bucket.as_fungible(), lsu_token)
        }   

        /// Swaps the given LSU token for YT (Buying YT)
        /// 
        /// # Arguments
        ///
        /// * `bucket`: [`FungibleBucket`] - A fungible bucket of LSU tokens to
        /// swap for YT.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of YT.
        /// 
        /// Note: In practice, the way an amount of YT can be determined given an
        /// LSU is by calculating the price of PT and YT based on P(PT) + P(YT) = LSU
        /// relationship. However, doing so require complex approximation algorithm
        /// which isn't covered in this implementation.
        /// Note: Some small discrepencies with result from guesses, need to handle.
        pub fn swap_exact_lsu_for_yt(
            &mut self, 
            mut lsu_token: FungibleBucket,
            guess_amount_to_swap_in: Decimal,
        ) 
        -> NonFungibleBucket 
        {
            self.assert_market_not_expired();

            assert_eq!(
                lsu_token.resource_address(),
                self.market_info.underlying_asset_address
            );

            // There would be an algorithm to estimate the PT that can be
            // swapped for LSU to determine the price of PT as this would
            // determine the amount of LSU one can borrow and pay back.
            // let est_max_pt_in = dec!(0); 
            let time_to_expiry = self.time_to_expiry();
            let market_state = self.get_market_state();
            let market_compute = self.compute_market(
                // Can't market state be a reference when calc compute?
                market_state.clone(),
                time_to_expiry
            );

            let required_lsu_to_borrow =
                guess_amount_to_swap_in
                .checked_sub(lsu_token.amount())
                .unwrap(); 

            info!("[swap_exact_lsu_for_yt] Required LSU to borrow: {:?}", required_lsu_to_borrow);
            
            let asset_to_swap_amount = self.calc_trade(
                guess_amount_to_swap_in.checked_neg().unwrap(), 
                time_to_expiry, 
                &market_state,
                &market_compute, 
            );

            info!("[flash_swap] Asset To Borrow Amount: {:?}", asset_to_swap_amount);
            
            assert!(
                asset_to_swap_amount >= required_lsu_to_borrow 
            );

            let withdrawn_asset = self.pool_component.protected_withdraw(
                lsu_token.resource_address(), 
                asset_to_swap_amount, 
                WithdrawStrategy::Exact
            )
            .as_fungible();

            info!("[flash_swap] Withdrawn Asset Amount: {:?}", withdrawn_asset.amount());

            lsu_token.put(withdrawn_asset);

            let (principal_token, yield_token) = 
                self.yield_tokenizer_component.tokenize_yield(lsu_token);

            info!("[flash_swap] Principal Token Amount: {:?}", principal_token.amount());

            let yield_token_data: YieldTokenData = 
                yield_token
                .as_non_fungible()
                .non_fungible()
                .data();

            info!(
                "[swap_exact_lsu_for_yt] YT Amount: {:?}", 
                yield_token_data.underlying_lsu_amount
            );

            assert!(
                principal_token.amount() >= guess_amount_to_swap_in
            );

            self.pool_component.protected_deposit(principal_token.into());
        
            // Saves the new implied rate of the trade.
            self.market_state.last_ln_implied_rate = 
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state,
                );

            return yield_token

        }
        
        /// Swaps the given YT for LSU tokens (Selling YT):
        ///
        /// 1. Seller sends YT into the swap contract.
        /// 2. Contract borrows an equivalent amount of PT from the pool.
        /// 3. The YTs and PTs are used to redeem LSU.
        /// 4. Contract calculates the required LSU to swap back to PT.
        /// 5. A portion of the LSU is sold to the pool for PT to return the amount from step 2.
        /// 6. The remaining LSU is sent to the seller.
        ///
        /// # Arguments
        ///
        /// * `yield_token`: [`FungibleBucket`] - A fungible bucket of LSU tokens to
        /// swap for YT.
        /// * `amount_yt_to_swap_in`: [Decimal] - Amount of YT to swap in.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A bucket of LSU.
        /// * [`Option<NonFungibleBucket>`] - A bucket of YT if not all were used.
        /// * [`Option<FungibleBucket>`] - A bucket of unused LSU.
        pub fn swap_exact_yt_for_lsu(
            &mut self, 
            yield_token: NonFungibleBucket,
            amount_yt_to_swap_in: Decimal,
        ) 
        -> (
            FungibleBucket, 
            Option<NonFungibleBucket>, 
            Option<FungibleBucket>
        ) 
        {
            self.assert_market_not_expired();

            assert_eq!(
                yield_token.resource_address(), 
                self.market_info.yt_address
            );
            
            // Need to borrow the same amount of PT as YT to redeem LSU
            let data: YieldTokenData = yield_token.non_fungible().data();
            let underlying_lsu_amount = data.underlying_lsu_amount;
            
            assert!(
                underlying_lsu_amount >= amount_yt_to_swap_in
            );

            let pt_flash_loan_amount = amount_yt_to_swap_in;

            // Borrow equivalent amount of PT from the pool - enough to get LSU
            info!("[swap_exact_yt_for_lsu] Flash loan equal amount of PT to redeem underlying asset");
            let (pt_flash_loan, flash_loan_receipt) = 
                self.flash_loan(
                    self.market_info.pt_address, 
                    pt_flash_loan_amount
                );

            // Combine PT and YT to redeem LSU
            info!("[swap_exact_yt_for_lsu] Redeeming equivalent underlying asset from combined PT & YT");
            let (
                mut lsu_token, 
                optional_yt_bucket, 
                optional_pt_bucket
            ) = 
                self.yield_tokenizer_component
                    .redeem(
                        pt_flash_loan, 
                        yield_token, 
                    );

            info!(
                "[swap_exact_yt_for_lsu] Redeemed underlying asset amount: {:?}",
                lsu_token.amount()
            );

            // info!(
            //     "[swap_exact_yt_for_lsu] Excess YT: {:?}",
            //     option_yt_bucket.unwrap().amount()
            // );

            // Retrieve flash loan requirements to ensure enough can be swapped back to repay
            // the flash loan.
            let flash_loan_data: FlashLoanReceipt = 
                flash_loan_receipt
                .as_non_fungible()
                .non_fungible()
                .data();

            let desired_pt_amount = flash_loan_data.amount;
            
            info!("[swap_exact_yt_for_lsu] Calculating required LSU for PT to repay loan");
            info!("[swap_exact_yt_for_lsu] Calculating state of the market...");
            let market_state = self.get_market_state();
            let time_to_expiry = self.time_to_expiry();

            info!("[swap_exact_yt_for_lsu] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Portion of lsu is sold to the pool for PT to return the borrowed PT
            info!("[swap_exact_yt_for_lsu] Calculating trade...");
            let required_lsu = 
                self.calc_trade(
                    desired_pt_amount,
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            info!(
                "[swap_exact_yt_for_lsu] Required LSU (Net LSU to Return): {:?}", 
                required_lsu
            );

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate of LSU/PT: {:?}", 
                required_lsu.checked_div(desired_pt_amount).unwrap()
            );

            let required_lsu_bucket = 
                lsu_token.take(required_lsu);


            info!("[swap_exact_yt_for_lsu] Swapping LSU for PT...");
            let (
                pt_flash_loan_repay, 
                returned_lsu
            ) = self.swap_exact_lsu_for_pt(
                required_lsu_bucket, 
                desired_pt_amount
            );

            lsu_token.put(returned_lsu);
            
            info!("[swap_exact_yt_for_lsu] Repaying flash loan...");
            let optional_return_bucket = 
                self.flash_loan_repay(
                    pt_flash_loan_repay, 
                    flash_loan_receipt
                );

            info!("[swap_exact_yt_for_lsu] Updating implied rate...");
            info!(
                "[swap_exact_yt_for_lsu] Implied Rate Before Trade: {:?}",
                self.market_state.last_ln_implied_rate
            );

            info!("[swap_exact_yt_for_lsu] 
                New Total PT: {:?}
                New Total Asset: {:?}",
                market_state.total_pt,
                market_state.total_asset
                );

            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state
                );

            info!(
                "[swap_exact_yt_for_lsu] Implied Rate After Trade: {:?}",
                new_implied_rate
            );

            info!(
                "[swap_exact_yt_for_lsu] New Implied Rate Movement Decrease: {:?}",
                self.market_state.last_ln_implied_rate
                .checked_sub(new_implied_rate)
                .unwrap()
                .is_negative()
            );

            self.market_state.last_ln_implied_rate = new_implied_rate;


            info!(
                "[swap_exact_yt_for_lsu] Actual LSU Returned: {:?}", 
                lsu_token.amount()
            );

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate of LSU/YT: {:?}", 
                lsu_token.amount().checked_div(amount_yt_to_swap_in).unwrap()
            );
            
            return (
                lsu_token, 
                optional_yt_bucket, 
                optional_return_bucket
            )
        }

        pub fn swap_exact_yt_for_lsu2(
            &mut self, 
            yield_token: NonFungibleBucket,
            amount_yt_to_swap_in: Decimal,
        ) 
            -> (
                FungibleBucket, 
                Option<NonFungibleBucket>,
                Option<FungibleBucket>
            ) 
        {
            self.assert_market_not_expired();

            assert_eq!(
                yield_token.resource_address(), 
                self.market_info.yt_address
            );
            
            // Need to borrow the same amount of PT as YT to redeem LSU
            let data: YieldTokenData = yield_token.non_fungible().data();
            let underlying_lsu_amount = data.underlying_lsu_amount;
            
            assert!(
                underlying_lsu_amount >= amount_yt_to_swap_in
            );

            let pt_to_withdraw = amount_yt_to_swap_in;

            info!(
                "[swap_exact_yt_for_lsu] Simulating trade to calculate
                Required LSU to pay back for withdrawn PT"
            );
            info!("[swap_exact_yt_for_lsu] Calculating state of the market...");
            let market_state = self.get_market_state();
            let time_to_expiry = self.time_to_expiry();

            info!("[swap_exact_yt_for_lsu] Calculating market compute...");
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            info!("[swap_exact_yt_for_lsu] Calculating trade...");
            // Do we calc_trade before any assets are removed/added?
            let lsu_owed_for_pt_flash_swap = 
                self.calc_trade(
                // Make sure the signs are correct based on direction of the trade
                // Pretty sure this is positive as we are intending to withdraw PT
                // And figuring out how much LSU is required for the PT.
                    pt_to_withdraw,
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            info!(
                "[swap_exact_yt_for_lsu] 
                Required LSU to pay back (Net LSU Returned): {:?}
                for desired PT amount: {:?}",
                lsu_owed_for_pt_flash_swap,
                pt_to_withdraw
            );

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate of LSU/PT: {:?}",
                lsu_owed_for_pt_flash_swap.checked_div(pt_to_withdraw).unwrap()
            );

            // *                    STATE CHANGES                       * //

            info!("[swap_exact_yt_for_lsu] Withdrawing desired PT to combine with YT...");
            let withdrawn_pt = 
                self.pool_component.protected_withdraw(
                    self.market_info.pt_address, 
                    pt_to_withdraw, 
                    WithdrawStrategy::Exact
                );

            info!("[swap_exact_yt_for_lsu] Redeeming LSU from PT & SY...");    
            // Combine PT and YT to redeem LSU
            let (
                mut lsu_token, 
                optional_yt_bucket,
                optional_pt_bucket,
            ) = self.yield_tokenizer_component
                    .redeem(
                        withdrawn_pt.as_fungible(), 
                        yield_token, 
                    );
            
            info!(
                "[swap_exact_yt_for_lsu] LSU Redeemed: {:?}",
                lsu_token.amount()
            );

            
            info!(
                "[swap_exact_yt_for_lsu] Excess YT: {:?}",
                optional_yt_bucket.as_ref().map(|bucket| bucket.amount().clone())
            );
            
            info!(
                "[swap_exact_yt_for_lsu] Excess PT: {:?}",
                optional_pt_bucket.as_ref().map(|bucket| bucket.amount().clone())
            );
            

            // Do we need assertion here to make sure lsu token received is greater than lsu owed?
            // let lsu_owed_back_to_pool = 
            //     lsu_token
            //     .amount()
            //     .checked_sub(lsu_owed_for_pt_flash_swap)
            //     .unwrap();

            // Can LSU redeemed be less than LSU owed?
            let lsu_owed = 
                lsu_token
                .take(lsu_owed_for_pt_flash_swap);

            assert_eq!(lsu_owed.amount(), lsu_owed_for_pt_flash_swap);

            self.pool_component.protected_deposit(lsu_owed.into());

            info!("[swap_exact_yt_for_lsu] Updating implied rate...");
            info!(
                "[swap_exact_yt_for_lsu] Implied Rate Before Trade: {:?}",
                self.market_state.last_ln_implied_rate
            );

            info!("[swap_exact_yt_for_lsu] 
                    New Total PT: {:?}
                    New Total Asset: {:?}",
                    market_state.total_pt,
                    market_state.total_asset
                );

            // Do we need to update market_compute and market_state again?
            let new_implied_rate =    
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state
                );

            info!(
                "[swap_exact_yt_for_lsu] Implied Rate After Trade: {:?}",
                new_implied_rate
            );

            info!(
                "[swap_exact_yt_for_lsu] New Implied Rate Movement Decrease: {:?}",
                self.market_state.last_ln_implied_rate
                .checked_sub(new_implied_rate)
                .unwrap()
                .is_negative()
            );

            self.market_state.last_ln_implied_rate = new_implied_rate;

            // *                    STATE CHANGES                       * //

            info!(
                "[swap_exact_yt_for_lsu] LSU Returned: {:?}", 
                lsu_token.amount()
            );

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate for LSU/YT: {:?}", 
                lsu_token.amount().checked_div(amount_yt_to_swap_in).unwrap()
            );

            return (lsu_token, optional_yt_bucket, optional_pt_bucket)
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
            );

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
        ) -> Decimal {
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
                );

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
                .unwrap();

            info!(
                "[calc_trade] Amount to Return Before Fees: {:?}", 
                pre_fee_amount
            );

            let fee = 
                calc_fee(
                    self.market_fee.fee_rate,
                    time_to_expiry,
                    net_pt_amount,
                    pre_fee_exchange_rate,
                    pre_fee_amount
                );

            info!("[calc_trade] Base Fee: {:?}", fee);

            // Fee allocated to the asset reserve
            // Portion of fees kept in the pool as additional liquidity
            // Helps maintain pool stability
            // Provides incentive for liquidity providers
            // Acts as a buffer against impermanent loss
            // Grows the pool's reserves over time
            let net_asset_fee_to_reserve =
                fee
                .checked_mul(self.market_fee.reserve_fee_percent)
                .unwrap();

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
            let trading_fee = 
                fee
                .checked_sub(net_asset_fee_to_reserve)
                .unwrap();

            info!("[Calc_trade] Trading Fee: {:?}", trading_fee);

            let net_amount = 
            // If this is [swap_exact_pt_to_lsu] then pre_fee_lsu_to_account is negative and
            // fee is positive so it actually adds to the net_lsu_to_account.
                pre_fee_amount
                .checked_sub(trading_fee)
                .unwrap();

            info!(
                "[calc_trade] 
                Amount to Return After Trading Fees: {:?}", 
                net_amount
            );

            // Net amount can be negative depending on direciton of the trade.
            // However, we want to have net amount to be positive to be able to 
            // perform the asset swap.
            let net_amount = if net_amount < PreciseDecimal::ZERO {
                // LSU ---> PT
                info!("[calc_trade] Trade Direction: LSU ---> PT");
                net_amount
                .checked_add(net_asset_fee_to_reserve)
                .and_then(|result| result.checked_abs())
                .unwrap()
            
            } else {
                // PT ---> LSU
                info!("[calc_trade] Trade Direction: PT ---> LSU");
                net_amount
                .checked_sub(net_asset_fee_to_reserve)
                .unwrap()
            };

            return 
                Decimal::try_from(net_amount)
                .ok()
                .unwrap()
        }

        
        /// Takes a flash loan of a resource and amount from pool reserves.
        /// 
        /// This method mints a transient `FlashLoanReceipt` NFT which must be burnt.
        ///
        /// # Arguments
        ///
        /// * `resource`: [`ResourceAddress`] - The resource to borrow.
        /// * `amount`: [`Decimal`] - The amount to borrow.
        /// wants.
        ///
        /// # Returns
        ///
        /// * [`FungibleBucket`] - A fungible bucket of requested loan.
        /// * [`NonFungibleBucket`] - A non fungible bucket of the flash loan receipt NFT.
        /// 
        /// Note: This method is private due to the way implied rates are saved. 
        fn flash_loan(
            &mut self, 
            resource: ResourceAddress, 
            amount: Decimal
        ) -> (FungibleBucket, NonFungibleBucket) {
            
            let flash_loan_receipt = self.flash_loan_rm.mint_ruid_non_fungible(
                FlashLoanReceipt {
                    resource,
                    amount,
                }
            )
            .as_non_fungible();

            let flash_loan = self.pool_component.protected_withdraw(
                resource, 
                amount, 
                WithdrawStrategy::Rounded(RoundingMode::ToZero)
            )
            .as_fungible();
        
            return (flash_loan, flash_loan_receipt)
        }

        /// Repays flash loan
        ///
        /// # Arguments
        ///
        /// * `flash_loan`: [`FungibleBucket`] - A fungible bucket of the flash
        /// loan repayment.
        /// * `flash_loan_receipt`: [`NonFungibleBucket`] - A non fungible bucket
        /// of the flash loan receipt NFT.
        ///
        /// # Returns
        ///
        /// * [`Option<FungibleBucket>`] - An option fungible bucket of repayment 
        /// overages.
        fn flash_loan_repay(
            &mut self, 
            mut flash_loan: FungibleBucket, 
            flash_loan_receipt: NonFungibleBucket
        ) -> Option<FungibleBucket> {
            let mut flash_loan_receipt_data: FlashLoanReceipt = flash_loan_receipt.as_non_fungible().non_fungible().data();
            let flash_loan_repay = flash_loan.take(flash_loan_receipt_data.amount);
            flash_loan_receipt_data.amount -= flash_loan_repay.amount();

            assert_eq!(self.flash_loan_rm.address(), flash_loan_receipt.resource_address());
            assert_eq!(flash_loan.resource_address(), flash_loan_receipt_data.resource);
            assert_eq!(flash_loan_receipt_data.amount, Decimal::ZERO);

            self.pool_component.protected_deposit(flash_loan_repay.into());

            flash_loan_receipt.burn();

            return Some(flash_loan)
        }

        /// Retrieves current market implied rate.
        fn get_ln_implied_rate(
            &mut self, 
            time_to_expiry: i64, 
            market_compute: MarketCompute,
            market_state: MarketState
        ) -> PreciseDecimal {

            let proportion = 
                calc_proportion(
                    dec!(0),
                    // market_state.total_pt,
                    // market_state.total_asset
                    self.get_vault_reserves()[0],
                    self.get_vault_reserves()[1],
                );

            let exchange_rate = 
                calc_exchange_rate(
                    proportion,
                    market_compute.rate_anchor,
                    market_compute.rate_scalar
                );

            // exchangeRate >= 1 so its ln >= 0
            let ln_exchange_rate = 
                exchange_rate
                .ln()
                .unwrap();

            let ln_implied_rate = 
                ln_exchange_rate.checked_mul(PERIOD_SIZE)
                .and_then(|result| result.checked_div(time_to_expiry))
                .unwrap();

            return ln_implied_rate
        }

        pub fn time_to_expiry(&self) -> i64 {
            self.market_info.maturity_date.to_instant().seconds_since_unix_epoch 
                - Clock::current_time_rounded_to_seconds().seconds_since_unix_epoch
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

        fn assert_market_status(&self) {
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
    }
}


