use crate::components::{admin, merchant};
use crate::errors::ContractError;
use crate::events;
use crate::types::{DataKey, Event, Merchant, Ticket};
use soroban_sdk::{panic_with_error, token, Address, Env, String, Vec};

const MAX_BPS: u32 = 10_000;

// ── Event creation (Issue #246) ───────────────────────────────────────────────

pub fn create_event(
    env: &Env,
    merchant_addr: &Address,
    name: &String,
    ticket_price: &i128,
    token: &Address,
    capacity: &u32,
    event_date: &u64,
    royalty_bps: &u32,
) -> u64 {
    merchant_addr.require_auth();

    if *ticket_price <= 0 {
        panic_with_error!(env, ContractError::InvalidAmount);
    }
    if *capacity == 0 {
        panic_with_error!(env, ContractError::InvalidCapacity);
    }
    if *royalty_bps > MAX_BPS {
        panic_with_error!(env, ContractError::InvalidRoyaltyBps);
    }
    if *event_date < env.ledger().timestamp() {
        panic_with_error!(env, ContractError::InvalidEventDate);
    }
    if !admin::is_accepted_token(env, token) {
        panic_with_error!(env, ContractError::TokenNotAccepted);
    }

    let merchant_id = merchant::get_merchant_id(env, merchant_addr);
    let merchant_record: Merchant = env
        .storage()
        .persistent()
        .get(&DataKey::Merchant(merchant_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::MerchantNotFound));

    if !merchant_record.active {
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
        event_date: *event_date,
        royalty_bps: *royalty_bps,
    };

    env.storage().persistent().set(&DataKey::Event(id), &event);
    env.storage().persistent().set(&DataKey::EventCount, &id);
    env.storage()
        .persistent()
        .set(&DataKey::EventTickets(id), &Vec::<u64>::new(env));

    events::publish_event_created_event(
        env,
        id,
        merchant_addr.clone(),
        merchant_id,
        name.clone(),
        *ticket_price,
        token.clone(),
        *capacity,
        *event_date,
        *royalty_bps,
        env.ledger().timestamp(),
    );

    id
}

pub fn get_event(env: &Env, event_id: &u64) -> Event {
    env.storage()
        .persistent()
        .get(&DataKey::Event(*event_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::EventNotFound))
}

// ── Ticket purchase via Shade payment (Issues #247 + #248) ────────────────────

pub fn purchase_ticket(env: &Env, event_id: &u64, buyer: &Address) -> u64 {
    buyer.require_auth();

    let mut event = get_event(env, event_id);

    if event.sold >= event.capacity {
        panic_with_error!(env, ContractError::EventSoldOut);
    }

    // Re-validate token in case admin removed it after event creation.
    if !admin::is_accepted_token(env, &event.token) {
        panic_with_error!(env, ContractError::TokenNotAccepted);
    }

    let merchant_address = merchant_id_to_address(env, event.merchant_id);
    let merchant_account = merchant::get_merchant_account(env, event.merchant_id);
    let platform_account = admin::get_platform_account(env);

    let amount = event.ticket_price;
    let fee = admin::calculate_fee(env, &merchant_address, &event.token, amount);
    if fee < 0 || fee >= amount {
        panic_with_error!(env, ContractError::InvalidAmount);
    }
    let merchant_amount = amount - fee;

    let token_client = token::TokenClient::new(env, &event.token);
    token_client.transfer(buyer, &merchant_account, &merchant_amount);
    if fee > 0 {
        token_client.transfer(buyer, &platform_account, &fee);
    }

    admin::record_merchant_payment(env, &merchant_address, &event.token, amount, fee);

    let new_ticket_id = env
        .storage()
        .persistent()
        .get(&DataKey::TicketCount)
        .unwrap_or(0u64)
        + 1;

    let ticket = Ticket {
        id: new_ticket_id,
        event_id: *event_id,
        owner: buyer.clone(),
        minted_at: env.ledger().timestamp(),
    };

    env.storage()
        .persistent()
        .set(&DataKey::Ticket(new_ticket_id), &ticket);
    env.storage()
        .persistent()
        .set(&DataKey::TicketCount, &new_ticket_id);

    let mut event_tickets: Vec<u64> = env
        .storage()
        .persistent()
        .get(&DataKey::EventTickets(*event_id))
        .unwrap_or_else(|| Vec::new(env));
    event_tickets.push_back(new_ticket_id);
    env.storage()
        .persistent()
        .set(&DataKey::EventTickets(*event_id), &event_tickets);

    add_user_ticket(env, buyer, new_ticket_id);

    event.sold += 1;
    env.storage()
        .persistent()
        .set(&DataKey::Event(*event_id), &event);

    events::publish_ticket_purchased_event(
        env,
        new_ticket_id,
        *event_id,
        event.merchant_id,
        buyer.clone(),
        amount,
        fee,
        merchant_amount,
        event.token.clone(),
        env.ledger().timestamp(),
    );

    new_ticket_id
}

