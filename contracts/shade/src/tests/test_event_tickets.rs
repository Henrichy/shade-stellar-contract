#![cfg(test)]

use crate::shade::{Shade, ShadeClient};
use soroban_sdk::testutils::{Address as _, Ledger as _, MockAuth, MockAuthInvoke};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, IntoVal, String};

const TOKEN_INITIAL_BALANCE: i128 = 1_000_000;

struct Fixture<'a> {
    env: Env,
    client: ShadeClient<'a>,
    admin: Address,
    token: Address,
}

fn setup() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    client.add_accepted_token(&admin, &token_address);

    Fixture {
        env,
        client,
        admin,
        token: token_address,
    }
}

fn fund(env: &Env, token: &Address, to: &Address, amount: i128) {
    let asset_client = StellarAssetClient::new(env, token);
    let issuer = asset_client.admin();
    asset_client
        .mock_auths(&[MockAuth {
            address: &issuer,
            invoke: &MockAuthInvoke {
                contract: token,
                fn_name: "mint",
                args: (to.clone(), amount).into_val(env),
                sub_invokes: &[],
            },
        }])
        .mint(to, &amount);
}

fn register_merchant_with_account(
    env: &Env,
    client: &ShadeClient,
    token: &Address,
) -> (Address, Address) {
    let merchant = Address::generate(env);
    let merchant_account = merchant.clone();
    client.register_merchant(&merchant);
    client.set_merchant_account(&merchant, &merchant_account);
    client.set_merchant_accepted_tokens(
        &merchant,
        &soroban_sdk::Vec::from_array(env, [token.clone()]),
    );
    (merchant, merchant_account)
}

fn future_date(env: &Env) -> u64 {
    env.ledger().timestamp() + 86_400
}

// ── #246 Event creation ───────────────────────────────────────────────────────

#[test]
fn create_event_stores_all_fields() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let event_date = future_date(&f.env);
    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Concert"),
        &500i128,
        &f.token,
        &10u32,
        &event_date,
        &500u32, // 5% royalty
    );

    let event = f.client.get_event(&event_id);
    assert_eq!(event.id, event_id);
    assert_eq!(event.name, String::from_str(&f.env, "Concert"));
    assert_eq!(event.ticket_price, 500);
    assert_eq!(event.token, f.token);
    assert_eq!(event.capacity, 10);
    assert_eq!(event.sold, 0);
    assert_eq!(event.event_date, event_date);
    assert_eq!(event.royalty_bps, 500);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // InvalidAmount
fn create_event_rejects_zero_price() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);
    f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "X"),
        &0i128,
        &f.token,
        &10u32,
        &future_date(&f.env),
        &0u32,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #48)")] // InvalidCapacity
fn create_event_rejects_zero_capacity() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);
    f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "X"),
        &100i128,
        &f.token,
        &0u32,
        &future_date(&f.env),
        &0u32,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #50)")] // InvalidRoyaltyBps
fn create_event_rejects_royalty_above_100pct() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);
    f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "X"),
        &100i128,
        &f.token,
        &10u32,
        &future_date(&f.env),
        &10_001u32,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #49)")] // InvalidEventDate
fn create_event_rejects_past_date() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);
    // Move ledger forward so 0 is firmly in the past.
    f.env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "X"),
        &100i128,
        &f.token,
        &10u32,
        &500u64,
        &0u32,
    );
}

// ── #247 + #248 Payment + minting ────────────────────────────────────────────

#[test]
fn purchase_ticket_transfers_funds_and_mints() {
    let f = setup();
    let (merchant, merchant_account) =
        register_merchant_with_account(&f.env, &f.client, &f.token);
    let buyer = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer, TOKEN_INITIAL_BALANCE);

    let price: i128 = 500;
    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Show"),
        &price,
        &f.token,
        &5u32,
        &future_date(&f.env),
        &500u32,
    );

    let ticket_id = f.client.purchase_ticket(&event_id, &buyer);

    // Ticket exists and is owned by buyer.
    let ticket = f.client.get_ticket(&ticket_id);
    assert_eq!(ticket.event_id, event_id);
    assert_eq!(ticket.owner, buyer);

    // Buyer's tickets list contains the new ticket.
    let user_tickets = f.client.get_user_tickets(&buyer);
    assert_eq!(user_tickets.len(), 1);
    assert_eq!(user_tickets.get_unchecked(0), ticket_id);

    // Sold counter incremented.
    let event = f.client.get_event(&event_id);
    assert_eq!(event.sold, 1);

    // Funds moved off the buyer.
    let token_client = TokenClient::new(&f.env, &f.token);
    assert_eq!(token_client.balance(&buyer), TOKEN_INITIAL_BALANCE - price);

    // Merchant account received `price - fee`. With no fee configured fee is 0,
    // so the merchant receives the full price.
    assert_eq!(token_client.balance(&merchant_account), price);
}

