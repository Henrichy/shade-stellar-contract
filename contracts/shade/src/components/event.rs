use crate::components::{core, merchant};
use crate::errors::ContractError;
use crate::types::{DataKey, Event, EventStatus, Merchant};
use soroban_sdk::{panic_with_error, Address, Env, String, Vec};

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
        status: EventStatus::Active,
        holders: Vec::new(env),
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
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InvoiceNotFound));

    if event.status != EventStatus::Active {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    if event.sold >= event.capacity {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    event.sold += 1;
    event.holders.push_back(buyer.clone());
    env.storage().persistent().set(&DataKey::Event(*event_id), &event);
}

pub fn cancel_event(env: &Env, event_id: &u64, merchant_addr: &Address) {
    merchant_addr.require_auth();

    let mut event: Event = env
        .storage()
        .persistent()
        .get(&DataKey::Event(*event_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InvoiceNotFound));

    let merchant_id = crate::components::merchant::get_merchant_id(env, merchant_addr);
    if event.merchant_id != merchant_id {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    if event.status == EventStatus::Cancelled {
        panic_with_error!(env, ContractError::InvalidAmount);
    }

    event.status = EventStatus::Cancelled;
    env.storage().persistent().set(&DataKey::Event(*event_id), &event);
}

pub fn get_event(env: &Env, event_id: &u64) -> Event {
    env.storage()
        .persistent()
        .get(&DataKey::Event(*event_id))
        .unwrap()
}
