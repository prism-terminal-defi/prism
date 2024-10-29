use scrypto::prelude::*;
use scrypto_math::*;
use common::structs::*;
use crate::liquidity_curve::*;

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
mod yield_amm {
    // The associated YieldTokenizer package and component which is used to verify associated PT, YT, and 
    // LSU asset. It is also used to perform YT <---> LSU swaps.
    // extern_blueprint! {
    //     "package_sim1p5n8qgf24qq7wxlxg7u2fyjfcrq860vsqnh9hv8neak98n40rz3hdv",
    //     YieldTokenizer {
    //         fn tokenize_yield(
    //             &mut self, 
    //             amount: FungibleBucket
    //         ) -> (FungibleBucket, NonFungibleBucket);
    //         fn redeem(
    //             &mut self, 
    //             principal_token: FungibleBucket, 
    //             yield_token: NonFungibleBucket,
    //             yt_redeem_amount: Decimal
    //         ) -> (FungibleBucket, Option<NonFungibleBucket>);
    //         fn pt_address(&self) -> ResourceAddress;
    //         fn yt_address(&self) -> ResourceAddress;
    //         fn underlying_resource(&self) -> ResourceAddress;
    //         fn maturity_date(&self) -> UtcDateTime;
    //     }
    // }

    extern_blueprint! {
        "package_sim1p5n8qgf24qq7wxlxg7u2fyjfcrq860vsqnh9hv8neak98n40rz3hdv",
        YieldTokenizer {
            fn tokenize_yield(
                &mut self, 
                amount: FungibleBucket
            ) -> (FungibleBucket, NonFungibleBucket);
            fn redeem(
                &mut self, 
                principal_token: FungibleBucket, 
                yield_token: NonFungibleBucket,
                yt_redeem_amount: Decimal
            ) -> (FungibleBucket, Option<NonFungibleBucket>);
            fn pt_address(&self) -> ResourceAddress;
            fn yt_address(&self) -> ResourceAddress;
            fn underlying_resource(&self) -> ResourceAddress;
            fn maturity_date(&self) -> UtcDateTime;
            fn get_yield_tokenizer_assets(
                &self,
                underlying_asset: ResourceAddress,
            ) -> (ResourceAddress, ResourceAddress);
            fn get_maturity_date(
                &self,
                underlying_asset: ResourceAddress
            ) -> UtcDateTime;
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
            assert!(market_fee_input.reserve_fee_percent > Decimal::ZERO && market_fee_input.reserve_fee_percent < Decimal::ONE);

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

            let underlying_asset_address = yield_tokenizer_component.underlying_resource();
            let (pt_address, yt_address) = 
                yield_tokenizer_component.get_yield_tokenizer_assets(underlying_asset_address);

            let maturity_date = yield_tokenizer_component.get_maturity_date(underlying_asset_address);

            let owner_role = OwnerRole::Updatable(AccessRule::from(owner_role_node.clone()));
            let combined_rule_node = owner_role_node.or(AccessRuleNode::from(global_component_caller_badge));

            let pool_component = 
                Blueprint::<TwoResourcePool>::instantiate(
                owner_role.clone(),
                AccessRule::from(combined_rule_node),
                (pt_address, underlying_asset_address),
                None,
            );

            let market_state = MarketState {
                total_pt: Decimal::ZERO,
                total_asset: Decimal::ZERO,
                scalar_root,
                last_ln_implied_rate: PreciseDecimal::ZERO,
                maturity_date,
                underlying_asset_address,
                pt_address,
                yt_address
            };

            let fee_rate = PreciseDecimal::from(market_fee_input.fee_rate.ln().unwrap());

            let market_fee = MarketFee {
                fee_rate,
                reserve_fee_percent: market_fee_input.reserve_fee_percent
            };

            Self {
                pool_component,
                yield_tokenizer_component,
                flash_loan_rm,
                market_fee,
                market_state,
                market_is_active: true,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .globalize()
        }

        // First set the natural log of the implied rate here.
        // We also set optional inital anchor rate as the there isn't an anchor rate yet until we have the implied rate.
        // The initial anchor rate is determined by a guess on the interest rate which trading will be most capital efficient.
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

            let rate_scalar = calc_rate_scalar(
                self.market_state.scalar_root,
                time_to_expiry
            );

            let market_compute = MarketCompute {
                rate_scalar,
                rate_anchor: initial_rate_anchor
            };

            let market_state = self.get_market_state();

            self.market_state.last_ln_implied_rate = self.get_ln_implied_rate( 
                time_to_expiry,
                market_compute,
                market_state,
            );

            info!("Implied Rate: {:?}", self.market_state.last_ln_implied_rate.exp().unwrap());
        }

        pub fn get_market_implied_rate(&mut self) -> PreciseDecimal {
            self.market_state.last_ln_implied_rate.exp().unwrap()
        }
        
        pub fn get_vault_reserves(&self) -> IndexMap<ResourceAddress, Decimal> {
            self.pool_component.get_vault_amounts()
        }

        pub fn get_market_state(&mut self) -> MarketState {
            let reserves = self.pool_component.get_vault_amounts();
            let market_state = MarketState {
                total_pt: reserves[0],
                total_asset: reserves[1],
                scalar_root: self.market_state.scalar_root,
                last_ln_implied_rate: self.market_state.last_ln_implied_rate,
                maturity_date: self.market_state.maturity_date,
                underlying_asset_address: self.market_state.underlying_asset_address,
                pt_address: self.market_state.pt_address,
                yt_address: self.market_state.yt_address,
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
            lsu_token: FungibleBucket, 
            principal_token: FungibleBucket
        ) -> (Bucket, Option<Bucket>) {
            self.pool_component.contribute((lsu_token.into(), principal_token.into()))
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
            self.pool_component.redeem(pool_units.into())
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
            assert_ne!(self.check_maturity(), true, "Market has reached its maturity");
            assert_eq!(principal_token.resource_address(), self.market_state.pt_address);

            let market_state = self.get_market_state();
            let time_to_expiry = self.time_to_expiry();

            // Calcs the rate scalar and rate anchor with the current market state
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Calcs the the swap
            let lsu_to_account = self.calc_trade(
                principal_token.amount().checked_neg().unwrap(), 
                time_to_expiry,
                &market_state,
                &market_compute,
            );

            info!(
                "[swap_exact_pt_for_lsu] All-in Exchange rate: {:?}", 
                principal_token.amount().checked_div(lsu_to_account).unwrap()
            );

            // Deposit all given PT tokens to the pool.
            self.pool_component.protected_deposit(principal_token.into());

            // Withdraw the amount of LSU tokens from the pool.
            let owed_lsu_bucket = self.pool_component.protected_withdraw(
                self.market_state.underlying_asset_address, 
                lsu_to_account, 
                WithdrawStrategy::Rounded(RoundingMode::ToZero)
            );

            // Saves the new implied rate.
            self.market_state.last_ln_implied_rate = 
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state
                );

            info!(
                "[swap_exact_pt_for_lsu] LSU Returned: {:?}", 
                owed_lsu_bucket.amount()
            );

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
            assert_ne!(self.check_maturity(), true, "Maturity date has lapsed");
            assert_eq!(lsu_token.resource_address(), self.market_state.underlying_asset_address);

            let time_to_expiry = self.time_to_expiry();

            let market_state = self.get_market_state();

            // Calcs the rate scalar and rate anchor with the current market state
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Calcs the swap
            let required_lsu = self.calc_trade(
                desired_pt_amount,
                time_to_expiry,
                &market_state,
                &market_compute,
            );

            // Assert the amount of LSU sent in is at least equal to the required
            // LSU needed for the desired PT amount.
            assert!(lsu_token.amount() >= required_lsu);

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

            // Deposit the required LSU to the pool.
            self.pool_component.protected_deposit(required_lsu_bucket.into());

            // Withdraw the desired PT amount.
            let owed_pt_bucket = self.pool_component.protected_withdraw(
                self.market_state.pt_address, 
                desired_pt_amount, 
                WithdrawStrategy::Rounded(RoundingMode::ToZero)
            );

            // Saves the new implied rate of the trade.
            self.market_state.last_ln_implied_rate = 
                self.get_ln_implied_rate(
                    time_to_expiry, 
                    market_compute,
                    market_state,
                );

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
            assert_ne!(
                self.check_maturity(), 
                true, 
                "Market has reached its maturity"
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

            let yield_token_data: YieldTokenData = yield_token.as_non_fungible().non_fungible().data();

            info!("[swap_exact_lsu_for_yt] YT Amount: {:?}", yield_token_data.underlying_lsu_amount);

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
        -> (FungibleBucket, Option<NonFungibleBucket>, Option<FungibleBucket>) 
        {
            assert_ne!(self.check_maturity(), true, "Market has reached its maturity");
            assert_eq!(yield_token.resource_address(), self.market_state.yt_address);
            
            // Need to borrow the same amount of PT as YT to redeem LSU
            let data: YieldTokenData = yield_token.non_fungible().data();
            let underlying_lsu_amount = data.underlying_lsu_amount;
            assert!(underlying_lsu_amount >= amount_yt_to_swap_in);
            let pt_flash_loan_amount = amount_yt_to_swap_in;

            // Borrow equivalent amount of PT from the pool - enough to get LSU
            let (pt_flash_loan, flash_loan_receipt) = 
                self.flash_loan(
                    self.market_state.pt_address, 
                    pt_flash_loan_amount
                );

            // Combine PT and YT to redeem LSU
            let (mut lsu_token, option_yt_bucket) = 
                self.yield_tokenizer_component.redeem(pt_flash_loan, yield_token, amount_yt_to_swap_in);

            // Retrieve flash loan requirements to ensure enough can be swapped back to repay
            // the flash loan.
            let flash_loan_data: FlashLoanReceipt = 
                flash_loan_receipt.as_non_fungible().non_fungible().data();

            let desired_pt_amount = flash_loan_data.amount;

            let time_to_expiry = self.time_to_expiry();
            let market_state = self.get_market_state();
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Portion of lsu is sold to the pool for PT to return the borrowed PT
            let required_lsu = self.calc_trade(
                    desired_pt_amount,
                    time_to_expiry,
                    &market_state,
                    &market_compute,
                );

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate: {:?}", 
                desired_pt_amount.checked_div(required_lsu).unwrap()
            );

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate: {:?}", 
                required_lsu.checked_div(desired_pt_amount).unwrap()
            );

            let required_lsu_bucket = lsu_token.take(required_lsu);

            let (pt_flash_loan_repay, returned_lsu) = 
                self.swap_exact_lsu_for_pt(required_lsu_bucket, desired_pt_amount);

            lsu_token.put(returned_lsu);
            
            let optional_return_bucket = self.flash_loan_repay(pt_flash_loan_repay, flash_loan_receipt);

            self.market_state.last_ln_implied_rate = self.get_ln_implied_rate(
                time_to_expiry, 
                market_compute,
                market_state
            );

            info!("[swap_exact_yt_for_lsu] LSU Returned: {:?}", lsu_token.amount());
            

            return (lsu_token, option_yt_bucket, optional_return_bucket)
        }


        pub fn swap_exact_yt_for_lsu2(
            &mut self, 
            yield_token: NonFungibleBucket,
            amount_yt_to_swap_in: Decimal,
        ) 
        -> (FungibleBucket, Option<NonFungibleBucket>) 
        {
            assert_ne!(self.check_maturity(), true, "Market has reached its maturity");
            assert_eq!(yield_token.resource_address(), self.market_state.yt_address);
            
            // Need to borrow the same amount of PT as YT to redeem LSU
            let data: YieldTokenData = yield_token.non_fungible().data();
            let underlying_lsu_amount = data.underlying_lsu_amount;
            assert!(underlying_lsu_amount >= amount_yt_to_swap_in);
            let pt_to_withdraw = amount_yt_to_swap_in;

            let time_to_expiry = self.time_to_expiry();
            let market_state = self.get_market_state();
            let market_compute = 
                self.compute_market(
                    market_state.clone(),
                    time_to_expiry
                );

            // Do we calc_trade before any assets are removed/added?
            let lsu_owed_for_pt_flash_swap = self.calc_trade(
                // Make sure the signs are correct based on direction of the trade
                pt_to_withdraw,
                time_to_expiry,
                &market_state,
                &market_compute,
            );

            let withdrawn_pt = self.pool_component.protected_withdraw(
                self.market_state.pt_address, 
                pt_to_withdraw, 
                WithdrawStrategy::Exact
            );

            // Combine PT and YT to redeem LSU
            let (mut lsu_token, option_yt_bucket) = 
                self.yield_tokenizer_component.redeem(withdrawn_pt.as_fungible(), yield_token, amount_yt_to_swap_in);


            // Do we need assertion here to make sure lsu token received is greater than lsu owed?
            let lsu_owed_back_to_pool = 
                lsu_token
                .amount()
                .checked_sub(lsu_owed_for_pt_flash_swap)
                .unwrap();

            let lsu_owed = lsu_token.take(lsu_owed_back_to_pool);


            self.pool_component.protected_deposit(lsu_owed.into());

            info!(
                "[swap_exact_yt_for_lsu] All-in Exchange rate: {:?}", 
                pt_to_withdraw.checked_div(lsu_owed_for_pt_flash_swap).unwrap()
            );

            self.market_state.last_ln_implied_rate = self.get_ln_implied_rate(
                time_to_expiry, 
                market_compute,
                market_state
            );

            info!("[swap_exact_yt_for_lsu] LSU Returned: {:?}", lsu_token.amount());

            return (lsu_token, option_yt_bucket)
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

            // Consider using MarketState instead
            let proportion = calc_proportion(
                net_pt_amount,
                market_state.total_pt,
                market_state.total_asset
            );

            info!("[Calc_trade] Market Compute: {:?}", market_compute);
            info!("[Calc_trade] Net PT Amount: {:?}", net_pt_amount);
            info!("[Calc_trade] Total PT State: {:?}", market_state.total_pt);
            info!("[Calc_trade] Total Asset State: {:?}", market_state.total_asset);
            info!("[Calc_trade] Proportion: {:?}", proportion);

            // Calcs exchange rate based on size of the trade (change)
            let pre_fee_exchange_rate = calc_exchange_rate(
                proportion,
                market_compute.rate_anchor,
                market_compute.rate_scalar
            );

            info!("[Calc_trade] Pre Fee Exchange Rate: {:?}", pre_fee_exchange_rate);

            let pre_fee_amount = 
                net_pt_amount
                .checked_div(pre_fee_exchange_rate)
                .unwrap()
                .checked_neg()
                .unwrap();

            info!("[Calc_trade] Pre Fee Amount: {:?}", pre_fee_amount);

            let fee = calc_fee(
                self.market_fee.fee_rate,
                time_to_expiry,
                net_pt_amount,
                pre_fee_exchange_rate,
                pre_fee_amount
            );

            info!("[Calc_trade] Fee: {:?}", fee);

            // Fee allocated to the asset reserve
            let net_asset_fee_to_reserve =
                fee
                .checked_mul(self.market_fee.reserve_fee_percent)
                .unwrap();

            info!("[Calc_trade] Net Asset Fee: {:?}", net_asset_fee_to_reserve);

            // Trading fee allocated to the reserve based on the direction
            // of the trade.
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

            info!("[Calc_trade] Net Amount: {:?}", net_amount);

            // Net amount can be negative depending on direciton of the trade.
            // However, we want to have net amount to be positive to be able to 
            // perform the asset swap.
            let net_amount = if net_amount < PreciseDecimal::ZERO {
                // LSU ---> PT
                net_amount
                .checked_add(net_asset_fee_to_reserve)
                .and_then(|result| result.checked_abs())
                .unwrap()
            } else {
                // PT ---> LSU
                net_amount
                .checked_sub(net_asset_fee_to_reserve)
                .unwrap()
            };

            return Decimal::try_from(net_amount).ok().unwrap()
            

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

            let proportion = calc_proportion(
                dec!(0),
                market_state.total_pt,
                market_state.total_asset
            );

            let exchange_rate = calc_exchange_rate(
                proportion,
                market_compute.rate_anchor,
                market_compute.rate_scalar
            );

            // exchangeRate >= 1 so its ln >= 0
            let ln_exchange_rate = exchange_rate.ln().unwrap();

            let ln_implied_rate = 
                ln_exchange_rate.checked_mul(PERIOD_SIZE)
                .and_then(|result| result.checked_div(time_to_expiry))
                .unwrap();

            return ln_implied_rate
        }

        pub fn time_to_expiry(&self) -> i64 {
            self.market_state.maturity_date.to_instant().seconds_since_unix_epoch 
                - Clock::current_time_rounded_to_seconds().seconds_since_unix_epoch
        }

        /// Checks whether maturity has lapsed
        pub fn check_maturity(&self) -> bool {
            Clock::current_time_comparison(
                self.market_state.maturity_date.to_instant(), 
                TimePrecision::Second, 
                TimeComparisonOperator::Gte
            )
        }
    }
}


