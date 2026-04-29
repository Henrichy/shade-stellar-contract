use super::*;
use crate::types::ChargeOutcome;
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
    plan_id: u64,
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

fn setup_with_grace(grace_period: u64) -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();

    // The "never charged" sentinel is `last_charged == 0`, so all tests must
    // run from a non-zero timestamp to avoid collisions when a charge lands.
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
    if grace_period > 0 {
        client.set_plan_grace_period(&merchant, &plan_id, &grace_period);
    }

    let sub_id = client.subscribe(&customer, &plan_id);

    Fixture {
        env,
        contract,
        client,
        merchant,
        customer,
        token,
        plan_id,
        sub_id,
    }
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|l| {
        l.timestamp += seconds;
    });
}

// ── Grace-period configuration ─────────────────────────────────────────────────

#[test]
fn test_default_plan_grace_period_is_zero() {
    let f = setup_with_grace(0);
    let plan = f.client.get_plan(&f.plan_id);
    assert_eq!(plan.grace_period, 0);
}

#[test]
fn test_set_plan_grace_period_updates_value() {
    let f = setup_with_grace(0);
    f.client
        .set_plan_grace_period(&f.merchant, &f.plan_id, &86_400);
    assert_eq!(f.client.get_plan(&f.plan_id).grace_period, 86_400);
}

#[test]
#[should_panic]
fn test_non_merchant_cannot_set_grace_period() {
    let f = setup_with_grace(0);
    let imposter = Address::generate(&f.env);
    f.client
        .set_plan_grace_period(&imposter, &f.plan_id, &86_400);
}

// ── Process charge outcomes ────────────────────────────────────────────────────

#[test]
fn test_first_charge_with_sufficient_allowance_returns_charged() {
    let f = setup_with_grace(86_400);
    fund(&f.env, &f.token, &f.customer, 5_000);
    approve(&f.env, &f.token, &f.customer, &f.contract, 5_000);

    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::Charged);
    assert_eq!(balance(&f.env, &f.token, &f.merchant), PLAN_AMOUNT);
}

#[test]
fn test_charge_before_interval_returns_not_due_yet() {
    let f = setup_with_grace(86_400);
    fund(&f.env, &f.token, &f.customer, 5_000);
    approve(&f.env, &f.token, &f.customer, &f.contract, 5_000);

    f.client.process_charge(&f.sub_id);
    // Don't advance — the next call is before the interval has elapsed.
    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::NotDueYet);
}

#[test]
fn test_failed_charge_with_zero_grace_terminates_immediately() {
    let f = setup_with_grace(0); // No grace.
                                 // No allowance → charge cannot succeed.

    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::Terminated);
    assert_eq!(
        f.client.get_subscription(&f.sub_id).status,
        SubscriptionStatus::Terminated
    );
}

#[test]
fn test_failed_charge_with_grace_enters_past_due() {
    let f = setup_with_grace(86_400);
    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::EnteredGrace);

    let sub = f.client.get_subscription(&f.sub_id);
    assert_eq!(sub.status, SubscriptionStatus::PastDue);
    assert_eq!(sub.past_due_since, f.env.ledger().timestamp());
}

#[test]
fn test_past_due_recovery_when_allowance_restored() {
    let f = setup_with_grace(86_400);
    // Step 1: charge fails → PastDue.
    f.client.process_charge(&f.sub_id);

    // Step 2: customer tops up & re-approves within the grace window.
    advance_time(&f.env, 3_600); // 1 hour later, still inside 24h grace.
    fund(&f.env, &f.token, &f.customer, 5_000);
    approve(&f.env, &f.token, &f.customer, &f.contract, 5_000);

    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::Recovered);

    let sub = f.client.get_subscription(&f.sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Active);
    assert_eq!(sub.past_due_since, 0);
    assert_eq!(balance(&f.env, &f.token, &f.merchant), PLAN_AMOUNT);
}

#[test]
fn test_past_due_terminates_after_grace_expires() {
    let f = setup_with_grace(86_400);
    f.client.process_charge(&f.sub_id); // → PastDue

    // Advance past the grace window without recovery.
    advance_time(&f.env, 86_401);

    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::Terminated);
    assert_eq!(
        f.client.get_subscription(&f.sub_id).status,
        SubscriptionStatus::Terminated
    );
}

#[test]
fn test_past_due_within_grace_keeps_state_unchanged_when_still_short() {
    let f = setup_with_grace(86_400);
    f.client.process_charge(&f.sub_id); // → PastDue
    let entered_at = f.client.get_subscription(&f.sub_id).past_due_since;

    advance_time(&f.env, 3_600);
    let outcome = f.client.process_charge(&f.sub_id);
    assert_eq!(outcome, ChargeOutcome::EnteredGrace);

    // past_due_since is preserved across re-checks within the window.
    assert_eq!(
        f.client.get_subscription(&f.sub_id).past_due_since,
        entered_at
    );
}

#[test]
#[should_panic]
fn test_process_charge_panics_on_terminated() {
    let f = setup_with_grace(0);
    f.client.process_charge(&f.sub_id); // → Terminated
                                        // Calling again must panic — terminated is final.
    f.client.process_charge(&f.sub_id);
}

#[test]
#[should_panic]
fn test_process_charge_panics_on_cancelled() {
    let f = setup_with_grace(86_400);
    f.client.cancel_subscription(&f.customer, &f.sub_id);
    f.client.process_charge(&f.sub_id);
}

// ── enforce_grace ──────────────────────────────────────────────────────────────

#[test]
fn test_enforce_grace_terminates_after_window() {
    let f = setup_with_grace(86_400);
    f.client.process_charge(&f.sub_id); // → PastDue
    advance_time(&f.env, 86_401);

    f.client.enforce_grace(&f.sub_id);
    assert_eq!(
        f.client.get_subscription(&f.sub_id).status,
        SubscriptionStatus::Terminated
    );
}

#[test]
fn test_enforce_grace_is_idempotent_on_already_terminated() {
    let f = setup_with_grace(86_400);
    f.client.process_charge(&f.sub_id);
    advance_time(&f.env, 86_401);
    f.client.enforce_grace(&f.sub_id);
    // Second call is a no-op (no panic).
    f.client.enforce_grace(&f.sub_id);
}

#[test]
#[should_panic]
fn test_enforce_grace_panics_during_grace_window() {
    let f = setup_with_grace(86_400);
    f.client.process_charge(&f.sub_id);
    advance_time(&f.env, 1_000); // still inside grace window
    f.client.enforce_grace(&f.sub_id);
}

#[test]
#[should_panic]
fn test_enforce_grace_panics_when_active() {
    let f = setup_with_grace(86_400);
    fund(&f.env, &f.token, &f.customer, 5_000);
    approve(&f.env, &f.token, &f.customer, &f.contract, 5_000);
    f.client.process_charge(&f.sub_id);
    // Subscription is Active, not PastDue → cannot enforce grace.
    f.client.enforce_grace(&f.sub_id);
}

// ── Strict charge() respects state ─────────────────────────────────────────────

#[test]
#[should_panic]
fn test_strict_charge_panics_on_past_due() {
    let f = setup_with_grace(86_400);
    f.client.process_charge(&f.sub_id); // → PastDue
    f.client.charge(&f.sub_id);
}
