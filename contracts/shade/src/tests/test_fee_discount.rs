#![cfg(test)]

use crate::shade::{Shade, ShadeClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn setup(env: &Env) -> (Address, ShadeClient<'_>, Address, Address) {
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(env, &contract_id);

    let admin = Address::generate(env);
    client.initialize(&admin);

    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.add_accepted_token(&admin, &token);
    client.set_fee(&admin, &token, &500);

    (admin, client, contract_id, token)
}

#[test]
fn test_no_discount_below_threshold() {
    let env = Env::default();
    let (_admin, client, _contract_id, token) = setup(&env);
    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let fee = client.calculate_fee(&merchant, &token, &10_000_000);
    let expected = 10_000_000i128 * 500 / 10_000;
    assert_eq!(fee, expected);
}

// TODO: fix commented tests

// #[test]
// fn test_discount_tier_1_at_10k_volume() {
//     let env = Env::default();
//     let (_admin, client, contract_id, token) = setup(&env);
//     let merchant = Address::generate(&env);
//     client.register_merchant(&merchant);

//     env.as_contract(&contract_id, || {
//         admin_component::increment_merchant_volume(&env, &merchant, &token, 10_000);
//     });

//     let fee = client.calculate_fee(&merchant, &token, &10_000_000);
//     let base_fee = 10_000_000i128 * 500 / 10_000;
//     let expected = base_fee - (base_fee * 10 / 10_000);
//     assert_eq!(fee, expected);
// }

// #[test]
// fn test_discount_tier_2_at_50k_volume() {
//     let env = Env::default();
//     let (_admin, client, contract_id, token) = setup(&env);
//     let merchant = Address::generate(&env);
//     client.register_merchant(&merchant);

//     env.as_contract(&contract_id, || {
//         admin_component::increment_merchant_volume(&env, &merchant, &token, 50_000);
//     });

//     let fee = client.calculate_fee(&merchant, &token, &10_000_000);
//     let base_fee = 10_000_000i128 * 500 / 10_000;
//     let expected = base_fee - (base_fee * 25 / 10_000);
//     assert_eq!(fee, expected);
// }

// #[test]
// fn test_discount_tier_3_at_100k_volume() {
//     let env = Env::default();
//     let (_admin, client, contract_id, token) = setup(&env);
//     let merchant = Address::generate(&env);
//     client.register_merchant(&merchant);

//     env.as_contract(&contract_id, || {
//         admin_component::increment_merchant_volume(&env, &merchant, &token, 100_000);
//     });

//     let fee = client.calculate_fee(&merchant, &token, &10_000_000);
//     let base_fee = 10_000_000i128 * 500 / 10_000;
//     let expected = base_fee - (base_fee * 50 / 10_000);
//     assert_eq!(fee, expected);
// }

// #[test]
// fn test_volume_accumulates() {
//     let env = Env::default();
//     let (_admin, client, contract_id, token) = setup(&env);
//     let merchant = Address::generate(&env);
//     client.register_merchant(&merchant);

//     env.as_contract(&contract_id, || {
//         admin_component::increment_merchant_volume(&env, &merchant, &token, 10_000);
//         admin_component::increment_merchant_volume(&env, &merchant, &token, 20_000);
//     });

//     assert_eq!(client.get_merchant_volume(&merchant, &token), 60_000);

//     let fee = client.calculate_fee(&merchant, &token, &10_000_000);
//     let base_fee = 10_000_000i128 * 500 / 10_000;
//     let expected = base_fee - (base_fee * 25 / 10_000);
//     assert_eq!(fee, expected);
// }

#[test]
fn test_zero_fee_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.add_accepted_token(&admin, &token);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    assert_eq!(client.calculate_fee(&merchant, &token, &10_000_000), 0);
}
