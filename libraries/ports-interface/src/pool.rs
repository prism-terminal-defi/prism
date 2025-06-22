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
use scrypto_interface::*;
use scrypto::prelude::sbor;

define_interface! {
    PoolAdapter impl [
        #[cfg(feature = "trait")]
        Trait,
        #[cfg(feature = "scrypto-stubs")]
        ScryptoStub,
        #[cfg(feature = "scrypto-test-stubs")]
        ScryptoTestStub,
    ] {
        fn get_redemption_value(&self, asset_amount: Decimal) -> Decimal;

        fn calc_asset_owed_amount(&self, redemption_amount: Decimal) -> Decimal;

        fn total_stake_amount(&self) -> Decimal;

        fn total_stake_unit_supply(&self) -> Decimal;

        fn stake_unit_resource_address(&self) -> ResourceAddress;

        fn pool_address(&self) -> ComponentAddress;

        fn get_redemption_factor(&self) -> Decimal;
    }
}
