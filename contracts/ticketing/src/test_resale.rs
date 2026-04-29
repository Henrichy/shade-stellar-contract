use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env, String};

fn make_qr(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

struct Fixture<'a> {
    env: Env,
    client: TicketingContractClient<'a>,
    organizer: Address,
    seller: Address,
    buyer: Address,
    event_id: u64,
    ticket_id: u64,
    token: Address,
}

fn setup_event_with_ticket() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Resale Test"),
        &String::from_str(&env, "Royalty enforcement"),
        &1_000_u64,
        &2_000_u64,
        &None::<u64>,
    );

    let ticket_id = client.issue_ticket(&organizer, &event_id, &seller, &make_qr(&env, 1));

    // Register a Stellar asset contract that we can mint freely against.
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    Fixture {
        env,
        client,
        organizer,
        seller,
        buyer,
        event_id,
        ticket_id,
        token,
    }
}

fn fund(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn balance(env: &Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(env, token).balance(who)
}

// ── Resale configuration ───────────────────────────────────────────────────────

#[test]
fn test_set_resale_config_persists_fields() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);

    let cfg = f.client.get_resale_config(&f.event_id);
    assert_eq!(cfg.event_id, f.event_id);
    assert_eq!(cfg.payment_token, f.token);
    assert_eq!(cfg.royalty_bps, 500);
}

#[test]
fn test_resale_config_overwrites_on_resubmit() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &1000_u32);

    assert_eq!(f.client.get_resale_config(&f.event_id).royalty_bps, 1000);
}

#[test]
#[should_panic]
fn test_non_organizer_cannot_set_resale_config() {
    let f = setup_event_with_ticket();
    let imposter = Address::generate(&f.env);
    f.client
        .set_resale_config(&imposter, &f.event_id, &f.token, &500_u32);
}

#[test]
#[should_panic]
fn test_royalty_bps_above_10000_rejected() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &10_001_u32);
}

#[test]
#[should_panic]
fn test_get_resale_config_panics_when_unset() {
    let f = setup_event_with_ticket();
    f.client.get_resale_config(&f.event_id);
}

// ── Resale execution & royalty math ────────────────────────────────────────────

#[test]
fn test_resell_splits_proceeds_seller_and_organizer_at_5_percent() {
    // 5% royalty: organizer gets 50, seller gets 950 on a 1_000 sale.
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    let organizer_before = balance(&f.env, &f.token, &f.organizer);
    let seller_before = balance(&f.env, &f.token, &f.seller);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);

    assert_eq!(balance(&f.env, &f.token, &f.buyer), 0);
    assert_eq!(
        balance(&f.env, &f.token, &f.organizer),
        organizer_before + 50
    );
    assert_eq!(balance(&f.env, &f.token, &f.seller), seller_before + 950);

    // Ticket ownership transferred.
    assert_eq!(f.client.get_ticket(&f.ticket_id).holder, f.buyer);
}

#[test]
fn test_zero_royalty_routes_full_proceeds_to_seller() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &0_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);

    assert_eq!(balance(&f.env, &f.token, &f.organizer), 0);
    assert_eq!(balance(&f.env, &f.token, &f.seller), 1_000);
}

#[test]
fn test_full_royalty_routes_full_proceeds_to_organizer() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &10_000_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);

    assert_eq!(balance(&f.env, &f.token, &f.organizer), 1_000);
    assert_eq!(balance(&f.env, &f.token, &f.seller), 0);
}

#[test]
fn test_royalty_floors_to_zero_below_one_unit() {
    // 1% royalty on 50 = 0 (integer division). Seller receives the full sale,
    // organizer gets nothing — a known consequence of bps math on tiny amounts.
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &100_u32);
    fund(&f.env, &f.token, &f.buyer, 50);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &50_i128);

    assert_eq!(balance(&f.env, &f.token, &f.organizer), 0);
    assert_eq!(balance(&f.env, &f.token, &f.seller), 50);
}

// ── Authorization & restriction tests ──────────────────────────────────────────

#[test]
#[should_panic]
fn test_resell_without_config_panics() {
    let f = setup_event_with_ticket();
    fund(&f.env, &f.token, &f.buyer, 1_000);
    // No set_resale_config call → must panic with ResaleNotConfigured.
    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);
}

#[test]
#[should_panic]
fn test_resell_after_check_in_rejected() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    f.client.check_in(&f.organizer, &f.ticket_id);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);
}

#[test]
#[should_panic]
fn test_only_current_holder_can_resell() {
    let f = setup_event_with_ticket();
    let imposter = Address::generate(&f.env);
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    f.client
        .resell_ticket(&imposter, &f.buyer, &f.ticket_id, &1_000_i128);
}

#[test]
#[should_panic]
fn test_resell_to_self_rejected() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    fund(&f.env, &f.token, &f.seller, 1_000);

    f.client
        .resell_ticket(&f.seller, &f.seller, &f.ticket_id, &1_000_i128);
}

#[test]
#[should_panic]
fn test_zero_sale_price_rejected() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &0_i128);
}

#[test]
#[should_panic]
fn test_negative_sale_price_rejected() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &-100_i128);
}

// ── Transfer chain after a resale ──────────────────────────────────────────────

#[test]
fn test_resold_ticket_can_be_transferred_again_by_new_holder() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);

    // Buyer (now holder) can transfer onward — confirms the holder field
    // was updated atomically with the royalty payout.
    let third_party = Address::generate(&f.env);
    f.client
        .transfer_ticket(&f.buyer, &f.ticket_id, &third_party);
    assert_eq!(f.client.get_ticket(&f.ticket_id).holder, third_party);
}

#[test]
#[should_panic]
fn test_old_seller_cannot_transfer_after_resale() {
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);
    fund(&f.env, &f.token, &f.buyer, 1_000);
    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128);

    let third_party = Address::generate(&f.env);
    // Original seller no longer holds the ticket → must panic.
    f.client
        .transfer_ticket(&f.seller, &f.ticket_id, &third_party);
}

// ── Royalty independence from event capacity / tiers ───────────────────────────

#[test]
fn test_resale_royalty_persists_across_multiple_resales() {
    // Each resale independently routes royalty to organizer.
    let f = setup_event_with_ticket();
    f.client
        .set_resale_config(&f.organizer, &f.event_id, &f.token, &500_u32);

    let buyer2 = Address::generate(&f.env);
    let buyer3 = Address::generate(&f.env);
    fund(&f.env, &f.token, &f.buyer, 1_000);
    fund(&f.env, &f.token, &buyer2, 2_000);
    fund(&f.env, &f.token, &buyer3, 4_000);

    f.client
        .resell_ticket(&f.seller, &f.buyer, &f.ticket_id, &1_000_i128); // royalty 50
    f.client
        .resell_ticket(&f.buyer, &buyer2, &f.ticket_id, &2_000_i128); // royalty 100
    f.client
        .resell_ticket(&buyer2, &buyer3, &f.ticket_id, &4_000_i128); // royalty 200

    assert_eq!(balance(&f.env, &f.token, &f.organizer), 50 + 100 + 200);
    assert_eq!(f.client.get_ticket(&f.ticket_id).holder, buyer3);
}