// ── Resale with royalty (Issue #254) ──────────────────────────────────────────

pub fn resell_ticket(
    env: &Env,
    seller: &Address,
    buyer: &Address,
    ticket_id: u64,
    resale_price: i128,
) {
    seller.require_auth();

    if resale_price <= 0 {
        panic_with_error!(env, ContractError::InvalidResalePrice);
    }

    let mut ticket: Ticket = env
        .storage()
        .persistent()
        .get(&DataKey::Ticket(ticket_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::TicketNotFound));

    if ticket.owner != *seller {
        panic_with_error!(env, ContractError::NotTicketOwner);
    }
    if seller == buyer {
        panic_with_error!(env, ContractError::NotAuthorized);
    }

    let event = get_event(env, &ticket.event_id);

    if !admin::is_accepted_token(env, &event.token) {
        panic_with_error!(env, ContractError::TokenNotAccepted);
    }

    let royalty = bps_of(resale_price, event.royalty_bps)
        .unwrap_or_else(|| panic_with_error!(env, ContractError::InvalidAmount));
    if royalty < 0 || royalty > resale_price {
        panic_with_error!(env, ContractError::InvalidAmount);
    }
    let seller_proceeds = resale_price - royalty;

    let merchant_account = merchant::get_merchant_account(env, event.merchant_id);
    let token_client = token::TokenClient::new(env, &event.token);

    if seller_proceeds > 0 {
        token_client.transfer(buyer, seller, &seller_proceeds);
    }
    if royalty > 0 {
        token_client.transfer(buyer, &merchant_account, &royalty);
    }

    let prev_owner = ticket.owner.clone();
    ticket.owner = buyer.clone();
    env.storage()
        .persistent()
        .set(&DataKey::Ticket(ticket_id), &ticket);

    remove_user_ticket(env, &prev_owner, ticket_id);
    add_user_ticket(env, buyer, ticket_id);

    events::publish_ticket_resold_event(
        env,
        ticket_id,
        ticket.event_id,
        event.merchant_id,
        prev_owner,
        buyer.clone(),
        resale_price,
        royalty,
        seller_proceeds,
        event.token.clone(),
        env.ledger().timestamp(),
    );
}

pub fn get_ticket(env: &Env, ticket_id: u64) -> Ticket {
    env.storage()
        .persistent()
        .get(&DataKey::Ticket(ticket_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::TicketNotFound))
}

pub fn get_event_tickets(env: &Env, event_id: u64) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::EventTickets(event_id))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn get_user_tickets(env: &Env, user: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::UserTickets(user.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn merchant_id_to_address(env: &Env, merchant_id: u64) -> Address {
    let m: Merchant = env
        .storage()
        .persistent()
        .get(&DataKey::Merchant(merchant_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::MerchantNotFound));
    m.address
}

fn add_user_ticket(env: &Env, user: &Address, ticket_id: u64) {
    let key = DataKey::UserTickets(user.clone());
    let mut list: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    list.push_back(ticket_id);
    env.storage().persistent().set(&key, &list);
}

fn remove_user_ticket(env: &Env, user: &Address, ticket_id: u64) {
    let key = DataKey::UserTickets(user.clone());
    let list: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    let mut new_list: Vec<u64> = Vec::new(env);
    for id in list.iter() {
        if id != ticket_id {
            new_list.push_back(id);
        }
    }
    env.storage().persistent().set(&key, &new_list);
}

// `value * bps / 10_000` with checked multiplication to catch overflow on the
// intermediate product before it would silently wrap.
fn bps_of(value: i128, bps: u32) -> Option<i128> {
    let scaled = value.checked_mul(bps as i128)?;
    Some(scaled / MAX_BPS as i128)
}
