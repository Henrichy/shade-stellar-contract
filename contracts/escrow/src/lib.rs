#![no_std]

mod errors;

use crate::errors::EscrowError;
use soroban_sdk::{contract, contractevent, contractimpl, contracttype, panic_with_error, token, Address, Env, String};

const MAX_FEE_BPS: u32 = 10_000; // 100%

#[contract]
pub struct EscrowContract;

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Buyer,
    Seller,
    Arbiter,
    Terms,
    Token,
    TotalAmount,
    Status,
    PlatformAccount,
    FeePercentageBps,
    TotalReleased,
    Milestones,
    ReleasedMilestones,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub id: u32,
    pub description: String,
    pub percentage_bps: u32,
    pub released: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    Pending = 0,
    Completed = 1,
    Disputed = 2,
    Resolved = 3,
    PartiallyReleased = 4,
}

#[contractevent]
pub struct EscrowInitializedEvent {
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
    pub token: Address,
    pub total_amount: i128,
    pub fee_percentage_bps: u32,
    pub timestamp: u64,
}

#[contractevent]
pub struct MilestoneAddedEvent {
    pub milestone_id: u32,
    pub description: String,
    pub percentage_bps: u32,
    pub timestamp: u64,
}

#[contractevent]
pub struct MilestoneReleasedEvent {
    pub milestone_id: u32,
    pub released_by: Address,
    pub amount: i128,
    pub fee: i128,
    pub net_amount: i128,
    pub timestamp: u64,
}

#[contractevent]
pub struct EscrowReleaseApprovedEvent {
    pub buyer: Address,
    pub seller: Address,
    pub token: Address,
    pub amount: i128,
    pub fee: i128,
    pub net_amount: i128,
    pub timestamp: u64,
}

#[contractevent]
pub struct EscrowDisputeOpenedEvent {
    pub buyer: Address,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contractevent]
pub struct EscrowDisputeResolvedEvent {
    pub arbiter: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub released_to_buyer: bool,
    pub timestamp: u64,
}

#[contractevent]
pub struct FeeDeductedEvent {
    pub fee_amount: i128,
    pub token: Address,
    pub platform_account: Address,
    pub timestamp: u64,
}

// ── Private helpers ────────────────────────────────────────────────────────────

fn _get_status(env: &Env) -> EscrowStatus {
    env.storage()
        .instance()
        .get(&DataKey::Status)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::NotInitialized))
}

fn _set_status(env: &Env, status: EscrowStatus) {
    env.storage().instance().set(&DataKey::Status, &status);
}

fn _get_total_amount(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalAmount)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::NotInitialized))
}

fn _get_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Token)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::NotInitialized))
}

fn _calculate_fee(amount: i128, fee_bps: u32) -> i128 {
    if fee_bps == 0 {
        return 0;
    }
    let fee = (amount * fee_bps as i128) / 10_000;
    if fee > amount {
        return amount;
    }
    fee
}

fn _get_remaining_balance(env: &Env) -> i128 {
    let total = _get_total_amount(&env);
    let released: i128 = env.storage()
        .instance()
        .get(&DataKey::TotalReleased)
        .unwrap_or(0);
    total - released
}

fn _get_platform_account(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::PlatformAccount)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::PlatformAccountNotSet))
}

fn _get_fee_percentage(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::FeePercentageBps)
        .unwrap_or(0)
}

fn _get_milestones(env: &Env) -> Vec<Milestone> {
    env.storage()
        .instance()
        .get(&DataKey::Milestones)
        .unwrap_or_else(|| Vec::new(&env))
}

fn _set_milestones(env: &Env, milestones: Vec<Milestone>) {
    env.storage().instance().set(&DataKey::Milestones, &milestones);
}

fn _get_released_milestones(env: &Env) -> Vec<bool> {
    env.storage()
        .instance()
        .get(&DataKey::ReleasedMilestones)
        .unwrap_or_else(|| Vec::new(&env))
}

fn _set_released_milestones(env: &Env, released: Vec<bool>) {
    env.storage().instance().set(&DataKey::ReleasedMilestones, &released);
}

fn _mark_milestone_released(env: &Env, milestone_id: u32) {
    let mut released = _get_released_milestones(&env);
    let milestone_index = milestone_id as usize;
    while released.len() <= milestone_index {
        released.push_back(false);
    }
    *released.get_mut(milestone_index).unwrap() = true;
    _set_released_milestones(&env, released);
}

fn _calculate_milestone_amount(total: i128, percentage_bps: u32) -> i128 {
    (total * percentage_bps as i128) / 10_000
}

fn _get_seller(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Seller)
        .unwrap_or_else(|| panic!("seller not set"))
}