#[test]
fn purchase_ticket_routes_fee_to_platform_when_configured() {
    let f = setup();
    let (merchant, merchant_account) =
        register_merchant_with_account(&f.env, &f.client, &f.token);
    // 10% platform fee on this token.
    f.client.set_fee(&f.admin, &f.token, &1_000i128);

    let buyer = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer, TOKEN_INITIAL_BALANCE);

    let price: i128 = 1_000;
    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Show"),
        &price,
        &f.token,
        &5u32,
        &future_date(&f.env),
        &0u32,
    );

    f.client.purchase_ticket(&event_id, &buyer);

    let token_client = TokenClient::new(&f.env, &f.token);
    let platform = f.client.get_platform_account();
    let expected_fee = price / 10; // 10% in bps == 1000
    assert_eq!(token_client.balance(&merchant_account), price - expected_fee);
    assert_eq!(token_client.balance(&platform), expected_fee);
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")] // EventSoldOut
fn purchase_ticket_panics_when_sold_out() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let buyer1 = Address::generate(&f.env);
    let buyer2 = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer1, TOKEN_INITIAL_BALANCE);
    fund(&f.env, &f.token, &buyer2, TOKEN_INITIAL_BALANCE);

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Tiny"),
        &100i128,
        &f.token,
        &1u32,
        &future_date(&f.env),
        &0u32,
    );
    f.client.purchase_ticket(&event_id, &buyer1);
    f.client.purchase_ticket(&event_id, &buyer2);
}

#[test]
#[should_panic(expected = "Error(Contract, #46)")] // EventNotFound
fn purchase_ticket_panics_when_event_missing() {
    let f = setup();
    let buyer = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer, TOKEN_INITIAL_BALANCE);
    f.client.purchase_ticket(&999u64, &buyer);
}

// ── #254 Resale royalty split ────────────────────────────────────────────────

#[test]
fn resale_splits_royalty_and_proceeds() {
    let f = setup();
    let (merchant, merchant_account) =
        register_merchant_with_account(&f.env, &f.client, &f.token);

    let buyer1 = Address::generate(&f.env);
    let buyer2 = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer1, TOKEN_INITIAL_BALANCE);
    fund(&f.env, &f.token, &buyer2, TOKEN_INITIAL_BALANCE);

    let primary_price: i128 = 1_000;
    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Show"),
        &primary_price,
        &f.token,
        &10u32,
        &future_date(&f.env),
        &1_000u32, // 10% royalty
    );

    let ticket_id = f.client.purchase_ticket(&event_id, &buyer1);

    let token_client = TokenClient::new(&f.env, &f.token);
    let merchant_balance_before_resale = token_client.balance(&merchant_account);
    let buyer1_balance_before_resale = token_client.balance(&buyer1);
    let buyer2_balance_before_resale = token_client.balance(&buyer2);

    let resale_price: i128 = 2_000;
    f.client
        .resell_ticket(&buyer1, &buyer2, &ticket_id, &resale_price);

    let expected_royalty = resale_price / 10; // 10%
    let expected_seller_proceeds = resale_price - expected_royalty;

    // Royalty went to the merchant account.
    assert_eq!(
        token_client.balance(&merchant_account),
        merchant_balance_before_resale + expected_royalty
    );
    // Original buyer (seller) got the remainder.
    assert_eq!(
        token_client.balance(&buyer1),
        buyer1_balance_before_resale + expected_seller_proceeds
    );
    // Reseller paid the full resale price.
    assert_eq!(
        token_client.balance(&buyer2),
        buyer2_balance_before_resale - resale_price
    );

    // Ownership transferred.
    let ticket = f.client.get_ticket(&ticket_id);
    assert_eq!(ticket.owner, buyer2);

    // User-ticket index updated for both parties.
    assert_eq!(f.client.get_user_tickets(&buyer1).len(), 0);
    let buyer2_tickets = f.client.get_user_tickets(&buyer2);
    assert_eq!(buyer2_tickets.len(), 1);
    assert_eq!(buyer2_tickets.get_unchecked(0), ticket_id);
}

#[test]
fn resale_with_zero_royalty_pays_seller_in_full() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let buyer1 = Address::generate(&f.env);
    let buyer2 = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer1, TOKEN_INITIAL_BALANCE);
    fund(&f.env, &f.token, &buyer2, TOKEN_INITIAL_BALANCE);

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "NoRoyalty"),
        &500i128,
        &f.token,
        &10u32,
        &future_date(&f.env),
        &0u32,
    );

    let ticket_id = f.client.purchase_ticket(&event_id, &buyer1);

    let token_client = TokenClient::new(&f.env, &f.token);
    let buyer1_before = token_client.balance(&buyer1);
    let resale_price: i128 = 750;
    f.client
        .resell_ticket(&buyer1, &buyer2, &ticket_id, &resale_price);
    assert_eq!(token_client.balance(&buyer1), buyer1_before + resale_price);
}

