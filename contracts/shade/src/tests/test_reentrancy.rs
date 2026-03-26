#![cfg(test)]

use crate::components::admin as admin_component;
use crate::components::reentrancy;
use crate::errors::ContractError;
use crate::shade::Shade;
use crate::shade::ShadeClient;
use crate::types::DataKey;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, Vec};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn setup_test() -> (Env, ShadeClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, contract_id, admin)
}

fn setup_with_token() -> (Env, ShadeClient<'static>, Address, Address, Address) {
    let (env, client, contract_id, admin) = setup_test();
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.add_accepted_token(&admin, &token);
    (env, client, contract_id, admin, token)
}

// ── Unit tests for reentrancy::enter / reentrancy::exit ──────────────────────

#[test]
fn test_enter_sets_reentrancy_flag() {
    let (env, _client, contract_id, _admin) = setup_test();

    env.as_contract(&contract_id, || {
        assert!(!env.storage().persistent().has(&DataKey::ReentrancyStatus));
        reentrancy::enter(&env);
        assert!(env.storage().persistent().has(&DataKey::ReentrancyStatus));
    });
}

#[test]
fn test_exit_clears_reentrancy_flag() {
    let (env, _client, contract_id, _admin) = setup_test();

    env.as_contract(&contract_id, || {
        reentrancy::enter(&env);
        assert!(env.storage().persistent().has(&DataKey::ReentrancyStatus));
        reentrancy::exit(&env);
        assert!(!env.storage().persistent().has(&DataKey::ReentrancyStatus));
    });
}

#[test]
fn test_enter_exit_cycle_is_reusable() {
    let (env, _client, contract_id, _admin) = setup_test();

    env.as_contract(&contract_id, || {
        // First cycle
        reentrancy::enter(&env);
        reentrancy::exit(&env);

        // Second cycle — should work without error
        reentrancy::enter(&env);
        reentrancy::exit(&env);

        assert!(!env.storage().persistent().has(&DataKey::ReentrancyStatus));
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_double_enter_panics_with_reentrancy_error() {
    let (env, _client, contract_id, _admin) = setup_test();

    env.as_contract(&contract_id, || {
        reentrancy::enter(&env);
        // Second enter should panic with ContractError::Reentrancy (#4)
        reentrancy::enter(&env);
    });
}

#[test]
fn test_exit_without_enter_is_safe() {
    let (env, _client, contract_id, _admin) = setup_test();

    // Calling exit when no flag is set should not panic
    env.as_contract(&contract_id, || {
        reentrancy::exit(&env);
        assert!(!env.storage().persistent().has(&DataKey::ReentrancyStatus));
    });
}

// ── Integration tests: reentrancy guard on protected admin functions ─────────
// These tests simulate reentrancy by manually setting the ReentrancyStatus flag
// before calling the protected function, verifying it panics with error #4.

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_add_accepted_token_blocks_reentrant_call() {
    let (env, _client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
        admin_component::add_accepted_token(&env, &admin, &token);
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_add_accepted_tokens_blocks_reentrant_call() {
    let (env, _client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let mut tokens = Vec::new(&env);
    tokens.push_back(token1);
    tokens.push_back(token2);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
        admin_component::add_accepted_tokens(&env, &admin, &tokens);
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_remove_accepted_token_blocks_reentrant_call() {
    let (env, _client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // First add the token normally
    env.as_contract(&contract_id, || {
        admin_component::add_accepted_token(&env, &admin, &token);
    });

    // Now simulate reentrancy during remove
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
        admin_component::remove_accepted_token(&env, &admin, &token);
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_set_account_wasm_hash_blocks_reentrant_call() {
    let (env, _client, contract_id, admin) = setup_test();

    let wasm_hash = BytesN::from_array(&env, &[0u8; 32]);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
        admin_component::set_account_wasm_hash(&env, &admin, &wasm_hash);
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_set_fee_blocks_reentrant_call() {
    let (env, _client, contract_id, admin, token) = setup_with_token();

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
        admin_component::set_fee(&env, &admin, &token, 500);
    });
}

// ── Reentrancy via client try_ methods (verifies exact error code) ───────────

#[test]
fn test_add_accepted_token_reentrancy_returns_error_code_4() {
    let (env, client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Set the reentrancy guard from within the contract context
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::Reentrancy as u32);
    let result = client.try_add_accepted_token(&admin, &token);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_remove_accepted_token_reentrancy_returns_error_code_4() {
    let (env, client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Add the token first, then set the guard
    client.add_accepted_token(&admin, &token);
    assert!(client.is_accepted_token(&token));

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::Reentrancy as u32);
    let result = client.try_remove_accepted_token(&admin, &token);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_set_fee_reentrancy_returns_error_code_4() {
    let (env, client, contract_id, admin, token) = setup_with_token();

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::Reentrancy as u32);
    let result = client.try_set_fee(&admin, &token, &500);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

#[test]
fn test_set_account_wasm_hash_reentrancy_returns_error_code_4() {
    let (env, client, contract_id, admin) = setup_test();

    let wasm_hash = BytesN::from_array(&env, &[0u8; 32]);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::Reentrancy as u32);
    let result = client.try_set_account_wasm_hash(&admin, &wasm_hash);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));
}

// ── State integrity: blocked calls must not mutate state ─────────────────────

#[test]
fn test_state_unchanged_after_blocked_reentrant_add_token() {
    let (env, client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    assert!(!client.is_accepted_token(&token));

    // Attempt reentrant add — should fail
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let _ = client.try_add_accepted_token(&admin, &token);

    // Clean up guard to allow state inspection
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::ReentrancyStatus);
    });

    // Token must not have been added
    assert!(!client.is_accepted_token(&token));
}

#[test]
fn test_state_unchanged_after_blocked_reentrant_set_fee() {
    let (env, client, contract_id, admin, token) = setup_with_token();

    // Set a known fee first
    client.set_fee(&admin, &token, &200);
    assert_eq!(client.get_fee(&token), 200);

    // Set guard and attempt to change fee
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let _ = client.try_set_fee(&admin, &token, &999);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::ReentrancyStatus);
    });

    // Fee should remain at the original value
    assert_eq!(client.get_fee(&token), 200);
}

#[test]
fn test_state_unchanged_after_blocked_reentrant_remove_token() {
    let (env, client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.add_accepted_token(&admin, &token);
    assert!(client.is_accepted_token(&token));

    // Set guard and attempt to remove token
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let _ = client.try_remove_accepted_token(&admin, &token);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::ReentrancyStatus);
    });

    // Token should still be accepted
    assert!(client.is_accepted_token(&token));
}

