#![no_std]

mod errors;
mod types;

use errors::SubscriptionError;
use soroban_sdk::{
    contract, contractimpl, panic_with_error, token, Address, Env, String, Vec,
};
use types::{DataKey, Plan, Subscription, SubscriptionStatus};

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
    env.storage().persistent().get(&DataKey::PlanCount).unwrap_or(0)
}

fn get_subscription_count(env: &Env) -> u64 {
    env.storage().persistent().get(&DataKey::SubscriptionCount).unwrap_or(0)
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
            env.storage().persistent().set(&DataKey::AcceptedTokens, &tokens);
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
        env.storage().persistent().set(&DataKey::PlanCount, &plan_id);

        let plan = Plan {
            id: plan_id,
            merchant,
            description,
            token,
            amount,
            interval,
            active: true,
            created_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::Plan(plan_id), &plan);
        plan_id
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
        env.storage().persistent().set(&DataKey::Plan(plan_id), &plan);
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
        env.storage().persistent().set(&DataKey::Plan(plan_id), &plan);
    }

    pub fn deactivate_plan(env: Env, merchant: Address, plan_id: u64) {
        merchant.require_auth();
        let mut plan = load_plan(&env, plan_id);
        if plan.merchant != merchant {
            panic_with_error!(&env, SubscriptionError::NotAuthorized);
        }
        plan.active = false;
        env.storage().persistent().set(&DataKey::Plan(plan_id), &plan);
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
        env.storage().persistent().set(&DataKey::SubscriptionCount, &sub_id);

        let sub = Subscription {
            id: sub_id,
            plan_id,
            customer,
            status: SubscriptionStatus::Active,
            created_at: env.ledger().timestamp(),
            last_charged: 0,
        };
        env.storage().persistent().set(&DataKey::Subscription(sub_id), &sub);
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
        env.storage().persistent().set(&DataKey::Subscription(sub_id), &sub);
    }

    // ── Billing ───────────────────────────────────────────────────────────────

    /// Authorise the contract as a spender so it can pull recurring charges.
    /// The customer must call this before the first charge (and top-up as needed).
    pub fn authorize_billing(
        env: Env,
        customer: Address,
        sub_id: u64,
        cycles: u32,
    ) {
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
        env.storage().persistent().set(&DataKey::Subscription(sub_id), &sub);
    }
}
