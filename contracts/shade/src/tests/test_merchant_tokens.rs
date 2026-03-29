#![cfg(test)]

use crate::errors::ContractError;
use crate::shade::Shade;
use crate::shade::ShadeClient;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{Address, BytesN, Env, Map, String, Symbol, TryIntoVal, Val, Vec};

fn assert_tokens_set_event(
    env: &Env,
    contract_id: &Address,
    expected_merchant: &Address,
    expected_tokens: &Vec<Address>,
    expected_timestamp: u64,
) {
    let events = env.events().all();
    assert!(!events.is_empty());

    let (event_contract_id, topics, data) = events.get(events.len() - 1).unwrap();
    assert_eq!(event_contract_id, contract_id.clone());
    assert_eq!(topics.len(), 1);

    let event_name: Symbol = topics.get(0).unwrap().try_into_val(env).unwrap();
    assert_eq!(event_name, Symbol::new(env, "merchant_tokens_set_event"));

    let data_map: Map<Symbol, Val> = data.try_into_val(env).unwrap();
    let merchant_val = data_map.get(Symbol::new(env, "merchant")).unwrap();
    let tokens_val = data_map.get(Symbol::new(env, "tokens")).unwrap();
    let timestamp_val = data_map.get(Symbol::new(env, "timestamp")).unwrap();

    let merchant_in_event: Address = merchant_val.try_into_val(env).unwrap();
    let tokens_in_event: Vec<Address> = tokens_val.try_into_val(env).unwrap();
    let timestamp_in_event: u64 = timestamp_val.try_into_val(env).unwrap();

    assert_eq!(merchant_in_event, expected_merchant.clone());
    assert_eq!(tokens_in_event, expected_tokens.clone());
    assert_eq!(timestamp_in_event, expected_timestamp);
}

#[test]
fn test_merchant_sets_and_gets_tokens() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    // Add tokens to global whitelist first
    client.add_accepted_token(&admin, &token1);
    client.add_accepted_token(&admin, &token2);

    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());

    let timestamp = env.ledger().timestamp();
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    assert_tokens_set_event(&env, &contract_id, &merchant, &tokens, timestamp);

    let merchant_tokens = client.get_merchant_accepted_tokens(&merchant);
    assert_eq!(merchant_tokens.len(), 1);
    assert_eq!(merchant_tokens.get(0).unwrap(), token1);
}

#[test]
fn test_merchant_cannot_set_unaccepted_global_token() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token = Address::generate(&env); // Not in global whitelist

    let mut tokens = Vec::new(&env);
    tokens.push_back(token);

    let result = client.try_set_merchant_accepted_tokens(&merchant, &tokens);
    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::TokenNotAccepted as u32);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_invoice_creation_with_merchant_whitelist() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    client.add_accepted_token(&admin, &token1);
    client.add_accepted_token(&admin, &token2);

    // Merchant only accepts token1
    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    // Invoice with token1 should succeed
    client.create_invoice(
        &merchant,
        &String::from_str(&env, "Test"),
        &1000,
        &token1,
        &None,
    );

    // Invoice with token2 should fail (globally accepted but not by merchant)
    let result = client.try_create_invoice(
        &merchant,
        &String::from_str(&env, "Test"),
        &1000,
        &token2,
        &None,
    );
    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::TokenNotAcceptedByMerchant as u32);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_empty_merchant_whitelist_defaults_to_global() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    client.add_accepted_token(&admin, &token);

    // Merchant hasn't set a whitelist, so any global token should work
    client.create_invoice(
        &merchant,
        &String::from_str(&env, "Test"),
        &1000,
        &token,
        &None,
    );
}

#[test]
fn test_non_merchant_cannot_set_tokens() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let non_merchant = Address::generate(&env);
    let tokens = Vec::new(&env);

    let result = client.try_set_merchant_accepted_tokens(&non_merchant, &tokens);
    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::MerchantNotFound as u32);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_duplicate_tokens_are_deduplicated() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    client.add_accepted_token(&admin, &token);

    // Pass the same token twice
    let mut tokens = Vec::new(&env);
    tokens.push_back(token.clone());
    tokens.push_back(token.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    let merchant_tokens = client.get_merchant_accepted_tokens(&merchant);
    assert_eq!(merchant_tokens.len(), 1);
    assert_eq!(merchant_tokens.get(0).unwrap(), token);
}

#[test]
fn test_merchant_can_overwrite_whitelist() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    client.add_accepted_token(&admin, &token1);
    client.add_accepted_token(&admin, &token2);

    // Set to token1 only
    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    let merchant_tokens = client.get_merchant_accepted_tokens(&merchant);
    assert_eq!(merchant_tokens.len(), 1);

    // Overwrite with token2 only
    let mut tokens2 = Vec::new(&env);
    tokens2.push_back(token2.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens2);

    let merchant_tokens = client.get_merchant_accepted_tokens(&merchant);
    assert_eq!(merchant_tokens.len(), 1);
    assert_eq!(merchant_tokens.get(0).unwrap(), token2);

    // token1 should no longer be accepted by this merchant
    assert!(!client.is_token_accepted_for_merchant(&merchant, &token1));
    assert!(client.is_token_accepted_for_merchant(&merchant, &token2));
}

#[test]
fn test_remove_merchant_accepted_token() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    client.add_accepted_token(&admin, &token1);
    client.add_accepted_token(&admin, &token2);

    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());
    tokens.push_back(token2.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    assert_eq!(client.get_merchant_accepted_tokens(&merchant).len(), 2);

    // Remove token1
    client.remove_merchant_accepted_token(&merchant, &token1);

    let merchant_tokens = client.get_merchant_accepted_tokens(&merchant);
    assert_eq!(merchant_tokens.len(), 1);
    assert_eq!(merchant_tokens.get(0).unwrap(), token2);
}

#[test]
fn test_remove_nonexistent_token_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    client.add_accepted_token(&admin, &token1);
    client.add_accepted_token(&admin, &token2);

    // Only set token1
    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    // Try to remove token2 which is not in the merchant's list
    let result = client.try_remove_merchant_accepted_token(&merchant, &token2);
    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::TokenNotAccepted as u32);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_inactive_merchant_cannot_set_tokens() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    // Deactivate the merchant
    client.set_merchant_status(&admin, &1, &false);

    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    client.add_accepted_token(&admin, &token);

    let mut tokens = Vec::new(&env);
    tokens.push_back(token);

    let result = client.try_set_merchant_accepted_tokens(&merchant, &tokens);
    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::MerchantNotActive as u32);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_is_token_accepted_for_merchant_public() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    client.add_accepted_token(&admin, &token1);
    client.add_accepted_token(&admin, &token2);

    // No merchant whitelist set — both global tokens should be accepted
    assert!(client.is_token_accepted_for_merchant(&merchant, &token1));
    assert!(client.is_token_accepted_for_merchant(&merchant, &token2));

    // Set merchant whitelist to only token1
    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());
    client.set_merchant_accepted_tokens(&merchant, &tokens);

    assert!(client.is_token_accepted_for_merchant(&merchant, &token1));
    assert!(!client.is_token_accepted_for_merchant(&merchant, &token2));
}
