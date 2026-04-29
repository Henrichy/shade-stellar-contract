#![no_std]

mod errors;
#[cfg(test)]
mod test_grace;
#[cfg(test)]
mod test_refund;
mod types;

use errors::SubscriptionError;
use soroban_sdk::{contract, contractimpl, panic_with_error, token, Address, Env, String, Vec};
use types::{ChargeOutcome, DataKey, Plan, Subscription, SubscriptionStatus};

fn require_admin(env: &Env) -> Address {
    let admin: Address = env
        .storage()
        .persistent()
        .get(&DataKey::Admin)
        .unwrap_or_else(|| panic_with_error!(env, SubscriptionError::NotInitialized));
    admin.require_auth();
    admin
}

fn get_plan_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::PlanCount)
        .unwrap_or(0)
}

fn get_subscription_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::SubscriptionCount)
        .unwrap_or(0)
}

fn load_plan(env: &Env, plan_id: u64) -> Plan {
    env.storage()
        .persistent()
        .get(&DataKey::Plan(plan_id))
        .unwrap_or_else(|| panic_with_error!(env, SubscriptionError::PlanNotFound))
}

fn load_subscription(env: &Env, sub_id: u64) -> Subscription {
    env.storage()
        .persistent()
        .get(&DataKey::Subscription(sub_id))
        .unwrap_or_else(|| panic_with_error!(env, SubscriptionError::SubscriptionNotFound))
}

#[contract]
pub struct SubscriptionContract;

