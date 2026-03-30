#![cfg(test)]

//! Comprehensive tests for `add_accepted_tokens` (batch token whitelisting).
//!
//! Covers:
//! - Batch addition of 5+ tokens in a single call
//! - TokenAdded event emitted for each new token
//! - Non-admin rejection
//! - Empty list is a no-op
//! - Duplicates within the batch are skipped
//! - Tokens already whitelisted are skipped (no duplicate event)
//! - Paused contract rejects batch addition
//! - Mixed batch (some new, some duplicate) only adds new ones

use crate::shade::{Shade, ShadeClient};
use crate::errors::ContractError;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{Address, Env, Map, Symbol, TryIntoVal, Val, Vec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_env() -> (Env, ShadeClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, contract_id, admin)
}

fn make_token(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env)).address()
}

fn make_tokens(env: &Env, n: u32) -> Vec<Address> {
    let mut tokens = Vec::new(env);
    for _ in 0..n {
        tokens.push_back(make_token(env));
    }
    tokens
}

/// Count how many "token_added_event" events were emitted.
fn count_token_added_events(env: &Env) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|(_, topics, _)| {
            if topics.is_empty() {
                return false;
            }
            let name: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(env);
            name.map(|s| s == Symbol::new(env, "TokenAddedEvent"))
                .unwrap_or(false)
        })
        .count()
}

/// Assert that the last event is a token_added_event for the given token.
fn assert_token_added_event(env: &Env, contract_id: &Address, expected_token: &Address) {
    let events = env.events().all();
    assert!(!events.is_empty(), "No events emitted");

    // Find the last token_added_event
    let token_event = events.iter().rev().find(|(cid, topics, _)| {
        if cid != contract_id || topics.is_empty() {
            return false;
        }
        let name: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(env);
        name.map(|s| s == Symbol::new(env, "TokenAddedEvent"))
            .unwrap_or(false)
    });

    assert!(token_event.is_some(), "No token_added_event found");
    let (_, _, data) = token_event.unwrap();
    let data_map: Map<Symbol, Val> = data.try_into_val(env).unwrap();
    let token_val = data_map.get(Symbol::new(env, "token")).unwrap();
    let token_in_event: Address = token_val.try_into_val(env).unwrap();
    assert_eq!(token_in_event, *expected_token);
}

// ---------------------------------------------------------------------------
// Success: batch addition
// ---------------------------------------------------------------------------

#[test]
fn test_batch_add_five_tokens_all_whitelisted() {
    let (env, client, _, admin) = setup_env();
    let tokens = make_tokens(&env, 5);

    client.add_accepted_tokens(&admin, &tokens);

    for i in 0..tokens.len() {
        assert!(client.is_accepted_token(&tokens.get(i).unwrap()));
    }
}

#[test]
fn test_batch_add_emits_event_for_each_token() {
    let (env, client, _, admin) = setup_env();
    let tokens = make_tokens(&env, 5);

    client.add_accepted_tokens(&admin, &tokens);

    assert_eq!(count_token_added_events(&env), 5);
}

#[test]
fn test_batch_add_single_token() {
    let (env, client, _, admin) = setup_env();
    let mut tokens = Vec::new(&env);
    let token = make_token(&env);
    tokens.push_back(token.clone());

    client.add_accepted_tokens(&admin, &tokens);

    panic!("Events: {:?}", env.events().all());
}

#[test]
fn test_batch_add_ten_tokens() {
    let (env, client, _, admin) = setup_env();
    let tokens = make_tokens(&env, 10);

    client.add_accepted_tokens(&admin, &tokens);

    assert_eq!(count_token_added_events(&env), 10);
    for i in 0..tokens.len() {
        assert!(client.is_accepted_token(&tokens.get(i).unwrap()));
    }
}

#[test]
fn test_batch_add_event_contains_correct_token_address() {
    let (env, client, contract_id, admin) = setup_env();
    let token = make_token(&env);
    let mut tokens = Vec::new(&env);
    tokens.push_back(token.clone());

    client.add_accepted_tokens(&admin, &tokens);

    assert_token_added_event(&env, &contract_id, &token);
}

