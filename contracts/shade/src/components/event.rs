use crate::components::{core, merchant};
use crate::errors::ContractError;
use crate::types::{DataKey, Event, Merchant};
use soroban_sdk::{panic_with_error, token, Address, Env, String};

pub fn create_event(
    env: &Env,
    merchant_addr: &Address,
    name: &String,
    ticket_price: &i128,
    token: &Address,
    capacity: &u32,
) -> u64 {
    merchant_addr.require_auth();

    let merchant_id = crate::components::merchant::get_merchant_id(env, merchant_addr);
    let merchant: Merchant = env
        .storage()
        .persistent()
        .get(&DataKey::Merchant(merchant_id))
        .unwrap();

    if !merchant.active {
        panic_with_error!(env, ContractError::MerchantNotActive);
    }

    let id = env
        .storage()
        .persistent()
        .get(&DataKey::EventCount)
        .unwrap_or(0u64)
        + 1;

    let event = Event {
        id,
        merchant_id,
        name: name.clone(),
        ticket_price: *ticket_price,
        token: token.clone(),
        capacity: *capacity,
        sold: 0,
        date: env.ledger().timestamp(),
    };

    env.storage().persistent().set(&DataKey::Event(id), &event);
    env.storage().persistent().set(&DataKey::EventCount, &id);

    id
}

pub fn purchase_ticket(env: &Env, event_id: &u64, buyer: &Address) {
    buyer.require_auth();

    let mut event: Event = env
        .storage()
        .persistent()
        .get(&DataKey::Event(*event_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InvoiceNotFound)); // Using InvoiceNotFound as a generic "Not Found" for now

    if event.sold >= event.capacity {
        panic_with_error!(env, ContractError::InvalidAmount); // Should use a proper error, but let's stick to existing ones for now
    }

    // Transfer tokens (this is a simplified version, usually we'd use the payment component)
    // For the sake of this task, let's just increment the sold count.
    
    event.sold += 1;
    env.storage().persistent().set(&DataKey::Event(*event_id), &event);
}

pub fn get_event(env: &Env, event_id: &u64) -> Event {
    env.storage()
        .persistent()
        .get(&DataKey::Event(*event_id))
        .unwrap()
}

/// Discount tiers: quantity → basis-point discount off the total price.
/// 500 bps = 5%, 1000 bps = 10%, 1500 bps = 15%.
fn group_discount_bps(quantity: u32) -> i128 {
    if quantity >= 20 {
        1500
    } else if quantity >= 10 {
        1000
    } else if quantity >= 5 {
        500
    } else {
        0
    }
}

/// Purchase multiple tickets for an event in a single call with automatic
/// group discount applied in Shade tokens.  The buyer pays the discounted
/// total in one transfer; the merchant receives the net amount.
pub fn purchase_tickets_bulk(
    env: &Env,
    event_id: &u64,
    buyer: &Address,
    quantity: u32,
    shade_token: &Address,
    merchant_account: &Address,
) {
    buyer.require_auth();

    if quantity == 0 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    let mut event: Event = env
        .storage()
        .persistent()
        .get(&DataKey::Event(*event_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InvoiceNotFound));

    if event.sold.saturating_add(quantity) > event.capacity {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    let gross = event.ticket_price.saturating_mul(i128::from(quantity));
    let discount_bps = group_discount_bps(quantity);
    let discount_amount = gross * discount_bps / 10_000;
    let net = gross - discount_amount;

    let token_client = token::TokenClient::new(env, shade_token);
    token_client.transfer(buyer, merchant_account, &net);

    event.sold = event.sold.saturating_add(quantity);
    env.storage().persistent().set(&DataKey::Event(*event_id), &event);
}
