#![no_std]

mod errors;

#[cfg(test)]
mod test;

use crate::errors::EscrowError;
use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, panic_with_error, token, Address, Env,
    String, Vec,
};

const MAX_FEE_BPS: u32 = 10_000;

// ── Storage keys ───────────────────────────────────────────────────────────────

#[derive(Clone)]
#[contracttype]
pub struct EscrowConfig {
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
    pub terms: String,
    pub token: Address,
    pub amount: i128,
    pub expiry: u64,
}

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Buyer,
    Seller,
    Arbiter,
    Terms,
    Token,
    TotalAmount,
    EscrowStatus,
    Deadline,
    PlatformAccount,
    FeePercentageBps,
    TotalReleased,
    Milestones,
}

// ── Types ──────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub id: u32,
    pub description: String,
    pub percentage_bps: u32,
    pub released: bool,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowStatus {
    Pending = 0,
    Completed = 1,
    Disputed = 2,
    Resolved = 3,
    PartiallyReleased = 4,
    Expired = 5,
}

// ── Events ─────────────────────────────────────────────────────────────────────

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

// Emitted on every fee deduction — satisfies #240 (Shade fee engine integration).
// Off-chain indexers (and the Shade fee engine) subscribe to this event to keep
// their fee ledgers consistent with the escrow contract.
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
        .get(&DataKey::EscrowStatus)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::NotInitialized))
}

fn _set_status(env: &Env, status: EscrowStatus) {
    env.storage()
        .instance()
        .set(&DataKey::EscrowStatus, &status);
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

fn _get_seller(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Seller)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::NotInitialized))
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

fn _get_total_released(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalReleased)
        .unwrap_or(0)
}

fn _get_remaining_balance(env: &Env) -> i128 {
    _get_total_amount(env) - _get_total_released(env)
}

fn _get_milestones(env: &Env) -> Vec<Milestone> {
    env.storage()
        .instance()
        .get(&DataKey::Milestones)
        .unwrap_or_else(|| Vec::new(env))
}

fn _set_milestones(env: &Env, milestones: Vec<Milestone>) {
    env.storage()
        .instance()
        .set(&DataKey::Milestones, &milestones);
}

pub fn _calculate_fee(amount: i128, fee_bps: u32) -> i128 {
    if fee_bps == 0 {
        return 0;
    }
    let fee = (amount * fee_bps as i128) / 10_000;
    fee.min(amount)
}

fn _calculate_milestone_amount(total: i128, percentage_bps: u32) -> i128 {
    (total * percentage_bps as i128) / 10_000
}

// Mark a specific milestone as released by rebuilding the Vec. Soroban's Vec
// is copy-on-write; there is no in-place mutation via get_mut.
fn _mark_milestone_released(env: &Env, milestone_id: u32) {
    let milestones = _get_milestones(env);
    let mut updated: Vec<Milestone> = Vec::new(env);
    for m in milestones.iter() {
        if m.id == milestone_id {
            updated.push_back(Milestone {
                released: true,
                ..m
            });
        } else {
            updated.push_back(m);
        }
    }
    _set_milestones(env, updated);
}

