use scrypto::prelude::*;
use crate::dex::yield_amm::YieldAMM;
use common::structs::*;

#[blueprint]
pub mod yield_amm_factory {
    
    enable_function_auth! {
        instantiate_yield_amm_factory => rule!(allow_all);
    }

    extern_blueprint! {
        "package_sim1p4nhxvep6a58e88tysfu0zkha3nlmmcp6j8y5gvvrhl5aw47jfsxlt",
        YieldTokenizerFactory {
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
            fn get_yield_tokenizer_map(&self) 
                -> HashMap<ResourceAddress, ComponentAddress>;
            fn get_yield_tokenizer_assets(
                &self,
                underlying_asset: ResourceAddress,
            ) -> (ResourceAddress, ResourceAddress);
        }
    }

    const TOKENIZER_FACTORY: Global<YieldTokenizerFactory> = global_component! (
        YieldTokenizerFactory,
        "component_sim1cr56r93g67fc6cmu8ump7n878jvndyztnns0a3slrafpc7jk2t366c"
    );

    pub struct YieldAMMFactory {
        pub yield_amm_map: HashMap<ResourceAddress, Global<YieldAMM>>,
    }

    impl YieldAMMFactory {
        pub fn instantiate_yield_amm_factory(
            owner_badge: ResourceAddress,
        ) -> Global<YieldAMMFactory> {

            let owner_role = OwnerRole::Updatable(rule!(require(owner_badge)));

            Self {
                yield_amm_map: HashMap::new()
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .globalize()
        }

        // Before a new AMM is instantiated need to know whether YieldTokenizer exist
        // Since this will be a private method, don't need to worry too much about security
        pub fn instantiate_new_yield_amm(
            &mut self,
            underlying_asset: ResourceAddress,
            scalar_root: Decimal,
            // Maybe implement the log within smart contract, then change back to Decimal input
            rate_anchor: PreciseDecimal,
            market_fee_input: MarketFeeInput,
        ) -> Global<YieldAMM> {

            assert_eq!(
                self.is_yield_tokenizer_exist(underlying_asset),
                true
            );

            let yield_tokenizer_map = TOKENIZER_FACTORY.get_yield_tokenizer_map();

            // Handle unwrap
            // Does YieldTokenizer exist?
            let yield_tokenizer_address = yield_tokenizer_map.get(&underlying_asset).unwrap();

            // Has the maturity date lapsed?

            let owner_badge_rule = Runtime::global_component().get_owner_role().rule;

            let owner_access_rule_node = match owner_badge_rule {
                AccessRule::Protected(access_rule_node) => access_rule_node,
                _ => unreachable!("Unexpected variant in owner_badge_rule"),
            };

            let yield_amm = 
                YieldAMM::instantiate_yield_amm(
                    owner_access_rule_node, 
                    scalar_root, 
                    market_fee_input,
                    *yield_tokenizer_address,
                );

            yield_amm.set_initial_ln_implied_rate(rate_anchor);
            
            self.yield_amm_map.insert(underlying_asset, yield_amm);

            return yield_amm
        }

        fn is_yield_tokenizer_exist(
            &self,
            underlying_asset: ResourceAddress,
        ) -> bool {

            // There could potentially be two markets for the same underlying asset
            // If there are two markets...
            let yield_tokenizer_map = 
                TOKENIZER_FACTORY.get_yield_tokenizer_map();

            yield_tokenizer_map.contains_key(&underlying_asset)

        }

        fn get_tokenizer_metadata(
            &self,
            asset_resource_address: ResourceAddress,
        ) -> ResourceAddress {

            let underlying_asset_global: GlobalAddress = 
                ResourceManager::from(asset_resource_address)
                .get_metadata("underyling_asset_address")
                .unwrap()
                .expect("");

            // Think about error handling better here
            let underlying_asset_address = 
                ResourceAddress::try_from(underlying_asset_global).ok().unwrap();

            return underlying_asset_address

        }

        pub fn add_liquidity(
            &mut self,
            underlying_asset: FungibleBucket,
            principal_token: FungibleBucket
        ) -> (
            Bucket, 
            Option<Bucket>, 
            // FungibleBucket, 
            // FungibleBucket
        ) {

            // Check whether PT is paired with underlying asset

            let underlying_asset_address = 
                self.get_tokenizer_metadata(principal_token.resource_address());

            assert_eq!(
                underlying_asset_address,
                underlying_asset.resource_address()
            );

            // Does the AMM exist? If so, what happens?

            let yield_amm = 
                self.yield_amm_map.get_mut(&underlying_asset_address).unwrap();

            yield_amm.add_liquidity(underlying_asset, principal_token)
        }

        pub fn remove_liqudity(
            &mut self,
            pool_units: FungibleBucket,
        ) -> (Bucket, Bucket) {

            todo!()
        }

        pub fn swap_exact_pt_for_lsu(
            &mut self,
            principal_token: FungibleBucket
        ) -> FungibleBucket {

            let underlying_asset = 
                self.get_tokenizer_metadata(principal_token.resource_address());

            
            let yield_amm = 
                self.yield_amm_map.get_mut(&underlying_asset).unwrap();


            yield_amm.swap_exact_pt_for_lsu(principal_token)
        }

        pub fn swap_exact_lsu_for_pt(
            &mut self,
            mut underlying_asset: FungibleBucket,
            guess_pt_amount: Decimal
        ) -> (FungibleBucket, FungibleBucket) {

            let yield_amm = 
                self.yield_amm_map
                    .get_mut(&underlying_asset.resource_address())
                    .unwrap();

            yield_amm.swap_exact_lsu_for_pt(
                underlying_asset, 
                guess_pt_amount
            )
        }

        pub fn swap_exact_lsu_for_yt(
            &mut self,
            mut underlying_asset: FungibleBucket,
            guess_amount_to_swap_in: Decimal
        ) -> NonFungibleBucket {

            let yield_amm = 
                self.yield_amm_map
                    .get_mut(&underlying_asset.resource_address())
                    .unwrap();

            yield_amm.swap_exact_lsu_for_yt(
                underlying_asset, 
                guess_amount_to_swap_in
            )
        }

        pub fn swap_exact_yt_for_lsu(
            &mut self,
            yield_token: NonFungibleBucket,
            amount_yt_to_swap_in: Decimal,
        ) -> (FungibleBucket, Option<NonFungibleBucket>, Option<FungibleBucket>) {

            let underlying_asset = 
                self.get_tokenizer_metadata(yield_token.resource_address());

            let yield_amm = 
            self.yield_amm_map.get_mut(&underlying_asset).unwrap();

            // Remember to switch this
            yield_amm.swap_exact_yt_for_lsu(
                yield_token, 
                amount_yt_to_swap_in
            )
            
        }


    }
}