#[contractimpl]
impl EscrowContract {
    pub fn init(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        terms: String,
        token: Address,
        total_amount: i128,
        fee_percentage_bps: u32,
        milestones: Vec<Milestone>,
    ) {
        if env.storage().instance().has(&DataKey::Buyer) {
            panic_with_error!(env, EscrowError::AlreadyInitialized);
        }

        if total_amount <= 0 {
            panic_with_error!(env, EscrowError::InvalidAmount);
        }

        if fee_percentage_bps > MAX_FEE_BPS {
            panic_with_error!(env, EscrowError::InvalidFeePercentage);
        }

        if milestones.len() > 1 {
            let mut total_bps: u32 = 0;
            for milestone in milestones.iter() {
                total_bps += milestone.percentage_bps;
            }
            if total_bps != 10_000 {
                panic_with_error!(env, EscrowError::MilestoneSumMismatch);
            }
        }

        env.storage().instance().set(&DataKey::Buyer, &buyer);
        env.storage().instance().set(&DataKey::Seller, &seller);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        env.storage().instance().set(&DataKey::Terms, &terms);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::TotalAmount, &total_amount);
        env.storage().instance().set(&DataKey::Status, &EscrowStatus::Pending);
        env.storage().instance().set(&DataKey::TotalReleased, &0i128);
        env.storage().instance().set(&DataKey::FeePercentageBps, &fee_percentage_bps);

        _set_milestones(&env, milestones);
        _set_released_milestones(&env, Vec::new(&env));

