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
use scrypto_math::*;
use crate::error::MarketError;
use crate::dex::PERIOD_SIZE;

    /// Calculates the exchange rate based on the proportion of the trade, 
    /// rate scalar, and rate anchor.
    pub fn calc_exchange_rate(
        proportion: Decimal,
        rate_anchor: PreciseDecimal,
        rate_scalar: Decimal,
    ) -> Result<PreciseDecimal, MarketError> {
        let ln_proportion = 
            log_proportion(proportion)?;

        let exchange_rate = 
            ln_proportion
                .checked_div(rate_scalar)
                .and_then(
                    |result| 
                    result.checked_add(rate_anchor)
                )
                .unwrap();

        // Exchange rate represents how many assets you get for 1 PT
        // If exchange rate < 1:
        // You would get less than 1 asset for 1 PT
        // This violates the fundamental principle that 1 PT will be worth 1 asset at expiry
        // It would create an arbitrage opportunity where you could:
        // Buy PT for less than 1 asset
        // Hold until expiry
        // Redeem for 1 asset, making risk-free profit

        // For example:
        // If exchange rate = 0.9
        // You could buy 1 PT for 0.9 assets
        // At expiry, redeem 1 PT for 1 asset
        // Profit 0.1 assets risk-free
        // If exchange rate = 1
        // There would be no incentive to buy PT since you could just hold the asset
        // The time value of money would not be reflected in the price
        // This is why:

        // Exchange rate must be > 1 to reflect the time value of money
        // The difference above 1 represents the implied interest rate
        // As time approaches expiry, the exchange rate will approach (but stay above) 1
        if exchange_rate < PreciseDecimal::ONE {
            return Err(MarketError::InvalidExchangeRate(Decimal::try_from(exchange_rate).unwrap()));
        }

        Ok(exchange_rate)
    }

    /// Calculates the size of the trade in relation
    /// to pool size in terms of PT sent or receiving.
    pub fn calc_proportion(
        net_pt_amount: Decimal,
        total_pt: Decimal,
        total_asset: Decimal,
    ) -> Decimal {
            
        let numerator = 
            total_pt
            .checked_sub(net_pt_amount)
            .unwrap();

        let proportion = 
            numerator
            .checked_div(
                total_pt.checked_add(total_asset).unwrap()
            )
            .expect("Proportion calculation failed");

        return proportion
    }

    /// Proportion which unwraps to a None when taking natural log may indicate illiquid market.
    /// The logit function (ln(p/(1-p))) is used for several important reasons:

    // Bounded Range:

    // The proportion p is bounded between 0 and 1 (0% to 100% of the pool)
    // The logit function maps this bounded range (0,1) to the entire real number line (-∞,+∞)
    // This ensures that the exchange rate can respond smoothly to any possible proportion of assets in the pool
    // Symmetry:

    // The logit function is symmetric around p=0.5, meaning it treats buying and selling with equal sensitivity
    // When p approaches 0, ln(p/(1-p)) approaches -∞
    // When p approaches 1, ln(p/(1-p)) approaches +∞
    // This creates a natural price discovery mechanism
    // Capital Efficiency:

    // This curve shape (derived from the logit function) provides better capital efficiency
    // The slope becomes steeper at the extremes (when p approaches 0 or 1), which helps prevent the pool from being drained
    // The gentler slope in the middle range allows for more efficient trading with less slippage
    // Interest Rate Relationship:

    // The logit transformation helps establish a clear relationship between the proportion of assets and the implied interest rate
    // When combined with rateScalar and rateAnchor, it creates a predictable and manageable interest rate curve
    // This is crucial for the "interest rate continuity" which is essential for:
    // Maintaining stable prices in normal trading conditions (middle of the curve)
    // Preventing pool drainage (extreme ends of the curve)
    // Creating a smooth relationship between pool proportions and interest rates
    // Enabling efficient capital utilization across the trading range
    pub fn log_proportion(
        proportion: Decimal
    ) -> Result<PreciseDecimal, MarketError> {

        // Proportion must be less than 1
        // p = y/(x+y)  where:
        // y is the amount of PT in the pool
        // x is the amount of assets in the pool
        // Need to < 1 bc if p = 1, then y = x + y
        // This would mean x = 0 (no assets in the pool)
        // When we try to calculate (log the proportion) 
        // ln(p/(1-p)) where p = 1
        // ln(1/(1-1)) = ln(1/0) becomes 0 
        // This is undefined mathematically (division by zero)
        // The implications when proportion equals 1:
        // It would mean the pool has only PT and no assets
        // This is an invalid state because:
        // The AMM needs both assets to function
        // There would be no liquidity for trading
        // Price discovery would be impossible

        // If proportion > 1, this means:

        // y/(x+y) > 1
        // y > x + y
        // -x > 0
        // x < 0

        // This is problematic because:

        // It's mathematically impossible to have negative assets in the pool
        // The AMM cannot operate with negative liquidity
        // When calculating ln(p/(1-p)) where p > 1:
        // If p = 1.2, then:
        // ln(1.2/(1-1.2))
        // = ln(1.2/-0.2)
        // = ln(-6)
        // This is undefined because natural log of a negative number is undefined in real numbers.

        // Trade takes out more asset than is in the pool.
        if proportion >= Decimal::ONE {
            return Err(MarketError::ProportionGreaterThanOrEqualToOne(proportion));
        }

        // Trade takes out more PT than is in the pool.
        if proportion.is_negative() {
            return Err(MarketError::ProportionLessThanZero(proportion));
        }

        let logit_p = 
            proportion
            .checked_div(
                Decimal::ONE
                .checked_sub(proportion)
                .unwrap()
            )
            .ok_or_else(
                || 
                MarketError::ArithmeticError("Division overflow in Logit calculation".to_string()
            ))?;

        let ln_proportion = 
            PreciseDecimal::from(logit_p)
            .ln()
            .ok_or_else(
                || 
                MarketError::ArithmeticError("Invalid proportion for ln calculation".to_string()
            ))?;

        Ok(ln_proportion)
    }

    /// Calculates the scalar rate as a function of time to maturity.
    /// The scalar rate determines the steepness of the curve. A higher 
    /// scalar rate flattens the curve (less slippage) while a lower scalar 
    /// rate steepens the curve (more slippage). It is based is based on an 
    /// initial immutable scalar root value. As the market matures, the scalar 
    /// rate increases, which ultimately flattens the curve over time. It is
    /// important that the curve flattens over time as it narrows... 
    pub fn calc_rate_scalar(
        scalar_root: Decimal,
        time_to_expiry: i64
    ) -> Decimal {
        
        let rate_scalar: Decimal = 
            scalar_root
            .checked_mul(PERIOD_SIZE)
            .and_then(
                |result| 
                result.checked_div(time_to_expiry)
            )
            .unwrap();

        assert!(rate_scalar >= Decimal::ZERO);

        return rate_scalar
    }

    /// Calculates the rate anchor
    /// The rate anchor determines where the curve starts and where exchange rates
    /// are initially anchored (and ultimately the implied rate of the market).
    /// E.g: A rate anchor of 1.05 means that the exchange rate will be around ~1.05
    /// pending other factors such as the rate scalar, size of the trade, and fees.
    pub fn calc_rate_anchor(
        last_ln_implied_rate: PreciseDecimal,
        proportion: Decimal,
        time_to_expiry: i64, 
        rate_scalar: Decimal
    ) -> Result<PreciseDecimal, MarketError> {
        let last_exchange_rate = 
            calc_exchange_rate_from_implied_rate(
                last_ln_implied_rate, 
                time_to_expiry
            );
    
        if last_exchange_rate < PreciseDecimal::ONE {
            return Err(MarketError::InvalidLastExchangeRate(Decimal::try_from(last_exchange_rate).unwrap()));
        }
    
        let ln_proportion = log_proportion(proportion)?;
    
        let new_exchange_rate: PreciseDecimal = 
            ln_proportion
            .checked_div(rate_scalar)
            .ok_or_else(
                || 
                MarketError::ArithmeticError("Division overflow in exchange rate calculation".to_string()
            ))?;
    
        let rate_anchor: PreciseDecimal = 
            last_exchange_rate
            .checked_sub(new_exchange_rate)
            .ok_or_else(
                || 
                MarketError::ArithmeticError("Subtraction overflow in rate calculation".to_string()
            ))?;

        Ok(rate_anchor)
    }

    /// Calculates and applies fees based on the direction of the trade.
    /// Since fees are a function of time to maturity, the fees will decrease
    /// as the market matures and contributes to flattening the curve over time.
    pub fn calc_fee(
        fee_rate: PreciseDecimal,
        time_to_expiry: i64,
        net_pt_amount: Decimal,
        exchange_rate: PreciseDecimal,
        pre_fee_amount: PreciseDecimal
    ) -> Result<PreciseDecimal, MarketError> {
        // In this case, the fee rate is the implied rate.
        let fee_rate = 
            calc_exchange_rate_from_implied_rate(
                fee_rate, 
                time_to_expiry
            );

        let fee_amount;

        // Multiply the trade if the direction of the trade is from LSU ---> PT
        // Divide the fee if the direciton of the trade is from PT ---> LSU
        if net_pt_amount > Decimal::ZERO {
            let post_fee_exchange_rate = 
                exchange_rate
                .checked_div(fee_rate)
                .unwrap();

            if post_fee_exchange_rate < PreciseDecimal::ONE {
                return Err(
                    MarketError::InvalidPostFeeExchangeRate(
                        Decimal::try_from(post_fee_exchange_rate).unwrap()
                    )
                );
            }

            // pre_fee_amount is negative but because fee_rate is subtracted by 1, 
            // fee_rate is also a negative. Multiplying together makes the result positive.
            fee_amount = 
                pre_fee_amount
                    .checked_mul(
                        PreciseDecimal::ONE
                        .checked_sub(fee_rate)
                        .unwrap()
                    )
                    .unwrap();
        } else {

            fee_amount = 
                pre_fee_amount
                    .checked_mul(
                        PreciseDecimal::ONE
                        .checked_sub(fee_rate)
                        .unwrap()
                    )
                    .and_then(
                        |result: PreciseDecimal| 
                        result.checked_div(fee_rate)
                    )
                    .and_then(
                        |result: PreciseDecimal| 
                        result.checked_neg()
                    )
                    .unwrap();
        };

        Ok(fee_amount)
    }

    /// Converts implied rate to an exchange rate given a time to expiry.
    pub fn calc_exchange_rate_from_implied_rate(
        ln_implied_rate: PreciseDecimal, 
        time_to_expiry: i64
    ) -> PreciseDecimal {

        let rt: PreciseDecimal = 
            ln_implied_rate
                .checked_mul(time_to_expiry)
                .and_then(|result: PreciseDecimal| 
                    result
                    .checked_div(PERIOD_SIZE)
                )
                .unwrap();
        
        let exchange_rate: PreciseDecimal = 
            rt.exp()
            .unwrap();

        return exchange_rate
    }