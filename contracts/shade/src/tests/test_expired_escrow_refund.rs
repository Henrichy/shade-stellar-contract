#![cfg(test)]

use crate::shade::{Shade, ShadeClient};
use crate::types::InvoiceStatus;
use account::account::{MerchantAccount, MerchantAccountClient};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, Env, String};

/// Set up a paid invoice with an expiration timestamp.
/// Returns (env, client, buyer/payer, invoice_id, token, merchant_account_id).
fn setup_paid_invoice_with_expiry(
    pay_ts: u64,
    expires_at: u64,
) -> (Env, ShadeClient<'static>, Address, u64, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let shade_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &shade_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Token with 0 fee so full amount lands in merchant account
    let token_contract = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = token_contract.address();
    client.add_accepted_token(&admin, &token);
    client.set_fee(&admin, &token, &0);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let merchant_account_id = env.register(MerchantAccount, ());
    let ma_client = MerchantAccountClient::new(&env, &merchant_account_id);
    ma_client.initialize(&merchant, &shade_id, &1_u64);
    client.set_merchant_account(&merchant, &merchant_account_id);

    let amount = 1_000_i128;
    let description = String::from_str(&env, "Escrow Invoice");
    let invoice_id = client.create_invoice(
        &merchant,
        &description,
        &amount,
        &token,
        &Some(expires_at),
    );

    let buyer = Address::generate(&env);
    let token_mint = token::StellarAssetClient::new(&env, &token);
    token_mint.mint(&buyer, &amount);

    env.ledger().set_timestamp(pay_ts);
    client.pay_invoice(&buyer, &invoice_id);

    (env, client, buyer, invoice_id, token, merchant_account_id)
}

// ---------------------------------------------------------------------------
// Test 1: Buyer claims refund after expiry — funds revert to buyer
// ---------------------------------------------------------------------------
#[test]
fn test_claim_refund_after_expiry_succeeds() {
    // Invoice expires at 5000, paid at 1000
    let (env, client, buyer, invoice_id, token, merchant_account_id) =
        setup_paid_invoice_with_expiry(1_000, 5_000);

    // Advance past expiry
    env.ledger().set_timestamp(6_000);
    client.claim_refund(&buyer, &invoice_id);

    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.status, InvoiceStatus::Refunded);
    assert_eq!(invoice.amount_refunded, 1_000);

    let tok = token::TokenClient::new(&env, &token);
    assert_eq!(tok.balance(&buyer), 1_000);
    assert_eq!(tok.balance(&merchant_account_id), 0);
}

// ---------------------------------------------------------------------------
// Test 2: claim_refund fails before expiry — EscrowNotExpired (#44)
// ---------------------------------------------------------------------------
#[test]
#[should_panic(expected = "HostError: Error(Contract, #44)")]
fn test_claim_refund_before_expiry_fails() {
    let (env, client, buyer, invoice_id, _token, _ma) =
        setup_paid_invoice_with_expiry(1_000, 5_000);

    // Still before expiry
    env.ledger().set_timestamp(3_000);
    client.claim_refund(&buyer, &invoice_id);
}

// ---------------------------------------------------------------------------
// Test 3: Non-buyer cannot claim refund — NotAuthorized (#1)
// ---------------------------------------------------------------------------
#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_claim_refund_non_buyer_fails() {
    let (env, client, _buyer, invoice_id, _token, _ma) =
        setup_paid_invoice_with_expiry(1_000, 5_000);

    env.ledger().set_timestamp(6_000);
    let random = Address::generate(&env);
    client.claim_refund(&random, &invoice_id);
}

// ---------------------------------------------------------------------------
// Test 4: Double claim fails — EscrowAlreadyRefunded (#45)
// ---------------------------------------------------------------------------
#[test]
#[should_panic(expected = "HostError: Error(Contract, #45)")]
fn test_claim_refund_double_claim_fails() {
    let (env, client, buyer, invoice_id, _token, _ma) =
        setup_paid_invoice_with_expiry(1_000, 5_000);

    env.ledger().set_timestamp(6_000);
    client.claim_refund(&buyer, &invoice_id);
    // Second claim should fail
    client.claim_refund(&buyer, &invoice_id);
}

// ---------------------------------------------------------------------------
// Test 5: Invoice without expiry cannot be claimed — InvoiceExpired (#27)
// ---------------------------------------------------------------------------
#[test]
#[should_panic(expected = "HostError: Error(Contract, #27)")]
fn test_claim_refund_no_expiry_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let shade_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &shade_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_contract = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = token_contract.address();
    client.add_accepted_token(&admin, &token);
    client.set_fee(&admin, &token, &0);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let merchant_account_id = env.register(MerchantAccount, ());
    let ma_client = MerchantAccountClient::new(&env, &merchant_account_id);
    ma_client.initialize(&merchant, &shade_id, &1_u64);
    client.set_merchant_account(&merchant, &merchant_account_id);

    let amount = 500_i128;
    let description = String::from_str(&env, "No Expiry Invoice");
    let invoice_id = client.create_invoice(&merchant, &description, &amount, &token, &None);

    let buyer = Address::generate(&env);
    let token_mint = token::StellarAssetClient::new(&env, &token);
    token_mint.mint(&buyer, &amount);

    env.ledger().set_timestamp(1_000);
    client.pay_invoice(&buyer, &invoice_id);

    env.ledger().set_timestamp(9_999_999);
    // No expires_at set — should panic with InvoiceExpired
    client.claim_refund(&buyer, &invoice_id);
}

// ---------------------------------------------------------------------------
// Test 6: Unpaid invoice cannot be claimed — InvalidInvoiceStatus (#16)
// ---------------------------------------------------------------------------
#[test]
#[should_panic(expected = "HostError: Error(Contract, #16)")]
fn test_claim_refund_unpaid_invoice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let shade_id = env.register(Shade, ());
    let client = ShadeClient::new(&env, &shade_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_contract = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = token_contract.address();
    client.add_accepted_token(&admin, &token);
    client.set_fee(&admin, &token, &0);

    let merchant = Address::generate(&env);
    client.register_merchant(&merchant);

    let merchant_account_id = env.register(MerchantAccount, ());
    let ma_client = MerchantAccountClient::new(&env, &merchant_account_id);
    ma_client.initialize(&merchant, &shade_id, &1_u64);
    client.set_merchant_account(&merchant, &merchant_account_id);

    let description = String::from_str(&env, "Unpaid Invoice");
    let invoice_id = client.create_invoice(&merchant, &description, &500, &token, &Some(5_000));

    let buyer = Address::generate(&env);
    // Never paid — advance past expiry and try to claim
    env.ledger().set_timestamp(6_000);
    client.claim_refund(&buyer, &invoice_id);
}