#[test]
#[should_panic(expected = "Error(Contract, #52)")] // NotTicketOwner
fn resale_rejects_non_owner_seller() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let buyer1 = Address::generate(&f.env);
    let imposter = Address::generate(&f.env);
    let buyer2 = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer1, TOKEN_INITIAL_BALANCE);
    fund(&f.env, &f.token, &buyer2, TOKEN_INITIAL_BALANCE);

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "X"),
        &100i128,
        &f.token,
        &5u32,
        &future_date(&f.env),
        &500u32,
    );
    let ticket_id = f.client.purchase_ticket(&event_id, &buyer1);

    f.client
        .resell_ticket(&imposter, &buyer2, &ticket_id, &200i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #54)")] // InvalidResalePrice
fn resale_rejects_zero_price() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let buyer1 = Address::generate(&f.env);
    let buyer2 = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer1, TOKEN_INITIAL_BALANCE);

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "X"),
        &100i128,
        &f.token,
        &5u32,
        &future_date(&f.env),
        &500u32,
    );
    let ticket_id = f.client.purchase_ticket(&event_id, &buyer1);
    f.client.resell_ticket(&buyer1, &buyer2, &ticket_id, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #51)")] // TicketNotFound
fn resale_rejects_unknown_ticket() {
    let f = setup();
    let (_merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);
    let a = Address::generate(&f.env);
    let b = Address::generate(&f.env);
    f.client.resell_ticket(&a, &b, &999u64, &100i128);
}

#[test]
fn dynamic_pricing_adjusts_over_time() {
    let f = setup();
    let (merchant, merchant_account) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let now = f.env.ledger().timestamp();
    let event_date = now + 10_000;
    let early_bird_end = now + 1_000;
    let base_price: i128 = 1_000;

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Dynamic Price Show"),
        &base_price,
        &f.token,
        &10u32,
        &event_date,
        &0u32,
    );

    f.client
        .configure_dynamic_pricing(&merchant, &event_id, &early_bird_end, &2_000u32, &5_000u32);

    let buyer_early = Address::generate(&f.env);
    let buyer_late = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer_early, TOKEN_INITIAL_BALANCE);
    fund(&f.env, &f.token, &buyer_late, TOKEN_INITIAL_BALANCE);

    // During early bird: 20% discount => 800
    assert_eq!(f.client.get_current_ticket_price(&event_id), 800i128);
    f.client.purchase_ticket(&event_id, &buyer_early);

    // Move ledger after early bird window: 50% markup => 1500
    f.env
        .ledger()
        .with_mut(|l| l.timestamp = early_bird_end + 1);
    assert_eq!(f.client.get_current_ticket_price(&event_id), 1_500i128);
    f.client.purchase_ticket(&event_id, &buyer_late);

    let token_client = TokenClient::new(&f.env, &f.token);
    assert_eq!(token_client.balance(&merchant_account), 2_300i128);
}

#[test]
fn cancel_event_executes_batch_refunds() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);

    let buyer1 = Address::generate(&f.env);
    let buyer2 = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer1, TOKEN_INITIAL_BALANCE);
    fund(&f.env, &f.token, &buyer2, TOKEN_INITIAL_BALANCE);

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Cancelable Show"),
        &500i128,
        &f.token,
        &5u32,
        &future_date(&f.env),
        &0u32,
    );

    f.client.purchase_ticket(&event_id, &buyer1);
    f.client.purchase_ticket(&event_id, &buyer2);

    let token_client = TokenClient::new(&f.env, &f.token);
    assert_eq!(token_client.balance(&buyer1), TOKEN_INITIAL_BALANCE - 500);
    assert_eq!(token_client.balance(&buyer2), TOKEN_INITIAL_BALANCE - 500);

    f.client.cancel_event_and_batch_refund(&merchant, &event_id);

    assert_eq!(token_client.balance(&buyer1), TOKEN_INITIAL_BALANCE);
    assert_eq!(token_client.balance(&buyer2), TOKEN_INITIAL_BALANCE);

    let event = f.client.get_event(&event_id);
    assert!(event.cancelled);
    assert!(event.refunds_processed);
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")] // InvalidInvoiceStatus
fn cancel_event_cannot_refund_twice() {
    let f = setup();
    let (merchant, _) = register_merchant_with_account(&f.env, &f.client, &f.token);
    let buyer = Address::generate(&f.env);
    fund(&f.env, &f.token, &buyer, TOKEN_INITIAL_BALANCE);

    let event_id = f.client.create_event(
        &merchant,
        &String::from_str(&f.env, "Single Refund"),
        &500i128,
        &f.token,
        &5u32,
        &future_date(&f.env),
        &0u32,
    );

    f.client.purchase_ticket(&event_id, &buyer);
    f.client.cancel_event_and_batch_refund(&merchant, &event_id);
    f.client.cancel_event_and_batch_refund(&merchant, &event_id);
}
