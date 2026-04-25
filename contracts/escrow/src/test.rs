#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, String,
};

fn setup_escrow(
    env: &Env,
    expires_at: u64,
) -> (
    EscrowContractClient,
    Address, // buyer
    Address, // seller
    Address, // arbiter
    Address, // token contract
) {
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(env, &contract_id);

    let buyer = Address::generate(env);
    let seller = Address::generate(env);
    let arbiter = Address::generate(env);

    // Deploy a test token and mint to the escrow contract
    let token_admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_id.address();
    let asset_client = StellarAssetClient::new(env, &token_address);
    asset_client.mint(&contract_id, &1000);

    let terms = String::from_str(env, "Deliver goods by deadline");

    client.init(
        &buyer,
        &seller,
        &arbiter,
        &terms,
        &token_address,
        &1000,
        &expires_at,
    );

    (client, buyer, seller, arbiter, token_address)
}

#[test]
fn init_stores_roles_and_terms() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, seller, arbiter, _token) = setup_escrow(&env, 2000);

    assert_eq!(client.buyer(), buyer);
    assert_eq!(client.seller(), seller);
    assert_eq!(client.arbiter(), arbiter);
    assert_eq!(client.terms(), String::from_str(&env, "Deliver goods by deadline"));
    assert_eq!(client.expires_at(), 2000);
    assert!(!client.is_refunded());
}

#[test]
fn claim_refund_succeeds_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, _seller, _arbiter, token_address) = setup_escrow(&env, 2000);

    // Advance time past expiry
    env.ledger().set_timestamp(3000);

    client.claim_refund(&buyer);

    assert!(client.is_refunded());

    // Verify buyer received the funds
    let token_client = TokenClient::new(&env, &token_address);
    assert_eq!(token_client.balance(&buyer), 1000);
}

#[test]
#[should_panic(expected = "escrow has not expired yet")]
fn claim_refund_fails_before_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, _seller, _arbiter, _token) = setup_escrow(&env, 2000);

    // Still before expiry
    env.ledger().set_timestamp(1500);
    client.claim_refund(&buyer);
}

#[test]
#[should_panic(expected = "only the buyer can claim a refund")]
fn claim_refund_fails_for_non_buyer() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _buyer, seller, _arbiter, _token) = setup_escrow(&env, 2000);

    env.ledger().set_timestamp(3000);
    client.claim_refund(&seller);
}

#[test]
#[should_panic(expected = "refund already claimed")]
fn claim_refund_fails_if_already_refunded() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, _seller, _arbiter, _token) = setup_escrow(&env, 2000);

    env.ledger().set_timestamp(3000);
    client.claim_refund(&buyer);
    // Second claim should panic
    client.claim_refund(&buyer);
}

#[test]
#[should_panic(expected = "expires_at must be in the future")]
fn init_fails_with_past_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(5000);

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_id.address();

    client.init(
        &buyer,
        &seller,
        &arbiter,
        &String::from_str(&env, "terms"),
        &token_address,
        &100,
        &1000, // in the past
    );
}