// ── Contract ───────────────────────────────────────────────────────────────────

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Initialise the escrow. Milestones must sum to exactly 10_000 bps when
    /// more than one is provided; a single milestone is treated as 100%. (#237)
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
            for m in milestones.iter() {
                total_bps += m.percentage_bps;
            }
            if total_bps != 10_000 {
                panic_with_error!(env, EscrowError::MilestoneSumMismatch);
            }
        }

        if expires_at <= env.ledger().timestamp() {
            panic!("expires_at must be in the future");
        }

        if amount <= 0 {
            panic!("amount must be positive");
        }
        env.storage().instance().set(&DataKey::Buyer, &buyer);
        env.storage().instance().set(&DataKey::Seller, &seller);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        env.storage().instance().set(&DataKey::Terms, &terms);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage()
            .instance()
            .set(&DataKey::TotalAmount, &total_amount);
        env.storage()
            .instance()
            .set(&DataKey::EscrowStatus, &EscrowStatus::Pending);
        env.storage()
            .instance()
            .set(&DataKey::TotalReleased, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::FeePercentageBps, &fee_percentage_bps);
        _set_milestones(&env, milestones);

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

    /// Set (or replace) the platform account that receives fees. Only callable
    /// by the buyer or arbiter while the escrow is still Pending. (#240)
    pub fn set_platform_account(env: Env, caller: Address, platform_account: Address) {
        caller.require_auth();

        if _get_status(&env) != EscrowStatus::Pending {
            panic_with_error!(env, EscrowError::InvalidStatus);
        }

        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        if caller != buyer && caller != arbiter {
            panic_with_error!(env, EscrowError::NotAuthorized);
        }

        env.storage()
            .instance()
            .set(&DataKey::PlatformAccount, &platform_account);
    }

    pub fn get_platform_account(env: Env) -> Address {
        _get_platform_account(&env)
    }

    /// Add a milestone while the escrow is still Pending with no funds released.
    /// Any party (buyer / seller / arbiter) may call this. (#237)
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
        _get_total_released(&env)
    }

    /// Release a single milestone to the seller with fee routing. (#237 + #240)
    pub fn approve_milestone_release(env: Env, milestone_id: u32) {
        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        buyer.require_auth();

        let status = _get_status(&env);
        if status != EscrowStatus::Pending && status != EscrowStatus::PartiallyReleased {
            panic_with_error!(env, EscrowError::InvalidStatus);
        }

        let milestones = _get_milestones(&env);
        let milestone = milestones
            .iter()
            .find(|m| m.id == milestone_id)
            .unwrap_or_else(|| panic_with_error!(env, EscrowError::MilestoneNotFound));

        if milestone.released {
            panic_with_error!(env, EscrowError::MilestoneAlreadyReleased);
        }

        let total_amount = _get_total_amount(&env);
        let token = _get_token(&env);
        let platform_account = _get_platform_account(&env);
        let fee_bps = _get_fee_percentage(&env);
        let seller = _get_seller(&env);

        let milestone_amount = _calculate_milestone_amount(total_amount, milestone.percentage_bps);
        let fee_amount = _calculate_fee(milestone_amount, fee_bps);
        let net_amount = milestone_amount - fee_amount;

        if milestone_amount > _get_remaining_balance(&env) {
            panic_with_error!(env, EscrowError::InsufficientBalance);
        }

        let token_client = token::TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &seller, &net_amount);
        if fee_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &platform_account, &fee_amount);
            FeeDeductedEvent {
                fee_amount,
                token: token.clone(),
                platform_account: platform_account.clone(),
                timestamp: env.ledger().timestamp(),
            }
            .publish(&env);
        }

        let new_total_released = _get_total_released(&env) + milestone_amount;
        env.storage()
            .instance()
            .set(&DataKey::TotalReleased, &new_total_released);

        _mark_milestone_released(&env, milestone_id);

        let new_status = if new_total_released >= total_amount {
            EscrowStatus::Completed
        } else {
            EscrowStatus::PartiallyReleased
        };
        _set_status(&env, new_status);

        MilestoneReleasedEvent {
            milestone_id,
            released_by: buyer,
            amount: milestone_amount,
            fee: fee_amount,
            net_amount,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    /// Release the entire escrow balance at once (no milestones path). (#240)
    pub fn approve_release(env: Env) {
        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        buyer.require_auth();

        if _get_status(&env) != EscrowStatus::Pending {
            panic_with_error!(env, EscrowError::InvalidStatus);
        }

        let token = _get_token(&env);
        let total_amount = _get_total_amount(&env);
        let fee_bps = _get_fee_percentage(&env);
        let platform_account = _get_platform_account(&env);
        let seller = _get_seller(&env);

        let fee_amount = _calculate_fee(total_amount, fee_bps);
        let net_amount = total_amount - fee_amount;

        let token_client = token::TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &seller, &net_amount);
        if fee_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &platform_account, &fee_amount);
            FeeDeductedEvent {
                fee_amount,
                token: token.clone(),
                platform_account: platform_account.clone(),
                timestamp: env.ledger().timestamp(),
            }
            .publish(&env);
        }

        env.storage()
            .instance()
            .set(&DataKey::TotalReleased, &total_amount);
        _set_status(&env, EscrowStatus::Completed);

        EscrowReleaseApprovedEvent {
            buyer,
            seller,
            token,
            amount: total_amount,
            fee: fee_amount,
            net_amount,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn open_dispute(env: Env) {
        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        buyer.require_auth();

        let current_status = _get_status(&env);
        if current_status != EscrowStatus::Pending
            && current_status != EscrowStatus::PartiallyReleased
        {
            panic_with_error!(env, EscrowError::InvalidStatus);
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
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();

        if _get_status(&env) != EscrowStatus::Disputed {
            panic_with_error!(env, EscrowError::InvalidStatus);
        }

        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        let seller = _get_seller(&env);
        let token = _get_token(&env);
        let amount = _get_remaining_balance(&env);
        let recipient = if released_to_buyer {
            buyer.clone()
        } else {
            seller
        };

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

    // ── Getters ──────────────────────────────────────────────────────────────────

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
}
