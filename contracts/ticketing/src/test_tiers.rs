use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, String};

fn make_qr(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

fn setup() -> (Env, Address, TicketingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);
    let organizer = Address::generate(&env);
    (env, organizer, client)
}

fn create_event_with_capacity(
    env: &Env,
    client: &TicketingContractClient,
    organizer: &Address,
    capacity: Option<u64>,
) -> u64 {
    client.create_event(
        organizer,
        &String::from_str(env, "Tiered Event"),
        &String::from_str(env, "VIP + Standard"),
        &1_000_u64,
        &2_000_u64,
        &capacity,
    )
}

// ── Tier creation ──────────────────────────────────────────────────────────────

#[test]
fn test_add_tier_returns_sequential_ids_and_persists_fields() {
    let (env, organizer, client) = setup();
    let event_id = create_event_with_capacity(&env, &client, &organizer, Some(200_u64));

    let vip = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &50_u64,
    );
    let standard = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "Standard"),
        &100_i128,
        &150_u64,
    );

    assert_eq!(vip, 1);
    assert_eq!(standard, 2);

    let vip_tier = client.get_tier(&vip);
    assert_eq!(vip_tier.event_id, event_id);
    assert_eq!(vip_tier.price, 500);
    assert_eq!(vip_tier.max_supply, 50);
    assert_eq!(vip_tier.sold, 0);

    let tiers = client.get_event_tiers(&event_id);
    assert_eq!(tiers.len(), 2);
}

#[test]
#[should_panic]
fn test_non_organizer_cannot_add_tier() {
    let (env, organizer, client) = setup();
    let imposter = Address::generate(&env);
    let event_id = create_event_with_capacity(&env, &client, &organizer, None);

    client.add_tier(
        &imposter,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &10_u64,
    );
}

#[test]
#[should_panic]
fn test_zero_supply_tier_rejected() {
    let (env, organizer, client) = setup();
    let event_id = create_event_with_capacity(&env, &client, &organizer, None);

    client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &0_u64,
    );
}

#[test]
#[should_panic]
fn test_negative_price_rejected() {
    let (env, organizer, client) = setup();
    let event_id = create_event_with_capacity(&env, &client, &organizer, None);

    client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &-1_i128,
        &10_u64,
    );
}

#[test]
fn test_zero_price_tier_allowed_for_free_admission() {
    let (env, organizer, client) = setup();
    let event_id = create_event_with_capacity(&env, &client, &organizer, None);

    let tier_id = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "Free"),
        &0_i128,
        &50_u64,
    );
    assert_eq!(client.get_tier(&tier_id).price, 0);
}

#[test]
#[should_panic]
fn test_combined_tier_supply_cannot_exceed_event_capacity() {
    let (env, organizer, client) = setup();
    // Event has 100 seats — combined tier supply must stay <= 100.
    let event_id = create_event_with_capacity(&env, &client, &organizer, Some(100_u64));

    client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &60_u64,
    );
    // 60 + 50 = 110 > 100 → must panic.
    client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "Standard"),
        &100_i128,
        &50_u64,
    );
}

#[test]
fn test_tier_supply_is_unbounded_when_event_has_no_capacity() {
    let (env, organizer, client) = setup();
    let event_id = create_event_with_capacity(&env, &client, &organizer, None);

    // Without an overall cap the organizer can size tiers freely.
    client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &10_000_u64,
    );
    client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "Standard"),
        &100_i128,
        &50_000_u64,
    );
    assert_eq!(client.get_event_tiers(&event_id).len(), 2);
}

// ── Tiered ticket issuance ─────────────────────────────────────────────────────

#[test]
fn test_issue_tiered_ticket_records_tier_id_and_increments_sold() {
    let (env, organizer, client) = setup();
    let holder = Address::generate(&env);
    let event_id = create_event_with_capacity(&env, &client, &organizer, Some(50_u64));

    let vip = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &10_u64,
    );

    let ticket_id =
        client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 1), &vip);

    let ticket = client.get_ticket(&ticket_id);
    assert_eq!(ticket.tier_id, Some(vip));
    assert_eq!(ticket.holder, holder);
    assert_eq!(client.get_tier(&vip).sold, 1);
}

#[test]
fn test_legacy_issue_ticket_leaves_tier_id_none() {
    let (env, organizer, client) = setup();
    let holder = Address::generate(&env);
    let event_id = create_event_with_capacity(&env, &client, &organizer, None);

    let ticket_id = client.issue_ticket(&organizer, &event_id, &holder, &make_qr(&env, 7));
    assert_eq!(client.get_ticket(&ticket_id).tier_id, None);
}

#[test]
#[should_panic]
fn test_tier_sells_out_independently_of_event_capacity() {
    let (env, organizer, client) = setup();
    let holder = Address::generate(&env);
    // Event capacity is 100, but VIP only has 2 seats.
    let event_id = create_event_with_capacity(&env, &client, &organizer, Some(100_u64));
    let vip = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &2_u64,
    );

    client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 1), &vip);
    client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 2), &vip);
    // 3rd VIP — must panic with TierAtCapacity even though event has room.
    client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 3), &vip);
}

#[test]
#[should_panic]
fn test_tier_must_belong_to_event_passed_in() {
    let (env, organizer, client) = setup();
    let holder = Address::generate(&env);

    let event_a = create_event_with_capacity(&env, &client, &organizer, None);
    let event_b = create_event_with_capacity(&env, &client, &organizer, None);
    let tier_a = client.add_tier(
        &organizer,
        &event_a,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &5_u64,
    );

    // Trying to issue a tier-A ticket against event_b → must panic.
    client.issue_tiered_ticket(&organizer, &event_b, &holder, &make_qr(&env, 1), &tier_a);
}

#[test]
fn test_two_tiers_track_sold_counts_independently() {
    let (env, organizer, client) = setup();
    let holder = Address::generate(&env);
    let event_id = create_event_with_capacity(&env, &client, &organizer, Some(100_u64));

    let vip = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "VIP"),
        &500_i128,
        &10_u64,
    );
    let standard = client.add_tier(
        &organizer,
        &event_id,
        &String::from_str(&env, "Standard"),
        &100_i128,
        &50_u64,
    );

    client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 1), &vip);
    client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 2), &vip);
    client.issue_tiered_ticket(&organizer, &event_id, &holder, &make_qr(&env, 3), &standard);

    assert_eq!(client.get_tier(&vip).sold, 2);
    assert_eq!(client.get_tier(&standard).sold, 1);
}
