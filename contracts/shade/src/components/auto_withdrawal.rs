use crate::components::merchant;
use crate::errors::ContractError;
use crate::events;
use crate::types::{AutoWithdrawalThreshold, DataKey};
use soroban_sdk::{panic_with_error, token, Address, Env};

/// Set auto-withdrawal threshold for a merchant and token
pub fn set_auto_withdrawal_threshold(
    env: &Env,
    merchant_address: &Address,
    token: &Address,
    threshold: i128,
) {
    merchant_address.require_auth();

    if threshold < 0 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    let merchant_id = merchant::get_merchant_id(env, merchant_address);
    let merchant = merchant::get_merchant(env, merchant_id);

    if !merchant.active {
        panic_with_error!(env, ContractError::MerchantNotActive);
    }

    let threshold_data = AutoWithdrawalThreshold {
        merchant_id,
        token: token.clone(),
        threshold,
    };

    env.storage().persistent().set(
        &DataKey::MerchantAutoWithdrawalThreshold(merchant_id, token.clone()),
        &threshold_data,
    );

    events::publish_auto_withdrawal_threshold_set_event(env, merchant_id, token.clone(), threshold);
}

/// Get auto-withdrawal threshold for a merchant and token
pub fn get_auto_withdrawal_threshold(
    env: &Env,
    merchant_id: u64,
    token: &Address,
) -> Option<i128> {
    env.storage()
        .persistent()
        .get(&DataKey::MerchantAutoWithdrawalThreshold(merchant_id, token.clone()))
        .map(|threshold_data: AutoWithdrawalThreshold| threshold_data.threshold)
}

/// Set auto-withdrawal recipient address for a merchant
pub fn set_auto_withdrawal_recipient(
    env: &Env,
    merchant_address: &Address,
    recipient: &Address,
) {
    merchant_address.require_auth();

    let merchant_id = merchant::get_merchant_id(env, merchant_address);
    let merchant = merchant::get_merchant(env, merchant_id);

    if !merchant.active {
        panic_with_error!(env, ContractError::MerchantNotActive);
    }

    env.storage()
        .persistent()
        .set(&DataKey::MerchantAutoWithdrawalRecipient(merchant_id), recipient);

    events::publish_auto_withdrawal_recipient_set_event(env, merchant_id, recipient.clone());
}

/// Get auto-withdrawal recipient for a merchant
pub fn get_auto_withdrawal_recipient(env: &Env, merchant_id: u64) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::MerchantAutoWithdrawalRecipient(merchant_id))
}

/// Check and trigger auto-withdrawal if balance exceeds threshold
pub fn check_and_trigger_auto_withdrawal(
    env: &Env,
    merchant_id: u64,
    token: &Address,
) -> bool {
    // Get threshold
    let threshold = match get_auto_withdrawal_threshold(env, merchant_id, token) {
        Some(t) => t,
        None => return false, // No threshold set
    };

    if threshold == 0 {
        return false; // Threshold disabled
    }

    // Get merchant account
    let merchant_account = merchant::get_merchant_account(env, merchant_id);

    // Check balance
    let token_client = token::TokenClient::new(env, token);
    let balance = token_client.balance(&merchant_account);

    if balance < threshold {
        return false; // Balance below threshold
    }

    // Get recipient (default to merchant address if not set)
    let merchant = merchant::get_merchant(env, merchant_id);
    let recipient = get_auto_withdrawal_recipient(env, merchant_id)
        .unwrap_or_else(|| merchant.address.clone());

    // Trigger withdrawal
    trigger_auto_withdrawal(env, merchant_id, token, &merchant_account, &recipient, balance);

    true
}

/// Internal function to trigger the actual withdrawal
fn trigger_auto_withdrawal(
    env: &Env,
    merchant_id: u64,
    token: &Address,
    merchant_account: &Address,
    recipient: &Address,
    amount: i128,
) {
    use soroban_sdk::contractclient;

    #[contractclient(name = "MerchantAccountAutoWithdrawalClient")]
    pub trait MerchantAccountAutoWithdrawal {
        fn withdraw_to(env: Env, token: Address, amount: i128, to: Address);
    }

    let client = MerchantAccountAutoWithdrawalClient::new(env, merchant_account);
    client.withdraw_to(&token.clone(), &amount, &recipient.clone());

    events::publish_auto_withdrawal_triggered_event(
        env,
        merchant_id,
        token.clone(),
        amount,
        recipient.clone(),
    );
}
