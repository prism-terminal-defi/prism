use scrypto_math::ExponentialPreciseDecimal;
use scrypto_test::{prelude::*, utils::dump_manifest_to_file_system};
use radix_transactions::manifest::decompiler::ManifestObjectNames;
use yield_amm::dex::yield_amm_test::YieldAMMState;
use common::structs::*;
use off_ledger::market_approx::*;
use std::path::Path;


#[derive(ManifestSbor, Clone, Debug)]
pub struct MarketFeeInput {
    // The trading fee charged on each trade.
    pub fee_rate: Decimal,
    // The reserve fee rate.
    pub reserve_fee_percent: Decimal,
}

#[derive(ScryptoSbor, ManifestSbor)]
pub enum Expiry {
    TwelveMonths,
    EighteenMonths,
    TwentyFourMonths,
}

pub struct Account {
    pub public_key: Secp256k1PublicKey,
    pub account_component: ComponentAddress,
    pub owner_badge: ResourceAddress,
}

pub struct Ledger {
    pub ledger: DefaultLedgerSimulator,
    pub account: Account,
    pub lsu_resource_address: ResourceAddress,
    pub validator_address: ComponentAddress,
    // pub amm_component: ComponentAddress,
    // pub pool_component: ComponentAddress,
    // pub pool_unit: ResourceAddress,
    // pub lsu_resource_address: ResourceAddress,
    // pub pt_resource: ResourceAddress,
    // pub yt_resource: ResourceAddress,
    // pub package_address: PackageAddress,
}

impl Ledger {
    pub fn new() -> Self {
        let custom_genesis = CustomGenesis::default(
            Epoch::of(1),
            CustomGenesis::default_consensus_manager_config(),
        );

        let mut ledger: LedgerSimulator<NoExtension, InMemorySubstateDatabase> = 
            LedgerSimulatorBuilder::new()
                .with_custom_genesis(custom_genesis)
                .without_kernel_trace()
                .build();


        let current_date = UtcDateTime::new(2024, 03, 05, 0, 0, 0).ok().unwrap();
        let current_date_ms = current_date.to_instant().seconds_since_unix_epoch * 1000;
        let receipt = ledger.advance_to_round_at_timestamp(Round::of(2), current_date_ms);
        receipt.expect_commit_success();

        let (
            public_key, 
            _private_key, 
            account_component
        ) = ledger.new_allocated_account();

        let owner_badge = 
            ledger
            .create_fungible_resource(
                dec!(1), 
                0u8, 
                account_component
            );

        let account = Account {
            public_key,
            account_component,
            owner_badge
        };

        ledger.load_account_from_faucet(account.account_component);

        let key = Secp256k1PrivateKey::from_u64(1u64).unwrap().public_key();
        let validator_address = ledger.get_active_validator_with_key(&key);
        let lsu_resource_address = ledger
            .get_active_validator_info_by_key(&key)
            .stake_unit_resource;

        Self::stake_to_validator(
            &mut ledger, 
            &account, 
            validator_address,
            dec!(10000)
        );








        // Self::tokenize_underlying_asset(
        //     &mut ledger, 
        //     &account, 
        //     yield_tokenizer_component_address, 
        //     lsu_resource_address, 
        //     dec!(5000)
        // );

        // let yield_amm_package_address = ledger.compile_and_publish(this_package!());

        // let scalar_root = dec!(50);

        // let market_fee_input = MarketFeeInput {
        //     fee_rate: dec!("1.01"),
        //     reserve_fee_percent: dec!("0.80")
        // };

        // let args = manifest_args!(owner_badge, scalar_root, market_fee_input);

        // let receipt = 
        //     Self::instantiate_component(
        //         &mut ledger, 
        //         &account, 
        //         yield_amm_package_address,
        //         "YieldAMM",
        //         "instantiate_yield_amm",
        //         args
        //     );

        // let manifest = ManifestBuilder::new()
        //     .lock_fee_from_faucet()
        //     .call_function(
        //         package_address,
        //         "YieldAMM",
        //         "instantiate_yield_amm",
        //         manifest_args!(owner_badge, scalar_root, market_fee_input),
        //     )
        //     .build();

        // let receipt = ledger.execute_manifest(
        //     manifest,
        //     vec![NonFungibleGlobalId::from_public_key(&public_key)],
        // );

        // let amm_component = receipt.expect_commit(true).new_component_addresses()[0];
        // let pool_component = receipt.expect_commit(true).new_component_addresses()[1];
        // let pool_unit = receipt.expect_commit(true).new_resource_addresses()[1];

        Self {
            ledger,
            account,
            lsu_resource_address,
            validator_address,
            // amm_component,
            // pool_component,
            // pool_unit,
            // lsu_resource_address,
            // pt_resource,
            // yt_resource,
            // package_address,
        }
    }