        EscrowInitializedEvent {
            buyer,
            seller,
            arbiter,
            token,
            total_amount,
            fee_percentage_bps,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn set_platform_account(env: Env, caller: Address, platform_account: Address) {
        caller.require_auth();

        if _get_status(&env) != EscrowStatus::Pending {
            panic!("cannot set platform account after escrow is active");
        }

        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        if caller != buyer && caller != arbiter {
            panic_with_error!(env, EscrowError::NotAuthorized);
        }

        env.storage().instance().set(&DataKey::PlatformAccount, &platform_account);
    }

    pub fn get_platform_account(env: Env) -> Address {
        _get_platform_account(&env)
    }

    pub fn add_milestone(env: Env, caller: Address, milestone: Milestone) {
        caller.require_auth();

        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        let seller: Address = env.storage().instance().get(&DataKey::Seller).unwrap();
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();

        if caller != buyer && caller != seller && caller != arbiter {
            panic_with_error!(env, EscrowError::NotAuthorized);
        }

        if _get_status(&env) != EscrowStatus::Pending {
            panic_with_error!(env, EscrowError::CannotAddMilestone);
        }

        if _get_total_released(&env) > 0 {
            panic_with_error!(env, EscrowError::CannotAddMilestone);
        }

        let mut milestones = _get_milestones(&env);
        milestones.push_back(milestone.clone());
        _set_milestones(&env, milestones);

        MilestoneAddedEvent {
            milestone_id: milestone.id,
            description: milestone.description,
            percentage_bps: milestone.percentage_bps,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn get_milestones(env: Env) -> Vec<Milestone> {
        _get_milestones(&env)
    }

    pub fn get_total_released(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalReleased)
            .unwrap_or(0)
    }

    pub fn approve_milestone_release(env: Env, milestone_id: u32) {
        let buyer = env.storage().instance().get(&DataKey::Buyer).unwrap();
        buyer.require_auth();

        let status = _get_status(&env);
        if status != EscrowStatus::Pending && status != EscrowStatus::PartiallyReleased {
            panic!("escrow must be pending or partially released");
        }

        let milestones = _get_milestones(&env);
        if milestone_id as usize >= milestones.len() {
            panic_with_error!(env, EscrowError::MilestoneNotFound);
        }

        let milestone = milestones.get(milestone_id as usize).unwrap();
        if milestone.released {
            panic_with_error!(env, EscrowError::MilestoneAlreadyReleased);
        }

        let total_amount = _get_total_amount(&env);
        let token = _get_token(&env);
        let platform_account = _get_platform_account(&env);
        let fee_bps = _get_fee_percentage(&env);

        let milestone_amount = _calculate_milestone_amount(total_amount, milestone.percentage_bps);
        let fee_amount = _calculate_fee(milestone_amount, fee_bps);
        let net_amount = milestone_amount - fee_amount;

        if milestone_amount > _get_remaining_balance(&env) {
            panic_with_error!(env, EscrowError::InsufficientBalance);
        }

        if net_amount < 0 {
            panic!("negative net amount after fee");
        }

        token::TokenClient::new(&env, &token)
            .transfer(&env.current_contract_address(), &_get_seller(&env), &net_amount);

        if fee_amount > 0 {
            token::TokenClient::new(&env, &token)
                .transfer(&env.current_contract_address(), &platform_account, &fee_amount);
        }

        let mut total_released: i128 = env.storage().instance().get(&DataKey::TotalReleased).unwrap_or(0);
        total_released += milestone_amount;
        env.storage().instance().set(&DataKey::TotalReleased, &total_released);

        let mut milestones = _get_milestones(&env);
        let milestone_mut = milestones.get_mut(milestone_id as usize).unwrap();
        milestone_mut.released = true;
        _set_milestones(&env, milestones);
        _mark_milestone_released(&env, milestone_id);

        if total_released == total_amount {
            _set_status(&env, EscrowStatus::Completed);
        } else {
            _set_status(&env, EscrowStatus::PartiallyReleased);
        }

        MilestoneReleasedEvent {
            milestone_id,
            released_by: buyer,
            amount: milestone_amount,
            fee: fee_amount,
            net_amount,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);

        FeeDeductedEvent {
            fee_amount,
            token,
            platform_account,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn approve_release(env: Env) {
        let buyer = env.storage().instance().get(&DataKey::Buyer).unwrap();
        buyer.require_auth();

        if _get_status(&env) != EscrowStatus::Pending {
            panic_with_error!(env, EscrowError::InvalidStatus);
        }

        let token = _get_token(&env);
        let total_amount = _get_total_amount(&env);
        let fee_bps = _get_fee_percentage(&env);
        let platform_account = _get_platform_account(&env);

        let fee_amount = _calculate_fee(total_amount, fee_bps);
        let net_amount = total_amount - fee_amount;

        if net_amount < 0 {
            panic!("fee exceeds amount");
        }

        token::TokenClient::new(&env, &token)
            .transfer(&env.current_contract_address(), &_get_seller(&env), &net_amount);

        if fee_amount > 0 {
            token::TokenClient::new(&env, &token)
                .transfer(&env.current_contract_address(), &platform_account, &fee_amount);
        }

        env.storage().instance().set(&DataKey::TotalReleased, &total_amount);
        _set_status(&env, EscrowStatus::Completed);

        EscrowReleaseApprovedEvent {
            buyer,
            seller: _get_seller(&env),
            token,
            amount: total_amount,
            fee: fee_amount,
            net_amount,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);

        FeeDeductedEvent {
            fee_amount,
            token,
            platform_account,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn open_dispute(env: Env) {
        let buyer = env.storage().instance().get(&DataKey::Buyer).unwrap();
        buyer.require_auth();

        let current_status = _get_status(&env);
        if current_status != EscrowStatus::Pending && current_status != EscrowStatus::PartiallyReleased {
            panic!("escrow cannot be disputed in current status");
        }

        let token = _get_token(&env);
        let amount = _get_total_amount(&env);

        _set_status(&env, EscrowStatus::Disputed);

        EscrowDisputeOpenedEvent {
            buyer,
            token,
            amount,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn resolve_dispute(env: Env, released_to_buyer: bool) {
        let arbiter = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();

        if _get_status(&env) != EscrowStatus::Disputed {
            panic!("escrow dispute is not open");
        }

        let buyer = env.storage().instance().get(&DataKey::Buyer).unwrap();
        let seller = _get_seller(&env);
        let token = _get_token(&env);
        let amount = _get_total_amount(&env);
        let recipient = if released_to_buyer { buyer } else { seller };

        token::TokenClient::new(&env, &token)
            .transfer(&env.current_contract_address(), &recipient, &amount);

        _set_status(&env, EscrowStatus::Resolved);

        EscrowDisputeResolvedEvent {
            arbiter,
            recipient,
            token,
            amount,
            released_to_buyer,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    // ── Getters ─────────────────────────────────────────────────────────────────

    pub fn buyer(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Buyer).unwrap()
    }

    pub fn seller(env: Env) -> Address {
        _get_seller(&env)
    }

    pub fn arbiter(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Arbiter).unwrap()
    }

    pub fn terms(env: Env) -> String {
        env.storage().instance().get(&DataKey::Terms).unwrap()
    }

    pub fn token(env: Env) -> Address {
        _get_token(&env)
    }

    pub fn total_amount(env: Env) -> i128 {
        _get_total_amount(&env)
    }

    pub fn status(env: Env) -> EscrowStatus {
        _get_status(&env)
    }

    pub fn fee_percentage_bps(env: Env) -> u32 {
        _get_fee_percentage(&env)
    }

    pub fn platform_account(env: Env) -> Address {
        _get_platform_account(&env)
    }

    pub fn get_milestones(env: Env) -> Vec<Milestone> {
        _get_milestones(&env)
    }
}