#[contractimpl]
impl SubscriptionContract {
    // ── Admin ─────────────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic_with_error!(&env, SubscriptionError::AlreadyInitialized);
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
    }

    pub fn add_accepted_token(env: Env, token: Address) {
        require_admin(&env);
        let mut tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AcceptedTokens)
            .unwrap_or_else(|| Vec::new(&env));
        if !tokens.contains(&token) {
            tokens.push_back(token);
            env.storage()
                .persistent()
                .set(&DataKey::AcceptedTokens, &tokens);
        }
    }

    pub fn is_accepted_token(env: Env, token: Address) -> bool {
        let tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AcceptedTokens)
            .unwrap_or_else(|| Vec::new(&env));
        tokens.contains(&token)
    }

    // ── Plans ─────────────────────────────────────────────────────────────────

    /// Create a recurring billing plan.  Returns the new plan ID.
    pub fn create_plan(
        env: Env,
        merchant: Address,
        description: String,
        token: Address,
        amount: i128,
        interval: u64,
    ) -> u64 {
        merchant.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, SubscriptionError::InvalidAmount);
        }
        if interval == 0 {
            panic_with_error!(&env, SubscriptionError::InvalidInterval);
        }

        let accepted: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AcceptedTokens)
            .unwrap_or_else(|| Vec::new(&env));
        if !accepted.contains(&token) {
            panic_with_error!(&env, SubscriptionError::TokenNotAccepted);
        }

        let plan_id = get_plan_count(&env) + 1;
        env.storage()
            .persistent()
            .set(&DataKey::PlanCount, &plan_id);

        let plan = Plan {
            id: plan_id,
            merchant,
            description,
            token,
            amount,
            interval,
            active: true,
            created_at: env.ledger().timestamp(),
            grace_period: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Plan(plan_id), &plan);
        plan_id
    }

    /// Set the grace period (in seconds) for a plan. When a charge fails
    /// for insufficient allowance, the subscription enters `PastDue` for
    /// this many seconds before being terminated. Only the plan merchant
    /// may call this.
    pub fn set_plan_grace_period(env: Env, merchant: Address, plan_id: u64, grace_period: u64) {
        merchant.require_auth();
        let mut plan = load_plan(&env, plan_id);
        if plan.merchant != merchant {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        plan.grace_period = grace_period;
        env.storage()
            .persistent()
            .set(&DataKey::Plan(plan_id), &plan);
    }

    pub fn get_plan(env: Env, plan_id: u64) -> Plan {
        load_plan(&env, plan_id)
    }

    pub fn get_plan_count(env: Env) -> u64 {
        get_plan_count(&env)
    }

    /// Update the billing amount for an existing plan.
    /// Only the plan's merchant may call this; existing subscriptions are not
    /// retroactively affected until the next charge cycle.
    pub fn update_plan_amount(env: Env, merchant: Address, plan_id: u64, new_amount: i128) {
        merchant.require_auth();
        if new_amount <= 0 {
            panic_with_error!(&env, SubscriptionError::InvalidAmount);
        }
        let mut plan = load_plan(&env, plan_id);
        if plan.merchant != merchant {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        plan.amount = new_amount;
        env.storage()
            .persistent()
            .set(&DataKey::Plan(plan_id), &plan);
    }

    /// Update the billing interval for an existing plan (in seconds).
    pub fn update_plan_interval(env: Env, merchant: Address, plan_id: u64, new_interval: u64) {
        merchant.require_auth();
        if new_interval == 0 {
            panic_with_error!(&env, SubscriptionError::InvalidInterval);
        }
        let mut plan = load_plan(&env, plan_id);
        if plan.merchant != merchant {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        plan.interval = new_interval;
        env.storage()
            .persistent()
            .set(&DataKey::Plan(plan_id), &plan);
    }

    pub fn deactivate_plan(env: Env, merchant: Address, plan_id: u64) {
        merchant.require_auth();
        let mut plan = load_plan(&env, plan_id);
        if plan.merchant != merchant {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        plan.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Plan(plan_id), &plan);
    }

    // ── Subscriptions ─────────────────────────────────────────────────────────

    /// Subscribe a customer to a plan.  Returns the new subscription ID.
    pub fn subscribe(env: Env, customer: Address, plan_id: u64) -> u64 {
        customer.require_auth();

        let plan = load_plan(&env, plan_id);
        if !plan.active {
            panic_with_error!(&env, SubscriptionError::PlanNotActive);
        }

        let sub_id = get_subscription_count(&env) + 1;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionCount, &sub_id);

        let sub = Subscription {
            id: sub_id,
            plan_id,
            customer,
            status: SubscriptionStatus::Active,
            created_at: env.ledger().timestamp(),
            last_charged: 0,
            past_due_since: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(sub_id), &sub);
        sub_id
    }

    pub fn get_subscription(env: Env, sub_id: u64) -> Subscription {
        load_subscription(&env, sub_id)
    }

    pub fn cancel_subscription(env: Env, caller: Address, sub_id: u64) {
        caller.require_auth();
        let mut sub = load_subscription(&env, sub_id);
        if sub.status != SubscriptionStatus::Active {
            panic_with_error!(&env, SubscriptionError::SubscriptionNotActive);
        }
        let plan = load_plan(&env, sub.plan_id);
        if sub.customer != caller && plan.merchant != caller {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        sub.status = SubscriptionStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(sub_id), &sub);
    }

    // ── Billing ───────────────────────────────────────────────────────────────

    /// Authorise the contract as a spender so it can pull recurring charges.
    /// The customer must call this before the first charge (and top-up as needed).
    pub fn authorize_billing(env: Env, customer: Address, sub_id: u64, cycles: u32) {
        customer.require_auth();
        let sub = load_subscription(&env, sub_id);
        if sub.customer != customer {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        if sub.status != SubscriptionStatus::Active {
            panic_with_error!(&env, SubscriptionError::SubscriptionNotActive);
        }
        let plan = load_plan(&env, sub.plan_id);
        let allowance_amount = plan.amount.saturating_mul(i128::from(cycles));

        let ledger_expiry = env.ledger().sequence() + 17_280 * u32::from(cycles);
        let token_client = token::TokenClient::new(&env, &plan.token);
        let spender = env.current_contract_address();
        token_client.approve(&customer, &spender, &allowance_amount, &ledger_expiry);
    }

    /// Return the current token allowance the contract holds from a customer.
    /// Callers can use this to verify a sufficient allowance exists before
    /// attempting a charge.
    pub fn get_billing_allowance(env: Env, customer: Address, sub_id: u64) -> i128 {
        let sub = load_subscription(&env, sub_id);
        let plan = load_plan(&env, sub.plan_id);
        let token_client = token::TokenClient::new(&env, &plan.token);
        let spender = env.current_contract_address();
        token_client.allowance(&customer, &spender)
    }

    /// Revoke the contract's spending allowance for a customer on a given
    /// subscription.  This effectively prevents future automatic charges
    /// without cancelling the subscription record.
    pub fn revoke_billing_authorization(env: Env, customer: Address, sub_id: u64) {
        customer.require_auth();
        let sub = load_subscription(&env, sub_id);
        if sub.customer != customer {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        let plan = load_plan(&env, sub.plan_id);
        let token_client = token::TokenClient::new(&env, &plan.token);
        let spender = env.current_contract_address();
        let current_seq = env.ledger().sequence();
        token_client.approve(&customer, &spender, &0_i128, &current_seq);
    }

    /// Charge the next billing cycle for a subscription.
    pub fn charge(env: Env, sub_id: u64) {
        let mut sub = load_subscription(&env, sub_id);
        if sub.status != SubscriptionStatus::Active {
            panic_with_error!(&env, SubscriptionError::SubscriptionNotActive);
        }
        let plan = load_plan(&env, sub.plan_id);
        let now = env.ledger().timestamp();
        if sub.last_charged > 0 && now < sub.last_charged.saturating_add(plan.interval) {
            panic_with_error!(&env, SubscriptionError::ChargeTooEarly);
        }

        let token_client = token::TokenClient::new(&env, &plan.token);
        let spender = env.current_contract_address();

        let allowance = token_client.allowance(&sub.customer, &spender);
        if allowance < plan.amount {
            panic_with_error!(&env, SubscriptionError::InsufficientAllowance);
        }

        token_client.transfer_from(&spender, &sub.customer, &plan.merchant, &plan.amount);

        sub.last_charged = now;
        sub.past_due_since = 0;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(sub_id), &sub);
    }

    /// Forgiving variant of [`charge`]. Drives the subscription's billing
    /// state machine — charging, transitioning to `PastDue` on missed
    /// payment, recovering when allowance returns, or terminating when the
    /// grace window expires. Returns a [`ChargeOutcome`] describing what
    /// happened so an off-chain billing bot can react without re-reading
    /// state.
    ///
    /// Unlike `charge`, this never panics on insufficient allowance — that
    /// failure mode is what the grace-period mechanism is for.
    pub fn process_charge(env: Env, sub_id: u64) -> ChargeOutcome {
        let mut sub = load_subscription(&env, sub_id);
        let plan = load_plan(&env, sub.plan_id);
        let now = env.ledger().timestamp();

        match sub.status {
            SubscriptionStatus::Active => {
                if sub.last_charged > 0 && now < sub.last_charged.saturating_add(plan.interval) {
                    return ChargeOutcome::NotDueYet;
                }
                if try_pull_charge(&env, &sub, &plan) {
                    sub.last_charged = now;
                    sub.past_due_since = 0;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Subscription(sub_id), &sub);
                    ChargeOutcome::Charged
                } else if plan.grace_period == 0 {
                    sub.status = SubscriptionStatus::Terminated;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Subscription(sub_id), &sub);
                    ChargeOutcome::Terminated
                } else {
                    sub.status = SubscriptionStatus::PastDue;
                    sub.past_due_since = now;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Subscription(sub_id), &sub);
                    ChargeOutcome::EnteredGrace
                }
            }
            SubscriptionStatus::PastDue => {
                if try_pull_charge(&env, &sub, &plan) {
                    sub.status = SubscriptionStatus::Active;
                    sub.last_charged = now;
                    sub.past_due_since = 0;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Subscription(sub_id), &sub);
                    ChargeOutcome::Recovered
                } else if now > sub.past_due_since.saturating_add(plan.grace_period) {
                    sub.status = SubscriptionStatus::Terminated;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Subscription(sub_id), &sub);
                    ChargeOutcome::Terminated
                } else {
                    ChargeOutcome::EnteredGrace
                }
            }
            SubscriptionStatus::Cancelled | SubscriptionStatus::Terminated => {
                panic_with_error!(&env, SubscriptionError::SubscriptionNotActive)
            }
        }
    }

    /// Manually terminate a `PastDue` subscription whose grace period has
    /// fully elapsed. Idempotent for already-terminated subscriptions.
    /// Anyone may call this — there's no value in restricting it.
    pub fn enforce_grace(env: Env, sub_id: u64) {
        let mut sub = load_subscription(&env, sub_id);
        if sub.status == SubscriptionStatus::Terminated {
            return;
        }
        if sub.status != SubscriptionStatus::PastDue {
            panic_with_error!(&env, SubscriptionError::SubscriptionNotActive);
        }
        let plan = load_plan(&env, sub.plan_id);
        let now = env.ledger().timestamp();
        if now <= sub.past_due_since.saturating_add(plan.grace_period) {
            panic_with_error!(&env, SubscriptionError::GraceNotExpired);
        }
        sub.status = SubscriptionStatus::Terminated;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(sub_id), &sub);
    }

    /// Cancel a subscription and refund the unused portion of the current
    /// billing cycle to the customer. The merchant must have approved the
    /// contract to spend at least `refund_amount` from their balance —
    /// without that allowance the inner `transfer_from` panics and nothing
    /// is changed.
    ///
    /// Refund math: `refund = amount * remaining_seconds / interval`,
    /// where `remaining_seconds = (last_charged + interval) - now` clamped
    /// to `[0, interval]`. If the subscription was never charged or the
    /// cycle has fully elapsed, no refund is issued and the call panics
    /// with `NothingToRefund`.
    ///
    /// Either the customer or the merchant may initiate; both must
    /// authorize so the merchant cannot drain themselves and the customer
    /// cannot pull funds without merchant consent.
    pub fn cancel_with_prorated_refund(env: Env, caller: Address, sub_id: u64) {
        caller.require_auth();
        let mut sub = load_subscription(&env, sub_id);
        if sub.status != SubscriptionStatus::Active && sub.status != SubscriptionStatus::PastDue {
            panic_with_error!(&env, SubscriptionError::SubscriptionNotActive);
        }
        let plan = load_plan(&env, sub.plan_id);
        let is_customer = sub.customer == caller;
        let is_merchant = plan.merchant == caller;
        if !is_customer && !is_merchant {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        // Whichever party did not initiate must still authorize so refunds
        // require mutual consent.
        if is_customer {
            plan.merchant.require_auth();
        } else {
            sub.customer.require_auth();
        }

        let refund_amount = prorated_refund(&sub, &plan, env.ledger().timestamp());
        if refund_amount <= 0 {
            panic_with_error!(&env, SubscriptionError::NothingToRefund);
        }

        // Pull the refund from the merchant's balance into the customer's.
        // The merchant's earlier `approve(contract, ...)` is what makes this
        // possible — without it the transfer fails and the subscription is
        // left untouched.
        let token_client = token::TokenClient::new(&env, &plan.token);
        let spender = env.current_contract_address();
        token_client.transfer_from(&spender, &plan.merchant, &sub.customer, &refund_amount);

        sub.status = SubscriptionStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(sub_id), &sub);
    }

    /// Read-only helper: how much would be refunded if the subscription
    /// were cancelled right now. Returns 0 when nothing would be refunded.
    /// Useful for off-chain previews before asking the merchant to approve.
    pub fn quote_prorated_refund(env: Env, sub_id: u64) -> i128 {
        let sub = load_subscription(&env, sub_id);
        let plan = load_plan(&env, sub.plan_id);
        prorated_refund(&sub, &plan, env.ledger().timestamp())
    }
}

