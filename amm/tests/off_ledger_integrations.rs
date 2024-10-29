use scrypto_math::ExponentialPreciseDecimal;
use scrypto_test::{prelude::*, utils::dump_manifest_to_file_system};

use radix_transactions::manifest::decompiler::ManifestObjectNames;
use yield_amm::dex::yield_amm_test::YieldAMMState;

use common::structs::*;
use off_ledger::market_approx::*;


mod test_environment;

use test_environment::TestEnvironment;

#[test]
fn approx_swap_exact_lsu_pt() {

    let mut test_env = TestEnvironment::instantiate();
    test_env.set_up(dec!(2000), dec!(2000), pdec!("1.04"));
    
    let (market_state, market_compute, market_fee, time_to_expiry) = 
        test_env.get_market_info();

    let mut approx_params = ApproxParams {
        guess_min: dec!(0),
        guess_max: dec!(1999),
        guess_offchain: Decimal::ZERO,
        max_iteration: 256,
        eps: dec!("0.01")
    };


    let asset_out = off_ledger::market_approx::market_approx_pt_out_lib::approx_swap_exact_sy_for_pt(
        &market_state,
        &market_compute,
        &market_fee,
        time_to_expiry,
        dec!(100),
        &mut approx_params
    );

    println!("[test] Asset out: {:?}", asset_out);

}


#[test]
fn swap_exact_lsu_for_pt_integration() {

    let mut test_env = TestEnvironment::instantiate();
    test_env.set_up(dec!(2000), dec!(2000), pdec!("1.04"));

    let (
        market_state, 
        market_compute, 
        market_fee, 
        time_to_expiry
    ) = test_env.get_market_info();

    let mut approx_params = ApproxParams {
        guess_min: dec!(0),
        guess_max: dec!(2000),
        guess_offchain: Decimal::ZERO,
        max_iteration: 256,
        eps: dec!("0.01")
    };

    let desired_pt_amount = off_ledger::market_approx::market_approx_pt_out_lib::approx_swap_exact_sy_for_pt(
        &market_state,
        &market_compute,
        &market_fee,
        time_to_expiry,
        dec!(100),
        &mut approx_params
    );

    let receipt = test_env.swap_exact_lsu_for_pt(dec!(100), desired_pt_amount.ok().unwrap());

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();

}


#[test]
fn approx_swap_exact_sy_for_yt() {
    let mut test_env = TestEnvironment::instantiate();
    test_env.set_up(dec!(2000), dec!(2000), pdec!("1.04"));
    
    let (market_state, market_compute, market_fee, time_to_expiry) = 
        test_env.get_market_info();

    let mut approx_params = ApproxParams {
        guess_min: dec!(0),
        guess_max: dec!(1999),
        guess_offchain: Decimal::ZERO,
        max_iteration: 256,
        eps: dec!("0.001")
    };


    let asset_out = off_ledger::market_approx::market_approx_pt_in_lib::approx_swap_exact_sy_for_yt(
        &market_state,
        &market_compute,
        &market_fee,
        time_to_expiry,
        dec!(100),
        &mut approx_params
    );

    println!("[test] Asset out: {:?}", asset_out);
    
} 

#[test]
fn swap_exact_lsu_for_yt_integration() {

    let mut test_env = TestEnvironment::instantiate();
    test_env.set_up(dec!(2000), dec!(2000), pdec!("1.04"));

    let (
        market_state, 
        market_compute, 
        market_fee, 
        time_to_expiry
    ) = test_env.get_market_info();

    let mut approx_params = ApproxParams {
        guess_min: dec!(0),
        guess_max: dec!(2000),
        guess_offchain: Decimal::ZERO,
        max_iteration: 256,
        eps: dec!("0.001")
    };

    let desired_yt_amount = off_ledger::market_approx::market_approx_pt_in_lib::approx_swap_exact_sy_for_yt(
        
        &market_state,
        &market_compute,
        &market_fee,
        time_to_expiry,
        dec!(100),
        &mut approx_params
    );

    println!("[test] Asset out: {:?}", desired_yt_amount);

    let receipt = test_env.swap_exact_lsu_for_yt(dec!(100), desired_yt_amount.ok().unwrap());

    println!(
        "Transaction Receipt: {}",
        receipt.display(&AddressBech32Encoder::for_simulator())
    );

    receipt.expect_commit_success();

}

#[test]
fn calc_max_pt_in() {

    let mut test_env = TestEnvironment::instantiate();
    test_env.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let (market_state, market_compute, _, _) = test_env.get_market_info();

    let max_pt_in = off_ledger::market_approx::market_approx_pt_in_lib::calc_max_pt_in(
        &market_state, 
        &market_compute
    );

    println!("Max Pt In: {:?}", max_pt_in);

}

#[test]
fn calc_slope() {

    let mut test_env = TestEnvironment::instantiate();
    test_env.set_up(dec!(1000), dec!(1000), pdec!("1.04"));

    let (market_state, market_compute, _, _) = test_env.get_market_info();

    let slope = off_ledger::market_approx::market_approx_pt_in_lib::calc_slope(
        &market_compute,
        &market_state,
        dec!(250), 
    );

    println!("Slope: {:?}", slope);

}