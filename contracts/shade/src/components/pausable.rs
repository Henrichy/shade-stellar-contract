use crate::components::core;
use crate::errors::ContractError;
use crate::events;
use crate::types::DataKey;
use soroban_sdk::{panic_with_error, Address, Env};

// TODO: create a more complex pausable functionality that comprises of pausing and unpausing particularly
// functionality of the contract like specifically pausing subscription, merchant, plan, payments and withdrawals etc.
// This way, when a particular functionality is paused, other functionalities of the contract will not be affected.
// unlike the current implementation where the entire contract is paused.

pub fn pause(env: &Env, admin: &Address) {
    admin.require_auth();

    if core::get_admin(env) != admin.clone() {
        panic_with_error!(env, ContractError::NotAuthorized);
    }

    assert_not_paused(env);

    env.storage().persistent().set(&DataKey::Paused, &true);

    events::publish_contract_paused_event(env, admin.clone(), env.ledger().timestamp());
}

pub fn unpause(env: &Env, admin: &Address) {
    admin.require_auth();

    if core::get_admin(env) != admin.clone() {
        panic_with_error!(env, ContractError::NotAuthorized);
    }

    assert_paused(env);

    env.storage().persistent().set(&DataKey::Paused, &false);

    events::publish_contract_unpaused_event(env, admin.clone(), env.ledger().timestamp());
}

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

pub fn assert_paused(env: &Env) {
    if !is_paused(env) {
        panic_with_error!(env, ContractError::ContractNotPaused);
    }
}

pub fn assert_not_paused(env: &Env) {
    if is_paused(env) {
        panic_with_error!(env, ContractError::ContractPaused);
    }
}
