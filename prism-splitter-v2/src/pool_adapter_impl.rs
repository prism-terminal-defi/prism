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

#[derive(ScryptoSbor, PartialEq, Debug)]
pub struct OneResourcePoolWrapper(pub Global<OneResourcePool>);
#[derive(ScryptoSbor, PartialEq, Debug)]
pub struct ValidatorWrapper(pub Global<Validator>);

impl PoolAdapterInterfaceTrait for OneResourcePoolWrapper {
    fn get_redemption_value(&self, asset_amount: Decimal) -> Decimal {
        self.0.get_redemption_value(asset_amount)
    }

    fn total_stake_amount(&self) -> Decimal {
        self.0.get_vault_amount()
    }

    fn total_stake_unit_supply(&self) -> Decimal {
        let pool_unit_address = 
            get_pool_unit_address(self.0.address().into());
        ResourceManager::from(pool_unit_address)
        .total_supply()
        .unwrap_or(Decimal::ZERO)
    }

    fn stake_unit_resource_address(&self) -> ResourceAddress {
        get_pool_unit_address(self.0.address().into())
    }

    fn pool_address(&self) -> ComponentAddress {
        self.0.address()
    }

    fn get_redemption_factor(&self) -> Decimal {
        self.total_stake_amount()
        .checked_div(self.total_stake_unit_supply())
        .expect("[OneResourcePoolWrapper] Redemption factor calculation failed")
    }

    fn calc_asset_owed_amount(&self, amount: Decimal) -> Decimal {
        let resource_divisibility =
            ResourceManager::from(
                self.stake_unit_resource_address()
            )
            .resource_type()
            .divisibility()
            .unwrap();

        let redemption_factor = self.get_redemption_factor();

        let amount_owed = 
            PreciseDecimal::from(amount)
            .checked_div(PreciseDecimal::from(redemption_factor))
            .expect("[OneResourcePoolWrapper] Asset owed calculation failed");

        amount_owed
        .checked_round(
            resource_divisibility,
            RoundingMode::ToNearestMidpointToEven
        )
        .and_then(
            |x|
            Decimal::try_from(x).ok()
        )
        .expect("[OneResourcePoolWrapper] Rounding owed amount failed")
    }
}

impl PoolAdapterInterfaceTrait for ValidatorWrapper {
    fn get_redemption_value(&self, asset_amount: Decimal) -> Decimal {
        self.0.get_redemption_value(asset_amount)
    }

    fn total_stake_amount(&self) -> Decimal {
        self.0.total_stake_xrd_amount()
    }

    fn total_stake_unit_supply(&self) -> Decimal {
        self.0.total_stake_unit_supply()
    }

    fn stake_unit_resource_address(&self) -> ResourceAddress {
        get_stake_unit_address(self.0.address().into())
    }

    fn pool_address(&self) -> ComponentAddress {
        self.0.address()
    }

    fn get_redemption_factor(&self) -> Decimal {
        self.total_stake_amount()
        .checked_div(self.total_stake_unit_supply())
        .expect("[ValidatorWrapper] Redemption factor calculation failed")
    }

    fn calc_asset_owed_amount(&self, amount: Decimal) -> Decimal {
        let resource_divisibility =
            ResourceManager::from(
                self.stake_unit_resource_address()
            )
            .resource_type()
            .divisibility()
            .unwrap();

        let redemption_factor = self.get_redemption_factor();

        let amount_owed = 
            PreciseDecimal::from(amount)
            .checked_div(PreciseDecimal::from(redemption_factor))
            .expect("[ValidatorWrapper] Asset owed calculation failed");

        amount_owed
        .checked_round(
            resource_divisibility,
            RoundingMode::ToNearestMidpointToEven
        )
        .and_then(
            |x|
            Decimal::try_from(x).ok()
        )
        .expect("[ValidatorWrapper] Rounding owed amount failed")
    }
}

pub fn get_pool_unit_address(pool: Global<OneResourcePool>) -> ResourceAddress {
    let global_address: GlobalAddress = 
    pool.get_metadata("pool_unit")
        .unwrap()
        .unwrap();

    ResourceAddress::try_from(global_address).ok().unwrap()
}

pub fn get_stake_unit_address(validator: Global<Validator>) -> ResourceAddress {
    let global_address: GlobalAddress = 
    validator.get_metadata("pool_unit")
        .unwrap()
        .unwrap();

    ResourceAddress::try_from(global_address).ok().unwrap()
}

