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

use scrypto::prelude::*;
use scrypto::prelude::sbor;
use scrypto_interface::*;

define_interface! {
    LsuPool as CaviarLsuPool impl [
        ScryptoStub,
        ScryptoTestStub,
    ] {
        fn new(
            admin_badge_resource_address: ResourceAddress,
            token_validator_component_address: ComponentAddress,
        ) -> Self;
        fn set_token_validator(
            token_validator_component_address: ComponentAddress
        );
        fn set_protocol_fee(&mut self, new_protocol_fee: Decimal);
        fn set_liquidity_fee(&mut self, new_liquidity_fee: Decimal);
        fn set_rederve_fee(&mut self, new_reserve_fee: Decimal);
        fn take_from_reserve_vaults(&mut self, resource_address: ResourceAddress) -> Bucket;
        fn set_validator_max_before_fee(&mut self, validator_max_before_fee: u32);
        fn get_token_validator_address(&self) -> ComponentAddress;
        fn get_fee_vaults_address(&self) -> ComponentAddress;
        fn get_vault_balance(&self, resource_address: ResourceAddress) -> Option<Decimal>;
        fn get_reserve_vault_balance(
            &self,
            resource_address: ResourceAddress,
        ) -> Option<Decimal>;
        fn get_price_lsu_xrd_cached(
            &self,
            resource_address: ResourceAddress,
        ) -> Option<Decimal>;
        fn get_dex_valuation_xrd(&self) -> Decimal;
        fn get_liquidity_token_resource_address(&self) -> ResourceAddress;
        fn get_liquidity_token_total_supply(&self) -> Decimal;
        fn get_credit_receipt_resource_address(&self) -> ResourceAddress;
        fn get_protocol_fee(&self) -> Decimal;   
        fn get_liquidity_fee(&self) -> Decimal;
        fn get_reserve_fee(&self) -> Decimal;
        fn get_price_lsu_xrd(&self, resource_address: ResourceAddress) -> Option<Decimal>;
        fn get_price(
            &self,
            lhs_resource_address: ResourceAddress,
            rhs_resource_address: ResourceAddress,
        ) -> Option<Decimal>;
        fn get_nft_data(&self, id: NonFungibleLocalId) -> HashMap<ResourceAddress, Decimal>;
        fn is_lsu_token(&self, resource_address: ResourceAddress) -> bool;
        fn is_validator(&self, component_address: ComponentAddress) -> bool;
        fn get_validator_address(
            &self,
            resource_address: ResourceAddress,
        ) -> Option<ComponentAddress>;
        fn get_validator_price_lsu_xrd(
            &self,
            resource_address: ResourceAddress,
        ) -> Option<Decimal>;
        fn get_validator_price_lsu_xrd_and_update_valuation(
            &mut self,
            resource_address: ResourceAddress,
        ) -> Decimal;
        fn update_multiple_validator_prices(&mut self, number: u32);
        fn get_validator_max_before_fee(&self) -> u32;
        fn get_validator_counter(&self) -> u32;
        fn get_validator_pointer(&self) -> u32;
        fn get_validator_address_map(&self, index: u32) -> ResourceAddress;
        fn get_id_resources_from_credit_proof(
            &self,
            proof: Proof,
        ) -> (NonFungibleLocalId, HashMap<ResourceAddress, Decimal>);
        fn merge_credit(&mut self, credit_proof1: Proof, credit_proof2: Proof);
        fn deposit_reserve_fee(&mut self, tokens: Bucket);
        fn add_liquidity(
            &mut self,
            bucket: Bucket, // mut bucket
            credit_proof: Option<Proof>,
        ) -> (Bucket, Bucket);
        fn remove_liquidity(
            &mut self,
            liquidity_tokens: Bucket, // mut liquidity_tokens
            lsu_resource: ResourceAddress,
            credit_proof: Option<Proof>,
        ) -> (Bucket, Bucket);
        fn swap(
            &mut self,
            bucket: Bucket, // mut bucket
            lsu_paying: ResourceAddress,
        ) -> (Bucket, Bucket);
    }
}

