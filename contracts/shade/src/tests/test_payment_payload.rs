#![cfg(test)]

use crate::shade::{Shade, ShadeClient};
use crate::types::{PaymentPayload, PaymentRoute, SwapRoute};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};

fn setup_client() -> (Env, ShadeClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    (env, client)
}

#[test]
fn test_validate_payment_payload_accepts_direct_payment() {
    let (env, client) = setup_client();
    let token = Address::generate(&env);

    let payload = PaymentPayload {
        input_token: token.clone(),
        settlement_token: token,
        route: PaymentRoute::Direct,
        max_slippage_bps: None,
    };

    client.validate_payment_payload(&payload);
}

#[test]
fn test_validate_payment_payload_accepts_swap_route() {
    let (env, client) = setup_client();
    let router = Address::generate(&env);
    let input_token = Address::generate(&env);
    let bridge_token = Address::generate(&env);
    let settlement_token = Address::generate(&env);

    let mut path = Vec::new(&env);
    path.push_back(input_token.clone());
    path.push_back(bridge_token);
    path.push_back(settlement_token.clone());

    let payload = PaymentPayload {
        input_token: input_token.clone(),
        settlement_token: settlement_token.clone(),
        route: PaymentRoute::Swap(SwapRoute { router, path }),
        max_slippage_bps: Some(250),
    };

    client.validate_payment_payload(&payload);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #44)")]
fn test_validate_payment_payload_rejects_invalid_swap_path() {
    let (env, client) = setup_client();
    let router = Address::generate(&env);
    let input_token = Address::generate(&env);
    let wrong_start = Address::generate(&env);
    let settlement_token = Address::generate(&env);

    let mut path = Vec::new(&env);
    path.push_back(wrong_start);
    path.push_back(settlement_token.clone());

    let payload = PaymentPayload {
        input_token,
        settlement_token,
        route: PaymentRoute::Swap(SwapRoute { router, path }),
        max_slippage_bps: Some(100),
    };

    client.validate_payment_payload(&payload);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #45)")]
fn test_validate_payment_payload_rejects_invalid_slippage() {
    let (env, client) = setup_client();
    let router = Address::generate(&env);
    let input_token = Address::generate(&env);
    let settlement_token = Address::generate(&env);

    let mut path = Vec::new(&env);
    path.push_back(input_token.clone());
    path.push_back(settlement_token.clone());

    let payload = PaymentPayload {
        input_token,
        settlement_token,
        route: PaymentRoute::Swap(SwapRoute { router, path }),
        max_slippage_bps: Some(10_001),
    };

    client.validate_payment_payload(&payload);
}