#[test]
fn test_batch_add_state_unchanged_after_reentrancy_block() {
    let (env, client, contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let mut tokens = Vec::new(&env);
    tokens.push_back(token1.clone());
    tokens.push_back(token2.clone());

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyStatus, &true);
    });

    let _ = client.try_add_accepted_tokens(&admin, &tokens);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::ReentrancyStatus);
    });

    // Neither token should have been added
    assert!(!client.is_accepted_token(&token1));
    assert!(!client.is_accepted_token(&token2));
}

// ── Guard cleanup: normal operations work after guard is properly released ───

#[test]
fn test_normal_operations_succeed_after_guard_released() {
    let (env, client, _contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token1 = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token2 = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // First operation completes and releases the guard
    client.add_accepted_token(&admin, &token1);
    assert!(client.is_accepted_token(&token1));

    // Second operation should also succeed (guard was released)
    client.add_accepted_token(&admin, &token2);
    assert!(client.is_accepted_token(&token2));
}

#[test]
fn test_sequential_guarded_operations_all_succeed() {
    let (env, client, _contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Run multiple guarded operations in sequence
    client.add_accepted_token(&admin, &token);
    assert!(client.is_accepted_token(&token));

    client.set_fee(&admin, &token, &300);
    assert_eq!(client.get_fee(&token), 300);

    client.remove_accepted_token(&admin, &token);
    assert!(!client.is_accepted_token(&token));
}

#[test]
fn test_different_guarded_functions_in_sequence() {
    let (env, client, _contract_id, admin) = setup_test();

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // add_accepted_token → set_fee → set_account_wasm_hash
    // Each must release the guard for the next to succeed.
    client.add_accepted_token(&admin, &token);
    client.set_fee(&admin, &token, &100);

    let wasm_hash = BytesN::from_array(&env, &[1u8; 32]);
    client.set_account_wasm_hash(&admin, &wasm_hash);

    // All operations completed — state reflects all changes
    assert!(client.is_accepted_token(&token));
    assert_eq!(client.get_fee(&token), 100);
}

// ── Error code verification ──────────────────────────────────────────────────

#[test]
fn test_reentrancy_error_code_value() {
    // Verify the error code is #4 as documented
    assert_eq!(ContractError::Reentrancy as u32, 4);
}

#[test]
fn test_multiple_enter_exit_cycles_stress() {
    let (env, _client, contract_id, _admin) = setup_test();

    env.as_contract(&contract_id, || {
        for _ in 0..10 {
            reentrancy::enter(&env);
            assert!(env.storage().persistent().has(&DataKey::ReentrancyStatus));
            reentrancy::exit(&env);
            assert!(!env.storage().persistent().has(&DataKey::ReentrancyStatus));
        }
    });
}
