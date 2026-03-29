#![cfg(test)]

use crate::errors::ContractError;
use crate::shade::{Shade, ShadeClient};
use crate::types::DataKey;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

fn setup_test() -> (Env, ShadeClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    // Use a dummy hash for now. For registration tests, we'll need to upload the actual WASM.
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    (env, client, contract_id)
}

#[test]
fn test_register_merchant_successfully() {
    let (env, client, contract_id) = setup_test();
    let merchant = Address::generate(&env);

    client.register_merchant(&merchant);

    let merchant_data = client.get_merchant(&1u64);
    assert_eq!(merchant_data.id, 1);
    assert_eq!(merchant_data.address, merchant);
    assert!(merchant_data.active);

    assert!(client.is_merchant(&merchant));

    let merchant_count: u64 = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantCount)
            .unwrap()
    });
    assert_eq!(merchant_count, 1);
}

#[test]
fn test_register_multiple_merchants_with_unique_ids() {
    let (env, client, contract_id) = setup_test();
    let merchant_1 = Address::generate(&env);
    let merchant_2 = Address::generate(&env);

    client.register_merchant(&merchant_1);
    client.register_merchant(&merchant_2);

    let merchant_data_1 = client.get_merchant(&1u64);
    let merchant_data_2 = client.get_merchant(&2u64);

    assert_eq!(merchant_data_1.id, 1);
    assert_eq!(merchant_data_1.address, merchant_1);
    assert_eq!(merchant_data_2.id, 2);
    assert_eq!(merchant_data_2.address, merchant_2);

    let merchant_count: u64 = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantCount)
            .unwrap()
    });
    assert_eq!(merchant_count, 2);
}

#[test]
fn test_register_duplicate_merchant_fails() {
    let (env, client, _contract_id) = setup_test();
    let merchant = Address::generate(&env);

    client.register_merchant(&merchant);

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::MerchantAlreadyRegistered as u32);
    let result = client.try_register_merchant(&merchant);

    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_get_merchant_lookup_valid_id() {
    let (env, client, _contract_id) = setup_test();
    let merchant = Address::generate(&env);

    client.register_merchant(&merchant);

    let merchant_data = client.get_merchant(&1u64);
    assert_eq!(merchant_data.id, 1);
    assert_eq!(merchant_data.address, merchant);
    assert!(merchant_data.active);
}

#[test]
fn test_get_merchant_lookup_invalid_id_fails() {
    let (_env, client, _contract_id) = setup_test();

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::MerchantNotFound as u32);

    let missing_zero = client.try_get_merchant(&0u64);
    assert!(matches!(missing_zero, Err(Ok(err)) if err == expected_error));

    let missing_out_of_range = client.try_get_merchant(&99u64);
    assert!(matches!(missing_out_of_range, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_is_merchant_returns_true_for_registered_and_false_for_unknown() {
    let (env, client, _contract_id) = setup_test();
    let registered_merchant = Address::generate(&env);
    let unknown_merchant = Address::generate(&env);

    client.register_merchant(&registered_merchant);

    assert!(client.is_merchant(&registered_merchant));
    assert!(!client.is_merchant(&unknown_merchant));
}
