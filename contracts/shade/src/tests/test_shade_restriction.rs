#![cfg(test)]

use crate::errors::ContractError;
use crate::shade::{Shade, ShadeClient};
use crate::types::Role;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{Address, Env, Map, Symbol, TryIntoVal, Val};

/// Setup the test environment and return clients and addresses.
fn setup() -> (Env, ShadeClient<'static>, account::account::MerchantAccountClient<'static>, Address, Address, Address) {
    let env = Env::default();
    
    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Initial authorization to set up the state
    env.mock_all_auths();
    
    client.initialize(&admin);
    client.grant_role(&admin, &manager, &Role::Manager);

    // Register Merchant in Shade
    client.register_merchant(&merchant);

    // Deploy MerchantAccount (via manual registration flow simulation)
    let acct_id = env.register(account::account::MerchantAccount, ());
    let acct_client = account::account::MerchantAccountClient::new(&env, &acct_id);
    let merchant_id_val = 1_u64; 
    acct_client.initialize(&merchant, &contract_id, &merchant_id_val);

    // Link Merchant Account in Shade
    client.set_merchant_account(&merchant, &acct_id);

    (env, client, acct_client, admin, manager, merchant)
}

/// Helper to assert the latest account restricted event.
fn assert_latest_account_restricted_event(
    env: &Env,
    contract_id: &Address,
    expected_merchant: &Address,
    expected_status: bool,
    expected_caller: &Address,
) {
    let events = env.events().all();
    assert!(!events.is_empty());

    let mut found = false;
    for i in (0..events.len()).rev() {
        let (event_contract_id_i, topics_i, data_i) = events.get(i).unwrap();
        if topics_i.len() == 1 {
            let event_name: Symbol = topics_i.get(0).unwrap().try_into_val(env).unwrap();
            if event_name == Symbol::new(env, "account_restricted_event") {
                assert_eq!(event_contract_id_i, contract_id.clone());
                let data_map: Map<Symbol, Val> = data_i.try_into_val(env).unwrap();
                let merchant_val = data_map.get(Symbol::new(env, "merchant")).unwrap();
                let status_val = data_map.get(Symbol::new(env, "status")).unwrap();
                let caller_val = data_map.get(Symbol::new(env, "caller")).unwrap();

                let merchant_in_event: Address = merchant_val.try_into_val(env).unwrap();
                let status_in_event: bool = status_val.try_into_val(env).unwrap();
                let caller_in_event: Address = caller_val.try_into_val(env).unwrap();

                assert_eq!(merchant_in_event, expected_merchant.clone());
                assert_eq!(status_in_event, expected_status);
                assert_eq!(caller_in_event, expected_caller.clone());
                found = true;
                break;
            }
        }
    }
    assert!(found, "account_restricted_event not found in events");
}

#[test]
fn test_admin_restrict_merchant_account_success() {
    let (env, client, acct_client, admin, _manager, merchant) = setup();

    // Verify initial state
    assert_eq!(acct_client.is_restricted_account(), false);

    // Admin restricts the account
    env.mock_all_auths();
    client.restrict_merchant_account(&admin, &merchant, &true);

    // Verify Shade event
    assert_latest_account_restricted_event(&env, &client.address, &merchant, true, &admin);

    // Verify Account contract state changed
    assert_eq!(acct_client.is_restricted_account(), true);

    // Admin un-restricts the account
    client.restrict_merchant_account(&admin, &merchant, &false);
    assert_latest_account_restricted_event(&env, &client.address, &merchant, false, &admin);
    assert_eq!(acct_client.is_restricted_account(), false);
}

#[test]
fn test_manager_restrict_merchant_account_success() {
    let (env, client, acct_client, _admin, manager, merchant) = setup();

    // Manager restricts the account
    env.mock_all_auths();
    client.restrict_merchant_account(&manager, &merchant, &true);

    // Verify Shade event
    assert_latest_account_restricted_event(&env, &client.address, &merchant, true, &manager);

    // Verify Account contract state
    assert_eq!(acct_client.is_restricted_account(), true);
}

#[test]
fn test_unauthorized_restriction_attempt() {
    let (env, client, acct_client, _admin, _manager, merchant) = setup();

    let random_user = Address::generate(&env);

    // Attempt from random user
    // We don't use mock_all_auths here to ensure we hit the role-check logic correctly
    // However, try_ methods will still report the contract error if role check fails
    let res = client.try_restrict_merchant_account(&random_user, &merchant, &true);
    assert_eq!(
        res,
        Err(Ok(soroban_sdk::Error::from_contract_error(
            ContractError::NotAuthorized as u32
        )))
    );

    // Attempt from the merchant themselves
    let res = client.try_restrict_merchant_account(&merchant, &merchant, &true);
    assert_eq!(
        res,
        Err(Ok(soroban_sdk::Error::from_contract_error(
            ContractError::NotAuthorized as u32
        )))
    );

    // Verify state did not change
    assert_eq!(acct_client.is_restricted_account(), false);
}

#[test]
fn test_invalid_merchant_restriction() {
    let (env, client, _acct_client, admin, _manager, _merchant) = setup();
    let invalid_merchant = Address::generate(&env);

    env.mock_all_auths();
    let res = client.try_restrict_merchant_account(&admin, &invalid_merchant, &true);
    assert_eq!(
        res,
        Err(Ok(soroban_sdk::Error::from_contract_error(
            ContractError::MerchantNotFound as u32
        )))
    );
}
