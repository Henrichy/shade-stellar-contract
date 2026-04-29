use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, String};

const MONTHLY: u64 = 2_592_000; // 30 days
const PLAN_AMOUNT: i128 = 1_000;

struct Fixture<'a> {
    env: Env,
    contract: Address,
    client: SubscriptionContractClient<'a>,
    merchant: Address,
    customer: Address,
    token: Address,
    sub_id: u64,
}

fn fund(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn approve(env: &Env, token: &Address, owner: &Address, spender: &Address, amount: i128) {
    let expiry = env.ledger().sequence() + 1_000_000;
    TokenClient::new(env, token).approve(owner, spender, &amount, &expiry);
}

fn balance(env: &Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(env, token).balance(who)
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|l| {
        l.timestamp += seconds;
    });
}

/// Create a subscription that has already been charged once. The merchant
/// has the plan amount in their balance and the customer has nothing.
fn setup_charged_subscription() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    // Avoid timestamp-zero collision with the "never charged" sentinel.
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract = env.register(SubscriptionContract, ());
    let client = SubscriptionContractClient::new(&env, &contract);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.add_accepted_token(&token);

    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    let plan_id = client.create_plan(
        &merchant,
        &String::from_str(&env, "Pro Plan"),
        &token,
        &PLAN_AMOUNT,
        &MONTHLY,
    );
    let sub_id = client.subscribe(&customer, &plan_id);

    // Customer pays for one cycle so the merchant has funds to refund from.
    fund(&env, &token, &customer, PLAN_AMOUNT);
    approve(&env, &token, &customer, &contract, PLAN_AMOUNT);
    client.charge(&sub_id);

    Fixture {
        env,
        contract,
        client,
        merchant,
        customer,
        token,
        sub_id,
    }
}

// ── quote_prorated_refund: pure-math previews ──────────────────────────────────

#[test]
fn test_quote_zero_when_never_charged() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    let contract = env.register(SubscriptionContract, ());
    let client = SubscriptionContractClient::new(&env, &contract);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.add_accepted_token(&token);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let plan_id = client.create_plan(
        &merchant,
        &String::from_str(&env, "Plan"),
        &token,
        &PLAN_AMOUNT,
        &MONTHLY,
    );
    let sub_id = client.subscribe(&customer, &plan_id);

    assert_eq!(client.quote_prorated_refund(&sub_id), 0);
}

#[test]
fn test_quote_full_refund_immediately_after_charge() {
    let f = setup_charged_subscription();
    // No time has elapsed → entire cycle is unused.
    assert_eq!(f.client.quote_prorated_refund(&f.sub_id), PLAN_AMOUNT);
}

#[test]
fn test_quote_half_refund_at_mid_cycle() {
    let f = setup_charged_subscription();
    advance_time(&f.env, MONTHLY / 2);
    let quote = f.client.quote_prorated_refund(&f.sub_id);
    // Allow ±1 due to integer division on odd intervals.
    assert!(
        (quote - PLAN_AMOUNT / 2).abs() <= 1,
        "expected ~{} got {}",
        PLAN_AMOUNT / 2,
        quote
    );
}

#[test]
fn test_quote_zero_after_full_cycle_elapsed() {
    let f = setup_charged_subscription();
    advance_time(&f.env, MONTHLY + 1);
    assert_eq!(f.client.quote_prorated_refund(&f.sub_id), 0);
}

// ── cancel_with_prorated_refund: behaviour ─────────────────────────────────────

#[test]
fn test_cancel_with_refund_transfers_unused_portion_to_customer() {
    let f = setup_charged_subscription();
    advance_time(&f.env, MONTHLY / 2);

    // Merchant approves the contract to refund up to one cycle worth.
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);

    let merchant_before = balance(&f.env, &f.token, &f.merchant);
    let customer_before = balance(&f.env, &f.token, &f.customer);
    let expected = f.client.quote_prorated_refund(&f.sub_id);

    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);

    assert_eq!(
        balance(&f.env, &f.token, &f.merchant),
        merchant_before - expected
    );
    assert_eq!(
        balance(&f.env, &f.token, &f.customer),
        customer_before + expected
    );
    assert_eq!(
        f.client.get_subscription(&f.sub_id).status,
        SubscriptionStatus::Cancelled
    );
}

#[test]
fn test_merchant_can_initiate_cancellation_refund() {
    let f = setup_charged_subscription();
    advance_time(&f.env, MONTHLY / 4);
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);

    let expected = f.client.quote_prorated_refund(&f.sub_id);
    f.client.cancel_with_prorated_refund(&f.merchant, &f.sub_id);

    assert_eq!(balance(&f.env, &f.token, &f.customer), expected);
    assert_eq!(
        f.client.get_subscription(&f.sub_id).status,
        SubscriptionStatus::Cancelled
    );
}

#[test]
fn test_full_refund_when_cancelled_at_start_of_cycle() {
    let f = setup_charged_subscription();
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);

    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);
    assert_eq!(balance(&f.env, &f.token, &f.customer), PLAN_AMOUNT);
    assert_eq!(balance(&f.env, &f.token, &f.merchant), 0);
}

// ── Authorization & error paths ────────────────────────────────────────────────

#[test]
#[should_panic]
fn test_third_party_cannot_cancel_with_refund() {
    let f = setup_charged_subscription();
    let stranger = Address::generate(&f.env);
    advance_time(&f.env, MONTHLY / 4);
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);

    f.client.cancel_with_prorated_refund(&stranger, &f.sub_id);
}

#[test]
#[should_panic]
fn test_refund_panics_without_merchant_allowance() {
    let f = setup_charged_subscription();
    advance_time(&f.env, MONTHLY / 4);
    // Merchant did NOT approve the contract → transfer_from must panic.
    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);
}

#[test]
#[should_panic]
fn test_refund_panics_when_nothing_to_refund() {
    let f = setup_charged_subscription();
    advance_time(&f.env, MONTHLY + 1);
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);

    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);
}

#[test]
#[should_panic]
fn test_cannot_refund_already_cancelled() {
    let f = setup_charged_subscription();
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);
    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);
    // Second call must panic.
    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);
}

#[test]
fn test_can_refund_from_past_due_state() {
    let f = setup_charged_subscription();
    f.client.set_plan_grace_period(
        &f.merchant,
        &f.client.get_subscription(&f.sub_id).plan_id,
        &86_400,
    );
    advance_time(&f.env, MONTHLY + 100); // cycle elapsed
    f.client.process_charge(&f.sub_id); // missing allowance → PastDue

    // Now move back to give a non-zero refund window — start a fresh cycle.
    // Approve & charge once more, then verify a mid-cycle refund works
    // even after a brief PastDue blip.
    fund(&f.env, &f.token, &f.customer, PLAN_AMOUNT);
    approve(&f.env, &f.token, &f.customer, &f.contract, PLAN_AMOUNT);
    f.client.process_charge(&f.sub_id); // Recovered → Active
    advance_time(&f.env, MONTHLY / 4);
    approve(&f.env, &f.token, &f.merchant, &f.contract, PLAN_AMOUNT);

    let quote = f.client.quote_prorated_refund(&f.sub_id);
    assert!(quote > 0);
    f.client.cancel_with_prorated_refund(&f.customer, &f.sub_id);
    assert_eq!(
        f.client.get_subscription(&f.sub_id).status,
        SubscriptionStatus::Cancelled
    );
}