    pub fn stake_to_validator(
        ledger: &mut LedgerSimulator<NoExtension, InMemorySubstateDatabase>,
        account: &Account,
        validator_address: ComponentAddress,
        amount: Decimal,
    ) {

        let manifest = ManifestBuilder::new()
            .lock_fee_from_faucet()
            .withdraw_from_account(
                account.account_component, 
                XRD, 
                dec!(10000)
            )
            .take_all_from_worktop(
                XRD, 
                "xrd"
            )
            .call_method_with_name_lookup(
                validator_address, 
                "stake", 
                |lookup| {
                (
                    lookup.bucket("xrd"),
                )
            })
            .deposit_batch(account.account_component)
            .build();

        ledger
            .execute_manifest(
                manifest,
                vec![NonFungibleGlobalId::from_public_key(&account.public_key)],
            )
            .expect_commit_success();
    }

    pub fn publish_package<P: AsRef<Path>>(
        ledger: &mut LedgerSimulator<NoExtension, InMemorySubstateDatabase>,
        path: P,
    ) -> PackageAddress {
        ledger.compile_and_publish(path)

    }

    pub fn instantiate_component(
        ledger: &mut LedgerSimulator<NoExtension, InMemorySubstateDatabase>,
        account: &Account,
        package_address: PackageAddress,
        blueprint: impl Into<String>,
        function: impl Into<String>,
        args: impl ResolvableArguments
    ) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee_from_faucet()
            .call_function(
                package_address,
                blueprint,
                function,
                args,
                // manifest_args!(owner_badge, scalar_root, market_fee_input),
            )
            .build();

