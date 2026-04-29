#![no_std]

mod errors;
#[cfg(test)]
mod test_integration;
#[cfg(test)]
mod test_resale;
#[cfg(test)]
mod test_tiers;

use crate::errors::TicketingError;
use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, panic_with_error, token, Address, BytesN,
    Env, String, Vec,
};

/// Basis-point denominator used for royalty math (10_000 = 100%).
const MAX_BPS: u32 = 10_000;

// ── Data Structures ────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub event_id: u64,
    pub organizer: Address,
    pub name: String,
    pub description: String,
    pub start_time: u64,
    pub end_time: u64,
    pub max_capacity: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ticket {
    pub ticket_id: u64,
    pub event_id: u64,
    pub holder: Address,
    pub qr_hash: BytesN<32>,
    pub checked_in: bool,
    pub check_in_time: Option<u64>,
    /// Pricing tier this ticket belongs to. `None` means the ticket was issued
    /// without tier metadata (e.g. flat-priced events).
    pub tier_id: Option<u64>,
}

/// A pricing tier within an event (e.g. "VIP", "Standard", "Early Bird").
/// Each tier has its own capacity and price; total supply across tiers is
/// validated against the event's `max_capacity` when set.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tier {
    pub tier_id: u64,
    pub event_id: u64,
    pub name: String,
    /// Token base-unit price per ticket in this tier. Stored on-chain so
    /// off-chain UIs can show consistent pricing.
    pub price: i128,
    pub max_supply: u64,
    pub sold: u64,
}

/// Resale royalty configuration for an event. When present, secondary-market
/// transfers via [`TicketingContract::resell_ticket`] route a percentage of
/// the sale price to the organizer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResaleConfig {
    pub event_id: u64,
    /// Token used to settle resales for this event.
    pub payment_token: Address,
    /// Royalty in basis points (10_000 = 100%). Must be <= 10_000.
    pub royalty_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckInRecord {
    pub ticket_id: u64,
    pub checked_in_by: Address,
    pub check_in_time: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TicketVerification {
    pub ticket_id: u64,
    pub event_id: u64,
    pub holder: Address,
    pub valid: bool,
    pub already_checked_in: bool,
}

// ── Storage Keys ───────────────────────────────────────────────────────────────

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Event(u64),
    Ticket(u64),
    EventCount,
    TicketCount,
    EventTickets(u64),  // Vec<u64> - ticket IDs for an event
    CheckInRecord(u64), // CheckInRecord by ticket_id
    Tier(u64),
    TierCount,
    EventTiers(u64),   // Vec<u64> - tier IDs for an event
    ResaleConfig(u64), // ResaleConfig keyed by event_id
}

// ── Events ─────────────────────────────────────────────────────────────────────

#[contractevent]
pub struct EventCreatedEvent {
    pub event_id: u64,
    pub organizer: Address,
    pub name: String,
    pub timestamp: u64,
}

pub fn publish_event_created_event(
    env: &Env,
    event_id: u64,
    organizer: Address,
    name: String,
    timestamp: u64,
) {
    EventCreatedEvent {
        event_id,
        organizer,
        name,
        timestamp,
    }
    .publish(env);
}

#[contractevent]
pub struct TicketIssuedEvent {
    pub ticket_id: u64,
    pub event_id: u64,
    pub holder: Address,
    pub qr_hash: BytesN<32>,
    pub timestamp: u64,
}

pub fn publish_ticket_issued_event(
    env: &Env,
    ticket_id: u64,
    event_id: u64,
    holder: Address,
    qr_hash: BytesN<32>,
    timestamp: u64,
) {
    TicketIssuedEvent {
        ticket_id,
        event_id,
        holder,
        qr_hash,
        timestamp,
    }
    .publish(env);
}

#[contractevent]
pub struct TicketCheckedInEvent {
    pub ticket_id: u64,
    pub event_id: u64,
    pub holder: Address,
    pub checked_in_by: Address,
    pub check_in_time: u64,
}