// ---------------------------------------------------------------------------
// Empty list
// ---------------------------------------------------------------------------

#[test]
fn test_batch_add_empty_list_is_noop() {
    let (env, client, _, admin) = setup_env();
    let tokens: Vec<Address> = Vec::new(&env);

    client.add_accepted_tokens(&admin, &tokens);

    assert_eq!(count_token_added_events(&env), 0);
}

// ---------------------------------------------------------------------------
// Duplicates
// ---------------------------------------------------------------------------

#[test]
fn test_batch_add_skips_duplicates_within_batch() {
    let (env, client, _, admin) = setup_env();
    let token = make_token(&env);

    // Same token twice in the batch
    let mut tokens = Vec::new(&env);
    tokens.push_back(token.clone());
    tokens.push_back(token.clone());

    client.add_accepted_tokens(&admin, &tokens);

    // Only one event, token added once
    assert_eq!(count_token_added_events(&env), 1);
    assert!(client.is_accepted_token(&token));
}

#[test]
fn test_batch_add_skips_already_whitelisted_tokens() {
    let (env, client, _, admin) = setup_env();
    let token = make_token(&env);

    // Add token individually first
    client.add_accepted_token(&admin, &token);
    let events_before = count_token_added_events(&env);

    // Now batch-add the same token
    let mut tokens = Vec::new(&env);
    tokens.push_back(token.clone());
    client.add_accepted_tokens(&admin, &tokens);

    // No new event emitted
    assert_eq!(count_token_added_events(&env), events_before);
}

#[test]
fn test_batch_add_mixed_new_and_existing_only_adds_new() {
    let (env, client, _, admin) = setup_env();
    let existing = make_token(&env);
    let new1 = make_token(&env);
    let new2 = make_token(&env);

    client.add_accepted_token(&admin, &existing);
    let events_before = count_token_added_events(&env);

    let mut tokens = Vec::new(&env);
    tokens.push_back(existing.clone());
    tokens.push_back(new1.clone());
    tokens.push_back(new2.clone());

    client.add_accepted_tokens(&admin, &tokens);

    // Only 2 new events (new1 and new2)
    assert_eq!(count_token_added_events(&env) - events_before, 2);
    assert!(client.is_accepted_token(&existing));
    assert!(client.is_accepted_token(&new1));
    assert!(client.is_accepted_token(&new2));
}

// ---------------------------------------------------------------------------
// Access control
// ---------------------------------------------------------------------------

#[test]
fn test_non_admin_cannot_batch_add_tokens() {
    let (env, client, _, _admin) = setup_env();
    let non_admin = Address::generate(&env);
    let tokens = make_tokens(&env, 3);

    let expected_error =
        soroban_sdk::Error::from_contract_error(ContractError::NotAuthorized as u32);

    let result = client.try_add_accepted_tokens(&non_admin, &tokens);
    assert!(matches!(result, Err(Ok(err)) if err == expected_error));

    // Nothing was whitelisted
    for i in 0..tokens.len() {
        assert!(!client.is_accepted_token(&tokens.get(i).unwrap()));
    }
}

// ---------------------------------------------------------------------------
// Paused contract
// ---------------------------------------------------------------------------

#[test]
fn test_batch_add_fails_when_contract_is_paused() {
    let (env, client, _, admin) = setup_env();
    let tokens = make_tokens(&env, 2);

    client.pause(&admin);

    let result = client.try_add_accepted_tokens(&admin, &tokens);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_batch_add_same_list_twice_is_idempotent() {
    let (env, client, _, admin) = setup_env();
    let tokens = make_tokens(&env, 3);

    client.add_accepted_tokens(&admin, &tokens);
    let events_after_first = count_token_added_events(&env);

    // Second call with same tokens - no new events
    client.add_accepted_tokens(&admin, &tokens);
    assert_eq!(count_token_added_events(&env), events_after_first);

    // All still whitelisted
    for i in 0..tokens.len() {
        assert!(client.is_accepted_token(&tokens.get(i).unwrap()));
    }
}