        ledger.execute_manifest(
            manifest,
            vec![NonFungibleGlobalId::from_public_key(&account.public_key)],
        )
    }

    pub fn tokenize_underlying_asset(
        ledger: &mut LedgerSimulator<NoExtension, InMemorySubstateDatabase>,
        account: &Account,
        yield_tokenizer_component_address: ComponentAddress,
        underlying_asset_address: ResourceAddress,
        amount: Decimal,
    ) {
        let manifest = ManifestBuilder::new()
            .lock_fee_from_faucet()
            .withdraw_from_account(
                account.account_component, 
                underlying_asset_address, 
                amount
            )
            .take_all_from_worktop(
                underlying_asset_address, 
                "lsu_bucket"
            )"tokenize_yield", 
                |lookup| {
                (
                    lookup.bucket("lsu_bucket"),
                )
            })
            .deposit_batch(account.account_component)
            .build();

        let receipt = ledger.execute_manifest(
            manifest,
            vec![NonFungibleGlobalId::from_public_key(&account.public_key)],
        );

        receipt.expect_commit_success();

    }

            .call_method_with_name_lookup(
                yield_tokenizer_component_address, 
                
    pub fn publish_yield_amm_package(
        ledger: &mut LedgerSimulator<NoExtension, InMemorySubstateDatabase>,        
    )

    pub fn instantiate_amm(&mut self) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .call_function(
                self.package_address,
                "YieldAMM",
                "instantiate_yield_amm",
                manifest_args!(OwnerRole::None, dec!(50), dec!("1.01"), dec!("0.80"),),
            );

        self.execute_manifest(manifest.object_names(), manifest.build(), "instantiate_amm")
    }

    pub fn advance_date(&mut self, date: UtcDateTime) {
        let date_ms = date.to_instant().seconds_since_unix_epoch * 1000;
        let receipt = self
            .ledger
            .advance_to_round_at_timestamp(Round::of(3), date_ms);
        receipt.expect_commit_success();
    }

    pub fn execute_manifest(
        &mut self,
        object_manifest: ManifestObjectNames,
        built_manifest: TransactionManifestV1,
        name: &str,
    ) -> TransactionReceiptV1 {
        dump_manifest_to_file_system(
            object_manifest,
            &built_manifest,
            "./transaction_manifest",
            Some(name),
            &NetworkDefinition::stokenet(),
        )
        .ok();

        let receipt = self.ledger.execute_manifest(
            built_manifest,
            vec![NonFungibleGlobalId::from_public_key(
                &self.account.public_key,
            )],
        );

        return receipt;
    }

    pub fn set_up(
        &mut self,
        pt_resource_amount: Decimal,
        lsu_resource_address_amount: Decimal,
        initial_rate_anchor: PreciseDecimal,
    ) {
        let receipt = self.add_liquidity(pt_resource_amount, lsu_resource_address_amount);
        receipt.expect_commit_success();
        self.set_initial_ln_implied_rate(initial_rate_anchor)
            .expect_commit_success();
    }

    pub fn set_initial_ln_implied_rate(
        &mut self,
        initial_rate_anchor: PreciseDecimal,
    ) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .create_proof_from_account_of_amount(
                self.account.account_component, 
                self.owner_badge, 
                dec!(1)
            )
            .call_method(
                self.amm_component,
                "set_initial_ln_implied_rate",
                manifest_args!(initial_rate_anchor,),
            );

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "set_initial_ln_implied_rate",
        )
    }

    pub fn get_implied_rate(&mut self) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .call_method(
                self.amm_component,
                "get_market_implied_rate",
                manifest_args!(),
            );

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "get_implied_rate",
        )
    }

    pub fn add_liquidity(
        &mut self,
        pt_resource: Decimal,
        lsu_resource_address: Decimal,
    ) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(
                self.account.account_component,
                self.pt_resource,
                pt_resource,
            )
            .withdraw_from_account(
                self.account.account_component,
                self.lsu_resource_address,
                lsu_resource_address,
            )
            .take_all_from_worktop(self.pt_resource, "pt_resource")
            .take_all_from_worktop(self.lsu_resource_address, "lsu_resource_address")
            .call_method_with_name_lookup(self.amm_component, "add_liquidity", |lookup| {
                (
                    lookup.bucket("pt_resource"),
                    lookup.bucket("lsu_resource_address"),
                )
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(manifest.object_names(), manifest.build(), "add_liquidity")
    }

    pub fn remove_liquidity(&mut self, pool_unit_amount: Decimal) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(
                self.account.account_component,
                self.pool_unit,
                pool_unit_amount,
            )
            .take_all_from_worktop(self.pool_unit, "pool_unit")
            .call_method_with_name_lookup(self.amm_component, "remove_liquidity", |lookup| {
                (lookup.bucket("pool_unit"),)
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "remove_liquidity",
        )
    }

    pub fn swap_exact_pt_for_lsu(&mut self, pt_amount: Decimal) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(self.account.account_component, self.pt_resource, pt_amount)
            .take_all_from_worktop(self.pt_resource, "pt_resource")
            .call_method_with_name_lookup(self.amm_component, "swap_exact_pt_for_lsu", |lookup| {
                (lookup.bucket("pt_resource"),)
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "swap_exact_pt_for_lsu",
        )
    }

    pub fn swap_exact_lsu_for_pt(
        &mut self,
        lsu_amount: Decimal,
        desired_pt_amount: Decimal,
    ) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(
                self.account.account_component,
                self.lsu_resource_address,
                lsu_amount,
            )
            .take_all_from_worktop(self.lsu_resource_address, "lsu_resource_address")
            .call_method_with_name_lookup(self.amm_component, "swap_exact_lsu_for_pt", |lookup| {
                (lookup.bucket("lsu_resource_address"), desired_pt_amount)
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "swap_exact_lsu_for_pt",
        )
    }

    pub fn swap_exact_lsu_for_yt(&mut self, lsu_amount: Decimal, est_max_pt_in: Decimal) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(
                self.account.account_component,
                self.lsu_resource_address,
                lsu_amount,
            )
            .take_all_from_worktop(self.lsu_resource_address, "lsu_resource_address")
            .call_method_with_name_lookup(self.amm_component, "swap_exact_lsu_for_yt", |lookup| {
                (
                    lookup.bucket("lsu_resource_address"),
                    est_max_pt_in
                )
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "swap_exact_lsu_to_yt",
        )
    }

    pub fn swap_exact_yt_for_lsu(&mut self, yt_amount: Decimal) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(self.account.account_component, self.yt_resource, dec!(1))
            .take_all_from_worktop(self.yt_resource, "yt_resource")
            .call_method_with_name_lookup(self.amm_component, "swap_exact_yt_for_lsu", |lookup| {
                (lookup.bucket("yt_resource"), yt_amount)
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "swap_exact_yt_for_lsu",
        )
    }

    pub fn swap_exact_yt_for_lsu2(&mut self, yt_amount: Decimal) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .withdraw_from_account(self.account.account_component, self.yt_resource, dec!(1))
            .take_all_from_worktop(self.yt_resource, "yt_resource")
            .call_method_with_name_lookup(self.amm_component, "swap_exact_yt_for_lsu2", |lookup| {
                (lookup.bucket("yt_resource"), yt_amount)
            })
            .deposit_batch(self.account.account_component);

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "swap_exact_yt_for_lsu",
        )
    }

    pub fn get_vault_reserves(&mut self) -> TransactionReceiptV1 {
        let manifest = ManifestBuilder::new()
            .lock_fee(self.account.account_component, dec!(10))
            .call_method(self.amm_component, "get_vault_reserves", manifest_args!());

        self.execute_manifest(
            manifest.object_names(),
            manifest.build(),
            "get_vault_reserves",
        )
    }

    pub fn get_market_info(&mut self) -> (MarketState, MarketCompute, MarketFee, i64) {
        let component_state: YieldAMMState = 
            self.ledger
                .component_state::<YieldAMMState>(self.amm_component);
        
        let last_ln_implied_rate = component_state.market_state.last_ln_implied_rate;
        let scalar_root = component_state.market_state.scalar_root;
        let maturity_date = component_state.market_state.maturity_date;
        let underlying_asset_address = component_state.market_state.underlying_asset_address;
        let pt_address = component_state.market_state.pt_address;
        let yt_address = component_state.market_state.yt_address;
    
        let vault_reserves_receipt = self.get_vault_reserves();
        let reserves: IndexMap<ResourceAddress, Decimal> = vault_reserves_receipt.expect_commit_success().output(1);
    
        let total_pt = reserves[0];
        let total_asset = reserves[1];
    
        let market_state = MarketState {
            total_pt,
            total_asset,
            scalar_root,
            last_ln_implied_rate,
            maturity_date,
            underlying_asset_address,
            pt_address,
            yt_address,
        };
    
        let fee_rate = component_state.market_fee.fee_rate;
        let reserve_fee_percent = component_state.market_fee.reserve_fee_percent;
    
        let market_fee = MarketFee {
            fee_rate,
            reserve_fee_percent
        };
    
        let time_to_expiry = self.ledger_time_to_expiry();
    
        let market_compute = off_ledger::liquidity_curve::compute_market(
            time_to_expiry,
            &market_state
        );

        return (market_state, market_compute, market_fee, time_to_expiry)

    }

    pub fn ledger_time_to_expiry(&mut self) -> i64 {
        let component_state: YieldAMMState = self
            .ledger
            .component_state::<YieldAMMState>(self.amm_component);

        // Calculating pre-trade implied rate
        let current_time = self.ledger.get_current_proposer_timestamp_ms() / 1000;

        let current_date = UtcDateTime::from_instant(&Instant::new(current_time))
            .ok()
            .unwrap();

        let expiry = component_state.market_state.maturity_date;

        let time_to_expiry = expiry.to_instant().seconds_since_unix_epoch
            - current_date.to_instant().seconds_since_unix_epoch;
        return time_to_expiry
    }
}


