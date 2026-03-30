#![cfg(test)]

//! Comprehensive tests for the `amend_invoice` function.
//!
//! Rules under test:
//! - Only the owning merchant can amend their invoice.
//! - Only `Pending` invoices can be amended.
//! - Amount must be positive when provided.
//! - Description length must not exceed 100 characters.
//! - Either field can be updated independently or together.

use crate::shade::{Shade, ShadeClient};
use crate::types::{DataKey, InvoiceStatus};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_env() -> (Env, ShadeClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, contract_id, admin)
}

fn make_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone())
        .address()
}

/// Register a merchant, add a token, create a Pending invoice, return (merchant, token, invoice_id).
fn create_pending_invoice(
    env: &Env,
    client: &ShadeClient<'_>,
    admin: &Address,
    amount: i128,
    description: &str,
) -> (Address, Address, u64) {
    let merchant = Address::generate(env);
    client.register_merchant(&merchant);
    let token = make_token(env, admin);
    client.add_accepted_token(admin, &token);
    let id = client.create_invoice(
        &merchant,
        &String::from_str(env, description),
        &amount,
        &token,
        &None,
    );
    (merchant, token, id)
}

/// Force an invoice into a given status by writing directly to storage.
fn force_invoice_status(env: &Env, contract_id: &Address, invoice_id: u64, status: InvoiceStatus) {
    env.as_contract(contract_id, || {
        let mut invoice: crate::types::Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(invoice_id))
            .unwrap();
        invoice.status = status;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id), &invoice);
    });
}

// ---------------------------------------------------------------------------
// Success cases
// ---------------------------------------------------------------------------

#[test]
fn test_amend_amount_on_pending_invoice() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Original");

    client.amend_invoice(&merchant, &id, &Some(999), &None);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.amount, 999);
    assert_eq!(invoice.description, String::from_str(&env, "Original"));
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

#[test]
fn test_amend_description_on_pending_invoice() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Original");

    client.amend_invoice(
        &merchant,
        &id,
        &None,
        &Some(String::from_str(&env, "Updated description")),
    );

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.amount, 500);
    assert_eq!(
        invoice.description,
        String::from_str(&env, "Updated description")
    );
}

#[test]
fn test_amend_both_fields_on_pending_invoice() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Original");

    client.amend_invoice(
        &merchant,
        &id,
        &Some(1200),
        &Some(String::from_str(&env, "New description")),
    );

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.amount, 1200);
    assert_eq!(
        invoice.description,
        String::from_str(&env, "New description")
    );
}

#[test]
fn test_amend_with_none_fields_is_noop() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Original");

    client.amend_invoice(&merchant, &id, &None, &None);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.amount, 500);
    assert_eq!(invoice.description, String::from_str(&env, "Original"));
}

#[test]
fn test_amend_invoice_multiple_times() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "v1");

    client.amend_invoice(&merchant, &id, &Some(600), &None);
    client.amend_invoice(
        &merchant,
        &id,
        &Some(700),
        &Some(String::from_str(&env, "v3")),
    );

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.amount, 700);
    assert_eq!(invoice.description, String::from_str(&env, "v3"));
}

// ---------------------------------------------------------------------------
// Failure: wrong status
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_paid_invoice_fails() {
    let (env, client, contract_id, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    force_invoice_status(&env, &contract_id, id, InvoiceStatus::Paid);

    client.amend_invoice(&merchant, &id, &Some(999), &None);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_cancelled_invoice_fails() {
    let (env, client, contract_id, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    force_invoice_status(&env, &contract_id, id, InvoiceStatus::Cancelled);

    client.amend_invoice(&merchant, &id, &Some(999), &None);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_refunded_invoice_fails() {
    let (env, client, contract_id, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    force_invoice_status(&env, &contract_id, id, InvoiceStatus::Refunded);

    client.amend_invoice(&merchant, &id, &Some(999), &None);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_partially_paid_invoice_fails() {
    let (env, client, contract_id, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    force_invoice_status(&env, &contract_id, id, InvoiceStatus::PartiallyPaid);

    client.amend_invoice(&merchant, &id, &Some(999), &None);
}

// ---------------------------------------------------------------------------
// Failure: wrong merchant
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_invoice_by_wrong_merchant_fails() {
    let (env, client, _, admin) = setup_env();
    let (_, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    // Register a second merchant and try to amend the first merchant's invoice
    let other_merchant = Address::generate(&env);
    client.register_merchant(&other_merchant);

    client.amend_invoice(&other_merchant, &id, &Some(999), &None);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_invoice_by_unregistered_address_fails() {
    let (env, client, _, admin) = setup_env();
    let (_, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    let random = Address::generate(&env);
    client.amend_invoice(&random, &id, &Some(999), &None);
}

// ---------------------------------------------------------------------------
// Failure: invalid amount
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_invoice_with_zero_amount_fails() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    client.amend_invoice(&merchant, &id, &Some(0), &None);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_amend_invoice_with_negative_amount_fails() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    client.amend_invoice(&merchant, &id, &Some(-100), &None);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_amend_invoice_amount_to_minimum_valid() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    // Minimum valid amount is 1 (above fee of 0)
    client.amend_invoice(&merchant, &id, &Some(1), &None);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.amount, 1);
}

#[test]
fn test_amend_invoice_preserves_other_fields() {
    let (env, client, _, admin) = setup_env();
    let (merchant, token, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    client.amend_invoice(&merchant, &id, &Some(800), &None);

    let invoice = client.get_invoice(&id);
    // Token, merchant_id, status, payer should be unchanged
    assert_eq!(invoice.token, token);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert!(invoice.payer.is_none());
}

#[test]
fn test_amend_description_exactly_100_chars_succeeds() {
    let (env, client, _, admin) = setup_env();
    let (merchant, _, id) = create_pending_invoice(&env, &client, &admin, 500, "Invoice");

    // Exactly 100 characters
    let desc_100 = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    client.amend_invoice(&merchant, &id, &None, &Some(desc_100.clone()));

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.description, desc_100);
}
