#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn init_stores_roles_and_terms() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    assert_eq!(client.buyer(), buyer);
    assert_eq!(client.seller(), seller);
    assert_eq!(client.arbiter(), arbiter);
    assert_eq!(client.terms(), terms);
}

#[test]
fn resolve_to_buyer() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    let result = client.resolve(&arbiter, &buyer);
    assert_eq!(result, buyer);
}

#[test]
fn resolve_to_seller() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    let result = client.resolve(&arbiter, &seller);
    assert_eq!(result, seller);
}

#[test]
#[should_panic(expected = "unauthorized: only arbiter can resolve")]
fn resolve_unauthorized_arbiter() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let fake_arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    client.resolve(&fake_arbiter, &buyer);
}