pub struct TestEnvironment {
    pub ledger: DefaultLedgerSimulator,
    pub account: Account,
    pub amm_component: ComponentAddress,
    pub pool_component: ComponentAddress,
    pub pool_unit: ResourceAddress,
    pub lsu_resource_address: ResourceAddress,
    pub pt_resource: ResourceAddress,
    pub yt_resource: ResourceAddress,
    pub package_address: PackageAddress,
}

impl TestEnvironment {
    pub fn new(
        ledger: &Ledger
    ) -> Self {


        let account = ledger.account;
        let ledger_sim = ledger.ledger;

        let yield_tokenizer_package_address = Ledger::publish_package(
            &mut ledger_sim,
            "../yield_tokenizer"
        );

        println!("Yield Tokenizer Package: {}", yield_tokenizer_package_address.display(&AddressBech32Encoder::for_simulator()));

        let args = manifest_args!(account.owner_badge);

        let receipt = 
            Ledger::instantiate_component(
                &mut ledger_sim, 
                &account, 
                yield_tokenizer_package_address,
                "YieldTokenizerFactory",
                "instantiate_yield_tokenizer_factory",
                args
            );

        let yield_tokenizer_factory_component_address = receipt.expect_commit_success().new_component_addresses()[0];
        // let pt_resource = receipt.expect_commit_success().new_resource_addresses()[0];
        // let yt_resource = receipt.expect_commit_success().new_resource_addresses()[1];

        println!("Yield Tokenizer Component: {}", yield_tokenizer_factory_component_address.display(&AddressBech32Encoder::for_simulator()));
        
        Self {
            ledger: ledger_sim,
            account,

        }

    }
}