/// Attempts a single token pull from customer to merchant for the plan's
/// amount. Returns `false` if the allowance is insufficient — callers
/// translate that into a state transition (PastDue / Terminated).
fn try_pull_charge(env: &Env, sub: &Subscription, plan: &Plan) -> bool {
    let token_client = token::TokenClient::new(env, &plan.token);
    let spender = env.current_contract_address();
    let allowance = token_client.allowance(&sub.customer, &spender);
    if allowance < plan.amount {
        return false;
    }
    token_client.transfer_from(&spender, &sub.customer, &plan.merchant, &plan.amount);
    true
}

/// Compute the prorated refund owed to the customer at `now` for the
/// remaining unused time in the current billing cycle.
///
/// Returns 0 when:
/// - `last_charged == 0` (never charged — no funds to refund)
/// - `now >= last_charged + interval` (cycle fully consumed)
fn prorated_refund(sub: &Subscription, plan: &Plan, now: u64) -> i128 {
    if sub.last_charged == 0 {
        return 0;
    }
    let cycle_end = sub.last_charged.saturating_add(plan.interval);
    if now >= cycle_end {
        return 0;
    }
    let remaining = cycle_end - now;
    let interval = plan.interval as i128;
    if interval == 0 {
        return 0;
    }
    // amount * remaining / interval; checked_mul guards against overflow on
    // pathological amounts before we floor-divide.
    let scaled = match plan.amount.checked_mul(remaining as i128) {
        Some(v) => v,
        None => return 0,
    };
    scaled / interval
}