pub fn publish_ticket_checked_in_event(
    env: &Env,
    ticket_id: u64,
    event_id: u64,
    holder: Address,
    checked_in_by: Address,
    check_in_time: u64,
) {
    TicketCheckedInEvent {
        ticket_id,
        event_id,
        holder,
        checked_in_by,
        check_in_time,
    }
    .publish(env);
}

#[contractevent]
pub struct TicketTransferedEvent {
    pub ticket_id: u64,
    pub event_id: u64,
    pub old_holder: Address,
    pub new_holder: Address,
    pub timestamp: u64,
}

pub fn publish_ticket_transferred_event(
    env: &Env,
    ticket_id: u64,
    event_id: u64,
    old_holder: Address,
    new_holder: Address,
    timestamp: u64,
) {
    TicketTransferedEvent {
        ticket_id,
        event_id,
        old_holder,
        new_holder,
        timestamp,
    }
    .publish(env);
}

#[contractevent]
pub struct TierCreatedEvent {
    pub tier_id: u64,
    pub event_id: u64,
    pub name: String,
    pub price: i128,
    pub max_supply: u64,
    pub timestamp: u64,
}

#[contractevent]
pub struct ResaleConfiguredEvent {
    pub event_id: u64,
    pub organizer: Address,
    pub payment_token: Address,
    pub royalty_bps: u32,
    pub timestamp: u64,
}

#[contractevent]
pub struct TicketResoldEvent {
    pub ticket_id: u64,
    pub event_id: u64,
    pub seller: Address,
    pub buyer: Address,
    pub sale_price: i128,
    pub royalty: i128,
    pub seller_proceeds: i128,
    pub payment_token: Address,
    pub timestamp: u64,
}

// ── Helper Functions ───────────────────────────────────────────────────────────

fn get_event_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::EventCount)
        .unwrap_or(0)
}

fn get_ticket_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::TicketCount)
        .unwrap_or(0)
}

fn increment_event_count(env: &Env, count: u64) {
    env.storage().persistent().set(&DataKey::EventCount, &count);
}

fn increment_ticket_count(env: &Env, count: u64) {
    env.storage()
        .persistent()
        .set(&DataKey::TicketCount, &count);
}

fn add_ticket_to_event(env: &Env, event_id: u64, ticket_id: u64) {
    let key = DataKey::EventTickets(event_id);
    let mut tickets: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    tickets.push_back(ticket_id);
    env.storage().persistent().set(&key, &tickets);
}

fn is_event_organizer(env: &Env, event_id: u64, organizer: &Address) -> bool {
    let event: Event = env
        .storage()
        .persistent()
        .get(&DataKey::Event(event_id))
        .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound));
    &event.organizer == organizer
}

fn get_tier_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::TierCount)
        .unwrap_or(0)
}

fn sum_tier_max_supply(env: &Env, event_id: u64) -> u64 {
    let tier_ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&DataKey::EventTiers(event_id))
        .unwrap_or_else(|| Vec::new(env));
    let mut sum: u64 = 0;
    for id in tier_ids.iter() {
        if let Some(tier) = env
            .storage()
            .persistent()
            .get::<_, Tier>(&DataKey::Tier(id))
        {
            sum = sum.saturating_add(tier.max_supply);
        }
    }
    sum
}

/// `value * bps / 10_000` with checked multiplication so overflow on the
/// intermediate product surfaces as `None` instead of silently wrapping.
fn bps_of(value: i128, bps: u32) -> Option<i128> {
    let scaled = value.checked_mul(bps as i128)?;
    Some(scaled / MAX_BPS as i128)
}

// ── Contract ───────────────────────────────────────────────────────────────────

#[contract]
pub struct TicketingContract;

