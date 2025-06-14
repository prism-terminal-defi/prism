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
    PrismSplitterAdapter impl [
        #[cfg(feature = "trait")]
        Trait,
        #[cfg(feature = "scrypto-stubs")]
        ScryptoStub,
        #[cfg(feature = "scrypto-test-stubs")]
        ScryptoTestStub,
    ] {
        fn tokenize(
            &mut self, 
            amount: FungibleBucket, 
            optional_yt_bucket: Option<NonFungibleBucket>
        ) -> (FungibleBucket, NonFungibleBucket);

        fn redeem(
            &mut self, 
            pt_bucket: FungibleBucket, 
            yt_bucket: NonFungibleBucket, 
            yt_redeem_amount: Decimal, 
        ) -> (FungibleBucket, Option<NonFungibleBucket>, Option<FungibleBucket>);
        fn claim_yield(
            &mut self, 
            yt_bucket: NonFungibleBucket,
        ) -> (FungibleBucket, Option<NonFungibleBucket>);
        fn get_underlying_asset_redemption_value(&self, amount: Decimal) -> Decimal;
        fn get_underlying_asset_redemption_factor(&self) -> Decimal;
        fn calc_asset_owed_amount(&self, amount: Decimal) -> Decimal;
        fn pt_address(&self) -> ResourceAddress;
        fn yt_address(&self) -> ResourceAddress;
        fn underlying_asset(&self) -> ResourceAddress;
        fn maturity_date(&self) -> UtcDateTime;
        fn protocol_resources(&self) -> (ResourceAddress, ResourceAddress);
    }
}
