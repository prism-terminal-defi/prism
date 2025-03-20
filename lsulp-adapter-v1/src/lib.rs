// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

mod blueprint_interface;
pub use blueprint_interface::*;

use ports_interface::pool::*;
use scrypto::prelude::*;
use scrypto_interface::*;

macro_rules! pool {
    ($address: expr) => {
        $crate::blueprint_interface::CaviarLsuPoolInterfaceScryptoStub::from(
            $address,
        )
    };
}

pub const NUMBER_VALIDATOR_PRICES_TO_UPDATE: u32 = 5;

#[blueprint_with_traits]
pub mod adapter {
    use scrypto::prelude::sbor;

    enable_method_auth! {
        methods {
            change_pool_address => restrict_to: [OWNER];
            get_redemption_value => PUBLIC;
            calc_asset_owed_amount => PUBLIC;
            total_stake_amount => PUBLIC;
            total_stake_unit_supply => PUBLIC;
            stake_unit_resource_address => PUBLIC;
            get_redemption_factor => PUBLIC;
            pool_address => PUBLIC;
        }
    }

    struct CaviarLsuPoolAdapter {
        pool_address: ComponentAddress
    }

    impl CaviarLsuPoolAdapter {
        pub fn instantiate(
            owner_access_rule: AccessRule,
            pool_address: ComponentAddress,
            dapp_definition: ComponentAddress,
            address_reservation: Option<GlobalAddressReservation>,
        ) -> Global<CaviarLsuPoolAdapter> {
            let address_reservation =
                address_reservation.unwrap_or_else(|| {
                    Runtime::allocate_component_address(BlueprintId {
                        package_address: Runtime::package_address(),
                        blueprint_name: Runtime::blueprint_name(),
                    })
                    .0
                });

            Self {
                pool_address
            }
            .instantiate()
            .prepare_to_globalize(OwnerRole::Updatable(owner_access_rule))
            .metadata(metadata! {
                init {
                    "dapp_definition" => dapp_definition, updatable;
                }
            })
            .with_address(address_reservation)
            .globalize()
        }

        pub fn change_pool_address(&mut self, new_pool_address: ComponentAddress) {
            self.pool_address = new_pool_address;
        }
    }

    impl PoolAdapterInterfaceTrait for CaviarLsuPoolAdapter {
        fn get_redemption_value(
            &self,
            asset_amount: Decimal,
        ) -> Decimal {
            let mut caviar_pool = pool!(self.pool_address);
            caviar_pool.update_multiple_validator_prices(NUMBER_VALIDATOR_PRICES_TO_UPDATE);

            let liquidity_token_total_supply = 
                caviar_pool
                .get_liquidity_token_total_supply();

            let dex_valuation = caviar_pool.get_dex_valuation_xrd();

            assert!(
                dex_valuation > Decimal::ZERO && 
                liquidity_token_total_supply > Decimal::ZERO,
                "Invalid pool state"
            );

            dex_valuation
            .checked_div(liquidity_token_total_supply)
            .and_then(|x| x.checked_mul(asset_amount))
            .expect("[CaviarLsuPoolAdapter] Redemption value calculation failed")
        }

        fn calc_asset_owed_amount(
            &self,
            amount: Decimal
        ) -> Decimal {
            let redemption_factor = self.get_redemption_factor();

            amount
            .checked_div(redemption_factor)
            .expect("[CaviarLsuPoolAdapter] Asset owed calculation failed")
        }

        fn total_stake_amount(&self) -> Decimal {
            let caviar_pool = pool!(self.pool_address);
            caviar_pool.get_dex_valuation_xrd()
        }

        fn total_stake_unit_supply(&self) -> Decimal {
            let caviar_pool = pool!(self.pool_address);
            caviar_pool.get_liquidity_token_total_supply()
        }

        fn stake_unit_resource_address(&self) -> ResourceAddress {
            let caviar_pool = pool!(self.pool_address);
            caviar_pool.get_liquidity_token_resource_address()
        }

        fn get_redemption_factor(&self) -> Decimal {
            let mut caviar_pool = pool!(self.pool_address);
            caviar_pool.update_multiple_validator_prices(NUMBER_VALIDATOR_PRICES_TO_UPDATE);

            caviar_pool.get_dex_valuation_xrd()
            .checked_div(caviar_pool.get_liquidity_token_total_supply())
            .expect("[CaviarLsuPoolAdapter] Redemption factor calculation failed")
        }

        fn pool_address(&self) -> ComponentAddress {
            self.pool_address
        }
    }
}