#[contractimpl]
impl TicketingContract {
    /// Creates a new event. Only the organizer can later issue tickets for this event.
    pub fn create_event(
        env: Env,
        organizer: Address,
        name: String,
        description: String,
        start_time: u64,
        end_time: u64,
        max_capacity: Option<u64>,
    ) -> u64 {
        organizer.require_auth();

        if end_time < start_time {
            panic_with_error!(env, TicketingError::InvalidTimeRange);
        }

        let event_count = get_event_count(&env);
        let new_event_id = event_count + 1;

        let event = Event {
            event_id: new_event_id,
            organizer: organizer.clone(),
            name,
            description,
            start_time,
            end_time,
            max_capacity,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Event(new_event_id), &event);

        increment_event_count(&env, new_event_id);

        publish_event_created_event(
            &env,
            new_event_id,
            organizer,
            event.name,
            env.ledger().timestamp(),
        );

        new_event_id
    }

    /// Get event details by ID.
    pub fn get_event(env: Env, event_id: u64) -> Event {
        env.storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound))
    }

    /// Issue a ticket for an event.
    /// The qr_hash must be a secure unique hash (SHA256) generated off-chain.
    pub fn issue_ticket(
        env: Env,
        organizer: Address,
        event_id: u64,
        holder: Address,
        qr_hash: BytesN<32>,
    ) -> u64 {
        organizer.require_auth();

        // Verify event exists and organizer owns it
        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound));

        if event.organizer != organizer {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }

        // Check capacity if set
        if let Some(max_cap) = event.max_capacity {
            let tickets: Vec<u64> = env
                .storage()
                .persistent()
                .get(&DataKey::EventTickets(event_id))
                .unwrap_or_else(|| Vec::new(&env));
            if tickets.len() as u64 >= max_cap {
                panic_with_error!(env, TicketingError::EventAtCapacity);
            }
        }

        // Ensure QR hash uniqueness (no duplicate hashes across all tickets)
        let ticket_count = get_ticket_count(&env);
        for i in 1..=ticket_count {
            if let Some(ticket) = env
                .storage()
                .persistent()
                .get::<_, Ticket>(&DataKey::Ticket(i))
            {
                if ticket.qr_hash == qr_hash {
                    panic_with_error!(env, TicketingError::DuplicateQRHash);
                }
            }
        }

        let ticket_count = get_ticket_count(&env);
        let new_ticket_id = ticket_count + 1;

        let ticket = Ticket {
            ticket_id: new_ticket_id,
            event_id,
            holder: holder.clone(),
            qr_hash: qr_hash.clone(),
            checked_in: false,
            check_in_time: None,
            tier_id: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Ticket(new_ticket_id), &ticket);

        add_ticket_to_event(&env, event_id, new_ticket_id);
        increment_ticket_count(&env, new_ticket_id);

        publish_ticket_issued_event(
            &env,
            new_ticket_id,
            event_id,
            holder,
            qr_hash,
            env.ledger().timestamp(),
        );

        new_ticket_id
    }

    /// Add a pricing tier (e.g. "VIP", "Standard") to an existing event.
    /// Tiers let an organizer charge different prices and cap supply per
    /// category within the same event. Only the event's organizer may add
    /// tiers; the combined `max_supply` across tiers cannot exceed the
    /// event's overall capacity (when set).
    pub fn add_tier(
        env: Env,
        organizer: Address,
        event_id: u64,
        name: String,
        price: i128,
        max_supply: u64,
    ) -> u64 {
        organizer.require_auth();

        if price < 0 {
            panic_with_error!(env, TicketingError::InvalidTierPrice);
        }
        if max_supply == 0 {
            panic_with_error!(env, TicketingError::InvalidTierSupply);
        }

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound));

        if event.organizer != organizer {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }

        // If the event has an overall capacity, ensure the new tier doesn't
        // push the total committed tier supply beyond it. This prevents an
        // organizer from over-promising tickets that can never be issued.
        if let Some(cap) = event.max_capacity {
            let existing_tier_supply = sum_tier_max_supply(&env, event_id);
            let new_total = existing_tier_supply.saturating_add(max_supply);
            if new_total > cap {
                panic_with_error!(env, TicketingError::TierAtCapacity);
            }
        }

        let new_tier_id = get_tier_count(&env) + 1;

        let tier = Tier {
            tier_id: new_tier_id,
            event_id,
            name: name.clone(),
            price,
            max_supply,
            sold: 0,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Tier(new_tier_id), &tier);

        let mut event_tiers: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::EventTiers(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        event_tiers.push_back(new_tier_id);
        env.storage()
            .persistent()
            .set(&DataKey::EventTiers(event_id), &event_tiers);

        env.storage()
            .persistent()
            .set(&DataKey::TierCount, &new_tier_id);

        TierCreatedEvent {
            tier_id: new_tier_id,
            event_id,
            name,
            price,
            max_supply,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);

        new_tier_id
    }

    /// Issue a ticket bound to a specific pricing tier.
    /// Increments the tier's `sold` counter and rejects further issuance once
    /// the tier's `max_supply` is reached, regardless of remaining event
    /// capacity. The tier must belong to the supplied event.
    pub fn issue_tiered_ticket(
        env: Env,
        organizer: Address,
        event_id: u64,
        holder: Address,
        qr_hash: BytesN<32>,
        tier_id: u64,
    ) -> u64 {
        organizer.require_auth();

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound));

        if event.organizer != organizer {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }

        let mut tier: Tier = env
            .storage()
            .persistent()
            .get(&DataKey::Tier(tier_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TierNotFound));

        if tier.event_id != event_id {
            panic_with_error!(env, TicketingError::TierEventMismatch);
        }
        if tier.sold >= tier.max_supply {
            panic_with_error!(env, TicketingError::TierAtCapacity);
        }

        if let Some(max_cap) = event.max_capacity {
            let tickets: Vec<u64> = env
                .storage()
                .persistent()
                .get(&DataKey::EventTickets(event_id))
                .unwrap_or_else(|| Vec::new(&env));
            if tickets.len() as u64 >= max_cap {
                panic_with_error!(env, TicketingError::EventAtCapacity);
            }
        }

        let ticket_count = get_ticket_count(&env);
        for i in 1..=ticket_count {
            if let Some(t) = env
                .storage()
                .persistent()
                .get::<_, Ticket>(&DataKey::Ticket(i))
            {
                if t.qr_hash == qr_hash {
                    panic_with_error!(env, TicketingError::DuplicateQRHash);
                }
            }
        }

        let new_ticket_id = ticket_count + 1;
        let ticket = Ticket {
            ticket_id: new_ticket_id,
            event_id,
            holder: holder.clone(),
            qr_hash: qr_hash.clone(),
            checked_in: false,
            check_in_time: None,
            tier_id: Some(tier_id),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Ticket(new_ticket_id), &ticket);
        add_ticket_to_event(&env, event_id, new_ticket_id);
        increment_ticket_count(&env, new_ticket_id);

        tier.sold += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Tier(tier_id), &tier);

        publish_ticket_issued_event(
            &env,
            new_ticket_id,
            event_id,
            holder,
            qr_hash,
            env.ledger().timestamp(),
        );

        new_ticket_id
    }

    pub fn get_tier(env: Env, tier_id: u64) -> Tier {
        env.storage()
            .persistent()
            .get(&DataKey::Tier(tier_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TierNotFound))
    }

    pub fn get_event_tiers(env: Env, event_id: u64) -> Vec<Tier> {
        let tier_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::EventTiers(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        let mut tiers = Vec::new(&env);
        for id in tier_ids.iter() {
            let tier: Tier = env.storage().persistent().get(&DataKey::Tier(id)).unwrap();
            tiers.push_back(tier);
        }
        tiers
    }

    /// Get ticket details by ID.
    pub fn get_ticket(env: Env, ticket_id: u64) -> Ticket {
        env.storage()
            .persistent()
            .get(&DataKey::Ticket(ticket_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TicketNotFound))
    }

    /// Get all tickets for an event.
    pub fn get_event_tickets(env: Env, event_id: u64) -> Vec<Ticket> {
        let ticket_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::EventTickets(event_id))
            .unwrap_or_else(|| Vec::new(&env));

        let mut tickets = Vec::new(&env);
        for ticket_id in ticket_ids.iter() {
            let ticket: Ticket = env
                .storage()
                .persistent()
                .get(&DataKey::Ticket(ticket_id))
                .unwrap();
            tickets.push_back(ticket);
        }
        tickets
    }

    /// Verify a ticket by comparing the provided QR hash with stored hash.
    /// Returns ticket verification status without marking as checked in.
    pub fn verify_ticket(env: Env, ticket_id: u64, qr_hash: BytesN<32>) -> TicketVerification {
        let ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(ticket_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TicketNotFound));

        let event_id = ticket.event_id;

        let valid = ticket.qr_hash == qr_hash;
        let already_checked_in = ticket.checked_in;

        TicketVerification {
            ticket_id,
            event_id,
            holder: ticket.holder,
            valid,
            already_checked_in,
        }
    }

    /// Check in a ticket. This is a state transition: pending → checked_in.
    /// Cannot be undone. Duplicate check-ins are rejected.
    pub fn check_in(env: Env, operator: Address, ticket_id: u64) {
        operator.require_auth();

        // Get the ticket
        let mut ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(ticket_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TicketNotFound));

        // Only event organizer can mark tickets as consumed.
        if !is_event_organizer(&env, ticket.event_id, &operator) {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }

        // Prevent double check-in (idempotency)
        if ticket.checked_in {
            panic_with_error!(env, TicketingError::AlreadyCheckedIn);
        }

        let check_in_time = env.ledger().timestamp();

        ticket.checked_in = true;
        ticket.check_in_time = Some(check_in_time);

        // Save updated ticket
        env.storage()
            .persistent()
            .set(&DataKey::Ticket(ticket_id), &ticket);

        // Record check-in metadata (who scanned, when)
        let check_in_record = CheckInRecord {
            ticket_id,
            checked_in_by: operator.clone(),
            check_in_time,
        };
        env.storage()
            .persistent()
            .set(&DataKey::CheckInRecord(ticket_id), &check_in_record);

        // Emit event
        publish_ticket_checked_in_event(
            &env,
            ticket_id,
            ticket.event_id,
            ticket.holder,
            operator,
            check_in_time,
        );
    }

    /// Transfer a ticket from current holder to a new holder.
    /// Cannot transfer a checked-in ticket.
    pub fn transfer_ticket(env: Env, current_holder: Address, ticket_id: u64, new_holder: Address) {
        current_holder.require_auth();

        let mut ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(ticket_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TicketNotFound));

        if ticket.holder != current_holder {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }

        if ticket.checked_in {
            panic_with_error!(env, TicketingError::TicketAlreadyCheckedIn);
        }

        let old_holder = ticket.holder.clone();
        ticket.holder = new_holder.clone();

        env.storage()
            .persistent()
            .set(&DataKey::Ticket(ticket_id), &ticket);

        publish_ticket_transferred_event(
            &env,
            ticket_id,
            ticket.event_id,
            old_holder,
            new_holder,
            env.ledger().timestamp(),
        );
    }

    /// Get the check-in record for a ticket.
    pub fn get_check_in_record(env: Env, ticket_id: u64) -> Option<CheckInRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::CheckInRecord(ticket_id))
    }

    /// Get total number of tickets issued for an event.
    pub fn get_event_ticket_count(env: Env, event_id: u64) -> u64 {
        let ticket_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::EventTickets(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        ticket_ids.len() as u64
    }

    /// Get total number of checked-in tickets for an event.
    pub fn get_event_checked_in_count(env: Env, event_id: u64) -> u64 {
        let ticket_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::EventTickets(event_id))
            .unwrap_or_else(|| Vec::new(&env));
        let mut count = 0;
        for ticket_id in ticket_ids.iter() {
            let ticket: Ticket = env
                .storage()
                .persistent()
                .get(&DataKey::Ticket(ticket_id))
                .unwrap();
            if ticket.checked_in {
                count += 1;
            }
        }
        count
    }

    /// Configure royalty + payment token for secondary-market resales of an
    /// event's tickets. Only the event organizer may call this; setting it
    /// again overwrites the previous configuration. `royalty_bps` is in basis
    /// points (10_000 = 100%).
    pub fn set_resale_config(
        env: Env,
        organizer: Address,
        event_id: u64,
        payment_token: Address,
        royalty_bps: u32,
    ) {
        organizer.require_auth();

        if royalty_bps > MAX_BPS {
            panic_with_error!(env, TicketingError::InvalidRoyaltyBps);
        }

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound));

        if event.organizer != organizer {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }

        let config = ResaleConfig {
            event_id,
            payment_token: payment_token.clone(),
            royalty_bps,
        };
        env.storage()
            .persistent()
            .set(&DataKey::ResaleConfig(event_id), &config);

        ResaleConfiguredEvent {
            event_id,
            organizer,
            payment_token,
            royalty_bps,
            timestamp: env.ledger().timestamp(),
        }
        .publish(&env);
    }

    pub fn get_resale_config(env: Env, event_id: u64) -> ResaleConfig {
        env.storage()
            .persistent()
            .get(&DataKey::ResaleConfig(event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::ResaleNotConfigured))
    }

    /// Resell a ticket on the secondary market with automatic royalty payout
    /// to the event organizer. The event must have a `ResaleConfig` set.
    /// `sale_price` is paid by the buyer; the configured royalty share goes
    /// to the organizer and the remainder to the seller.
    ///
    /// A checked-in ticket cannot be resold; reselling to oneself is rejected.
    pub fn resell_ticket(
        env: Env,
        seller: Address,
        buyer: Address,
        ticket_id: u64,
        sale_price: i128,
    ) {
        // Both parties must authorize: the seller for the ticket transfer and
        // the buyer for the token outflow. Without buyer auth the inner
        // `token.transfer` would fail under recording-mode auth.
        seller.require_auth();
        buyer.require_auth();

        if sale_price <= 0 {
            panic_with_error!(env, TicketingError::InvalidResalePrice);
        }
        if seller == buyer {
            panic_with_error!(env, TicketingError::SameHolder);
        }

        let mut ticket: Ticket = env
            .storage()
            .persistent()
            .get(&DataKey::Ticket(ticket_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::TicketNotFound));

        if ticket.holder != seller {
            panic_with_error!(env, TicketingError::NotAuthorized);
        }
        if ticket.checked_in {
            panic_with_error!(env, TicketingError::TicketAlreadyCheckedIn);
        }

        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(ticket.event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::EventNotFound));

        let config: ResaleConfig = env
            .storage()
            .persistent()
            .get(&DataKey::ResaleConfig(ticket.event_id))
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::ResaleNotConfigured));

        let royalty = bps_of(sale_price, config.royalty_bps)
            .unwrap_or_else(|| panic_with_error!(env, TicketingError::InvalidResalePrice));
        if royalty < 0 || royalty > sale_price {
            panic_with_error!(env, TicketingError::InvalidResalePrice);
        }
        let seller_proceeds = sale_price - royalty;

        let token_client = token::TokenClient::new(&env, &config.payment_token);
        if seller_proceeds > 0 {
            token_client.transfer(&buyer, &seller, &seller_proceeds);
        }
        if royalty > 0 {
            token_client.transfer(&buyer, &event.organizer, &royalty);
        }

        let old_holder = ticket.holder.clone();
        ticket.holder = buyer.clone();
        env.storage()
            .persistent()
            .set(&DataKey::Ticket(ticket_id), &ticket);

        let now = env.ledger().timestamp();
        publish_ticket_transferred_event(
            &env,
            ticket_id,
            ticket.event_id,
            old_holder,
            buyer.clone(),
            now,
        );
        TicketResoldEvent {
            ticket_id,
            event_id: ticket.event_id,
            seller,
            buyer,
            sale_price,
            royalty,
            seller_proceeds,
            payment_token: config.payment_token,
            timestamp: now,
        }
        .publish(&env);
    }
}
