#![cfg(test)]

use crate::shade::{Shade, ShadeClient};
use crate::types::{InvoiceFilter, InvoiceStatus};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env, String};

fn setup_test() -> (Env, ShadeClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);
    client.initialize(&admin, &wasm_hash);
    (env, client, contract_id, admin)
}

fn create_token(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

/// Creates three invoices at timestamps 1000, 2000, and 3000.
/// Returns (client, merchant, token, [id1, id2, id3]).
fn setup_invoices_at_timestamps(
    env: &Env,
    client: &ShadeClient<'_>,
    admin: &Address,
) -> (Address, Address, [u64; 3]) {
    let merchant = Address::generate(env);
    client.register_merchant(&merchant);

    let token = create_token(env);
    client.add_accepted_token(admin, &token);

    env.ledger().set_timestamp(1_000);
    let id1 = client.create_invoice(
        &merchant,
        &String::from_str(env, "Invoice 1"),
        &100,
        &token,
        &None,
    );

    env.ledger().set_timestamp(2_000);
    let id2 = client.create_invoice(
        &merchant,
        &String::from_str(env, "Invoice 2"),
        &200,
        &token,
        &None,
    );

    env.ledger().set_timestamp(3_000);
    let id3 = client.create_invoice(
        &merchant,
        &String::from_str(env, "Invoice 3"),
        &300,
        &token,
        &None,
    );

    (merchant, token, [id1, id2, id3])
}

#[test]
fn test_filter_no_dates_returns_all() {
    let (env, client, _contract_id, admin) = setup_test();
    let (_merchant, _token, ids) = setup_invoices_at_timestamps(&env, &client, &admin);

    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: None,
        end_date: None,
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 3);
    assert_eq!(result.get(0).unwrap().id, ids[0]);
    assert_eq!(result.get(1).unwrap().id, ids[1]);
    assert_eq!(result.get(2).unwrap().id, ids[2]);
}

#[test]
fn test_filter_by_start_date() {
    let (env, client, _contract_id, admin) = setup_test();
    setup_invoices_at_timestamps(&env, &client, &admin);

    // Only invoices created at or after timestamp 2000 should be returned
    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(2_000),
        end_date: None,
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().date_created, 2_000);
    assert_eq!(result.get(1).unwrap().date_created, 3_000);
}

#[test]
fn test_filter_by_end_date() {
    let (env, client, _contract_id, admin) = setup_test();
    setup_invoices_at_timestamps(&env, &client, &admin);

    // Only invoices created at or before timestamp 2000 should be returned
    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: None,
        end_date: Some(2_000),
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().date_created, 1_000);
    assert_eq!(result.get(1).unwrap().date_created, 2_000);
}

#[test]
fn test_filter_by_date_range() {
    let (env, client, _contract_id, admin) = setup_test();
    setup_invoices_at_timestamps(&env, &client, &admin);

    // Only the invoice at timestamp 2000 falls within [2000, 2000]
    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(2_000),
        end_date: Some(2_000),
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0).unwrap().date_created, 2_000);
}

#[test]
fn test_filter_exact_boundary_dates() {
    let (env, client, _contract_id, admin) = setup_test();
    setup_invoices_at_timestamps(&env, &client, &admin);

    // start_date == timestamp of first invoice: all three should match
    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(1_000),
        end_date: Some(3_000),
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 3);

    // start_date one past the last invoice: none should match
    let filter_empty = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(3_001),
        end_date: None,
    };

    let result_empty = client.get_invoices(&filter_empty);
    assert_eq!(result_empty.len(), 0);
}

#[test]
fn test_filter_date_range_no_matches() {
    let (env, client, _contract_id, admin) = setup_test();
    setup_invoices_at_timestamps(&env, &client, &admin);

    // Range that doesn't overlap with any invoice timestamps
    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(4_000),
        end_date: Some(5_000),
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_filter_date_combined_with_status() {
    let (env, client, _contract_id, admin) = setup_test();
    let (_merchant, _token, _ids) = setup_invoices_at_timestamps(&env, &client, &admin);

    // All invoices are Pending (status 0); filter by date range + Pending status
    let filter = InvoiceFilter {
        status: Some(InvoiceStatus::Pending as u32),
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(1_000),
        end_date: Some(2_000),
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 2);

    // Filter by date range + a non-existent status should return nothing
    let filter_paid = InvoiceFilter {
        status: Some(InvoiceStatus::Paid as u32),
        merchant: None,
        min_amount: None,
        max_amount: None,
        start_date: Some(1_000),
        end_date: Some(3_000),
    };

    let result_paid = client.get_invoices(&filter_paid);
    assert_eq!(result_paid.len(), 0);
}

#[test]
fn test_filter_date_combined_with_amount_range() {
    let (env, client, _contract_id, admin) = setup_test();
    setup_invoices_at_timestamps(&env, &client, &admin);

    // Invoices in date range [1000, 2000] with amount >= 200 → only invoice 2 (amount=200)
    let filter = InvoiceFilter {
        status: None,
        merchant: None,
        min_amount: Some(200),
        max_amount: None,
        start_date: Some(1_000),
        end_date: Some(2_000),
    };

    let result = client.get_invoices(&filter);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0).unwrap().amount, 200);
    assert_eq!(result.get(0).unwrap().date_created, 2_000);
}
