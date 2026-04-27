#![cfg(test)]

use super::*;
use soroban_sdk::token;
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String,
};

// Expiry 1 hour from "now" (ledger starts at 0 by default)
const EXPIRY: u64 = 3_600;

fn make_config(
    buyer: &Address,
    seller: &Address,
    arbiter: &Address,
    terms: &String,
    token: &Address,
    amount: i128,
) -> EscrowConfig {
    EscrowConfig {
        buyer: buyer.clone(),
        seller: seller.clone(),
        arbiter: arbiter.clone(),
        terms: terms.clone(),
        token: token.clone(),
        amount,
        expiry: EXPIRY,
    }
}

#[test]
fn init_stores_roles_terms_token_and_amount() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 7500));

    assert_eq!(client.buyer(), buyer);
    assert_eq!(client.seller(), seller);
    assert_eq!(client.arbiter(), arbiter);
    assert_eq!(client.terms(), terms);
    assert_eq!(client.token(), token);
    assert_eq!(client.amount(), 7500);
    assert_eq!(client.expiry(), EXPIRY);
    assert_eq!(client.status(), EscrowStatus::Pending);
}

#[test]
fn buyer_can_approve_release() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 5000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &5000);

    client.approve_release();

    assert_eq!(client.status(), EscrowStatus::Completed);
    assert_eq!(token_client.balance(&seller), 5000);
}

#[test]
fn buyer_can_open_dispute_and_arbiter_resolve() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 9000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &9000);

    client.open_dispute();
    assert_eq!(client.status(), EscrowStatus::Disputed);

    client.resolve_dispute(&true);

    assert_eq!(client.status(), EscrowStatus::Resolved);
    assert_eq!(token_client.balance(&buyer), 9000);
}

// ---------------------------------------------------------------------------
// Expiration & refund tests
// ---------------------------------------------------------------------------

/// Buyer claims refund after expiry — funds return to buyer, status Resolved.
#[test]
fn buyer_can_claim_refund_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    // Advance time past expiry
    env.ledger().set_timestamp(EXPIRY + 1);

    client.claim_refund();

    assert_eq!(client.status(), EscrowStatus::Resolved);
    assert_eq!(token_client.balance(&buyer), 4000);
    assert_eq!(token_client.balance(&contract_id), 0);
}

/// claim_refund at the exact expiry boundary (timestamp == expiry) must fail —
/// the lock requires strictly greater than expiry.
#[test]
#[should_panic(expected = "escrow has not expired yet")]
fn claim_refund_fails_at_exact_expiry_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    // Exactly at expiry — should still be locked
    env.ledger().set_timestamp(EXPIRY);

    client.claim_refund();
}

/// claim_refund before expiry must panic.
#[test]
#[should_panic(expected = "escrow has not expired yet")]
fn claim_refund_fails_before_expiry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    // Time has not advanced — ledger timestamp is 0, well before expiry
    client.claim_refund();
}

/// claim_refund on a completed escrow must panic — not pending.
#[test]
#[should_panic(expected = "escrow is not pending")]
fn claim_refund_fails_if_already_completed() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    // Buyer approves release first — status becomes Completed
    client.approve_release();
    assert_eq!(client.status(), EscrowStatus::Completed);

    // Now try to claim refund after expiry — should fail because not Pending
    env.ledger().set_timestamp(EXPIRY + 1);
    client.claim_refund();
}

/// Only the buyer can claim a refund — seller cannot.
#[test]
#[should_panic]
fn claim_refund_fails_if_caller_is_not_buyer() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    env.mock_all_auths();
    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    env.ledger().set_timestamp(EXPIRY + 1);

    // Only authorize the seller — buyer.require_auth() will fail
    env.mock_auths(&[MockAuth {
        address: &seller,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "claim_refund",
            args: ().into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.claim_refund();
}
