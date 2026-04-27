#![cfg(test)]

use super::*;
use soroban_sdk::token;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup(env: &Env, amount: i128) -> (EscrowContractClient, Address, Address, Address, Address) {
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(env, &contract_id);
    let buyer = Address::generate(env);
    let seller = Address::generate(env);
    let arbiter = Address::generate(env);
    let terms = String::from_str(env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();
    client.init(&buyer, &seller, &arbiter, &terms, &token, &amount);
    token::StellarAssetClient::new(env, &token).mint(&contract_id, &amount);
    (client, buyer, seller, arbiter, token)
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
    client.init(&buyer, &seller, &arbiter, &terms, &token, &7500i128);
    assert_eq!(client.buyer(), buyer);
    assert_eq!(client.seller(), seller);
    assert_eq!(client.arbiter(), arbiter);
    assert_eq!(client.terms(), terms);
    assert_eq!(client.token(), token);
    assert_eq!(client.amount(), 7500);
    assert_eq!(client.status(), EscrowStatus::Pending);
}

#[test]
fn buyer_can_approve_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _buyer, seller, _arbiter, token) = setup(&env, 5000);
    client.approve_release();
    assert_eq!(client.status(), EscrowStatus::Completed);
    assert_eq!(token::StellarAssetClient::new(&env, &token).balance(&seller), 5000);
}

#[test]
fn buyer_can_open_dispute_and_arbiter_resolve() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, buyer, _seller, _arbiter, token) = setup(&env, 9000);
    client.open_dispute();
    assert_eq!(client.status(), EscrowStatus::Disputed);
    client.resolve_dispute(&true);
    assert_eq!(client.status(), EscrowStatus::Resolved);
    assert_eq!(token::StellarAssetClient::new(&env, &token).balance(&buyer), 9000);
}

#[test]
fn arbiter_can_split_funds_evenly() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, buyer, seller, _arbiter, token) = setup(&env, 1000);
    client.open_dispute();
    client.resolve(&500, &500);
    assert_eq!(client.status(), EscrowStatus::Resolved);
    let tc = token::StellarAssetClient::new(&env, &token);
    assert_eq!(tc.balance(&buyer), 500);
    assert_eq!(tc.balance(&seller), 500);
}

#[test]
fn arbiter_can_award_all_to_buyer() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, buyer, seller, _arbiter, token) = setup(&env, 800);
    client.open_dispute();
    client.resolve(&800, &0);
    assert_eq!(client.status(), EscrowStatus::Resolved);
    let tc = token::StellarAssetClient::new(&env, &token);
    assert_eq!(tc.balance(&buyer), 800);
    assert_eq!(tc.balance(&seller), 0);
}

#[test]
fn arbiter_can_award_all_to_seller() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, buyer, seller, _arbiter, token) = setup(&env, 600);
    client.open_dispute();
    client.resolve(&0, &600);
    assert_eq!(client.status(), EscrowStatus::Resolved);
    let tc = token::StellarAssetClient::new(&env, &token);
    assert_eq!(tc.balance(&buyer), 0);
    assert_eq!(tc.balance(&seller), 600);
}

#[test]
#[should_panic(expected = "buyer_amount + seller_amount must equal total escrowed amount")]
fn resolve_panics_if_amounts_dont_sum_to_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _buyer, _seller, _arbiter, _token) = setup(&env, 1000);
    client.open_dispute();
    client.resolve(&400, &400); // 800 != 1000
}

#[test]
#[should_panic(expected = "escrow dispute is not open")]
fn resolve_panics_if_not_disputed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _buyer, _seller, _arbiter, _token) = setup(&env, 1000);
    // still Pending, not Disputed
    client.resolve(&500, &500);
}