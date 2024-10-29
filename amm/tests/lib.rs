use scrypto_math::ExponentialPreciseDecimal;
use scrypto_test::{prelude::*, utils::dump_manifest_to_file_system};
use yield_amm::dex::yield_amm_test::YieldAMMState;
use common::structs::*;
use off_ledger::market_approx::*;

mod test_environment;
use test_environment::TestEnvironment;


#[test]
fn instantiate() {
    TestEnvironment::instantiate();
}

#[test]
fn can_instantiate_amm() {
    let mut test_env = TestEnvironment::instantiate();

    let receipt = test_env.instantiate_amm();

    receipt.expect_commit_success();

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );
}

#[test]
fn add_liquidity() {
    let mut test_environment = TestEnvironment::instantiate();

    let receipt = test_environment.add_liquidity(dec!(1000), dec!(1000));

    receipt.expect_commit_success();
}

#[test]
fn remove_liquidity() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment
        .add_liquidity(dec!(1000), dec!(1000))
        .expect_commit_success();

    let receipt = test_environment.remove_liquidity(dec!(1000));

    receipt.expect_commit_success();
}

#[test]
fn set_initial_ln_implied_rate() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment
        .add_liquidity(dec!(1000), dec!(1000))
        .expect_commit_success();

    let receipt = test_environment.set_initial_ln_implied_rate(pdec!("1.04"));

    receipt.expect_commit_success();
}

#[test]
fn swap_exact_pt_for_lsu() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let receipt = test_environment.swap_exact_pt_for_lsu(dec!("703.124999999999999999"));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

#[test]
fn swap_pt_for_lsu_one_day_before_maturity() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let date = UtcDateTime::new(2025, 03, 04, 0, 0, 0).ok().unwrap();

    test_environment.advance_date(date);

    let receipt = test_environment.swap_exact_pt_for_lsu(dec!(100));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

#[test]
fn exchange_rate_narrows_towards_maturity() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let date = UtcDateTime::new(2025, 02, 05, 0, 0, 0).ok().unwrap();

    test_environment.advance_date(date);

    let receipt = test_environment.swap_exact_pt_for_lsu(dec!(100));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

#[test]
fn swap_exact_lsu_for_pt() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    test_environment.swap_exact_lsu_for_pt(dec!(100), dec!(133.16));

    let receipt = test_environment.swap_exact_lsu_for_pt(dec!(100), dec!(133.16));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

// #[test]
// fn swap_exact_lsu_for_yt() {
//     let mut test_environment = TestEnvironment::instantiate();

//     test_environment.set_up(dec!(4000), dec!(4000), pdec!("1.04"));

//     let receipt = test_environment.swap_exact_lsu_for_yt(dec!(100));

//     println!(
//         "Transaction Receipt: {}",
//         receipt.display(&AddressBech32Encoder::for_simulator())
//     );

//     receipt.expect_commit_success();
// }

#[test]
fn swap_exact_yt_for_lsu() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let receipt = test_environment.swap_exact_yt_for_lsu(dec!(100));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

#[test]
fn swap_exact_yt_for_lsu2() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let receipt = test_environment.swap_exact_yt_for_lsu2(dec!(100));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

#[test]
fn swap_one_day_before_maturity() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let date = UtcDateTime::new(2025, 03, 04, 23, 59, 59).ok().unwrap();

    test_environment.advance_date(date);

    let receipt = test_environment.swap_exact_pt_for_lsu(dec!(999));

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();
}

#[test]
pub fn lp_fees_increases() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    test_environment
        .swap_exact_pt_for_lsu(dec!(100))
        .expect_commit_success();

    let receipt = test_environment.get_vault_reserves();

    let output: IndexMap<ResourceAddress, Decimal> = receipt.expect_commit_success().output(1);

    println!("Vault Reserves: {:?}", output);

    receipt.expect_commit_success();
}

#[test]
fn prove_interest_rate_continuity() {
    let mut test_environment = TestEnvironment::instantiate();

    // Setting up the pool
    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    // Establishing the market implied rate
    test_environment
        .swap_exact_pt_for_lsu(dec!(100))
        .expect_commit_success();

    // Retrieving market implied rate
    let component_state: YieldAMMState = test_environment
        .ledger
        .component_state::<YieldAMMState>(test_environment.amm_component);
    let last_ln_implied_rate = component_state.market_state.last_ln_implied_rate;

    // Calculating pre-trade implied rate
    let current_time = test_environment.ledger.get_current_proposer_timestamp_ms() / 1000;

    let current_date = UtcDateTime::from_instant(&Instant::new(current_time))
        .ok()
        .unwrap();

    let expiry = component_state.market_state.maturity_date;

    let time_to_expiry = expiry.to_instant().seconds_since_unix_epoch
        - current_date.to_instant().seconds_since_unix_epoch;

    let manifest = ManifestBuilder::new()
        .lock_fee(test_environment.account.account_component, dec!(10))
        .call_method(
            test_environment.amm_component,
            "compute_market",
            manifest_args!(time_to_expiry),
        );
    
    let receipt = test_environment.execute_manifest(
        manifest.object_names(),
        manifest.build(),
        "compute_market"
    );

    let output: MarketCompute = receipt.expect_commit_success().output(1);

    let manifest = ManifestBuilder::new()
        .lock_fee(test_environment.account.account_component, dec!(10))
        .call_method(
            test_environment.amm_component,
            "get_vault_reserves",
            manifest_args!(),
        );

    let receipt = test_environment.execute_manifest(
        manifest.object_names(),
        manifest.build(),
        "get_vault_reserves"
    );

    let reserves: IndexMap<ResourceAddress, Decimal> = 
        receipt.expect_commit_success().output(1);

    let total_pt = reserves[0];
    let total_asset = reserves[1];
    
    let current_proportion = yield_amm::liquidity_curve::calc_proportion(
        dec!(0), 
        total_pt,
        total_asset,
    );

    let rate_scalar = output.rate_scalar;
    let rate_anchor = output.rate_anchor;

    let pre_trade_exchange_rate = yield_amm::liquidity_curve::calc_exchange_rate(
        current_proportion, 
        rate_anchor, 
        rate_scalar, 
    );

    // Asserting pre trade exchange rate = last market implied rate
    assert_eq!(last_ln_implied_rate.exp().unwrap(), pre_trade_exchange_rate);
}

#[test]
fn can_no_longer_trade_after_expiry() {
    let mut test_environment = TestEnvironment::instantiate();

    test_environment.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let date = UtcDateTime::new(2025, 03, 05, 0, 0, 0).ok().unwrap();

    test_environment.advance_date(date);

    test_environment
        .swap_exact_pt_for_lsu(dec!(100))
        .expect_commit_failure();

    test_environment
        .swap_exact_lsu_for_pt(dec!(100), dec!(100))
        .expect_commit_failure();

    // test_environment
    //     .swap_exact_lsu_for_yt(dec!(100))
    //     .expect_commit_failure();

    test_environment
        .swap_exact_yt_for_lsu(dec!(100))
        .expect_commit_failure();
}



// Testing Goals:
// Whether implied rate moves and the conditions to which it moves
// Interest rate continuity is maintained
// Exchange rate is calculated correctly
// Whether fee is applied correctly
// Testing notes:
// Proportion as it relates to size of the tradedoesn't seem to change exchange rate,
// More so that the reserves of the pool do. However, time to maturity seems to be biggest
// factor.
// What happens when the liquidity of the reserves are too low?
// Particularly with LSU ---> YT swaps, can require lots of borrow in the pool.
// Want to simulate a trade which people constantly trading on one side.
