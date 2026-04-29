#![cfg(test)]

use crate::shade::{Shade, ShadeClient};
use crate::types::InvoiceStatus;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{token, Address, Env, String};

fn setup_test_with_auto_withdrawal() -> (
    Env,
    ShadeClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    // Register Shade contract
    let shade_contract_id = env.register(Shade, ());
    let shade_client = ShadeClient::new(&env, &shade_contract_id);

    // Initialize with admin
    let admin = Address::generate(&env);
    shade_client.initialize(&admin);

    // Create and register token
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone());

    // Add token as accepted
    shade_client.add_accepted_token(&admin, &token.address());

    // Set fee to 500 bps (5%)
    shade_client.set_fee(&admin, &token.address(), &500);

    // Register merchant
    let merchant = Address::generate(&env);
    shade_client.register_merchant(&merchant);

    (env, shade_client, shade_contract_id, admin, merchant, token.address())
}

#[test]
fn test_set_auto_withdrawal_threshold() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, token) =
        setup_test_with_auto_withdrawal();

    let threshold = 10_000i128;

    // Set threshold
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &threshold);

    // Verify threshold was set
    let retrieved_threshold = shade_client.get_auto_withdrawal_threshold(&1u64, &token);
    assert_eq!(retrieved_threshold, Some(threshold));
}

#[test]
fn test_set_auto_withdrawal_threshold_zero_disables() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, token) =
        setup_test_with_auto_withdrawal();

    // Set threshold to 10_000
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &10_000i128);

    // Verify it was set
    let retrieved = shade_client.get_auto_withdrawal_threshold(&1u64, &token);
    assert_eq!(retrieved, Some(10_000i128));

    // Set threshold to 0 (disable)
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &0i128);

    // Verify it's now 0
    let retrieved = shade_client.get_auto_withdrawal_threshold(&1u64, &token);
    assert_eq!(retrieved, Some(0i128));
}

#[test]
fn test_set_auto_withdrawal_recipient() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, _token) =
        setup_test_with_auto_withdrawal();

    let recipient = Address::generate(&env);

    // Set recipient
    shade_client.set_auto_withdrawal_recipient(&merchant, &recipient);

    // Verify recipient was set
    let retrieved_recipient = shade_client.get_auto_withdrawal_recipient(&1u64);
    assert_eq!(retrieved_recipient, Some(recipient));
}

#[test]
fn test_auto_withdrawal_triggered_on_payment() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, token) =
        setup_test_with_auto_withdrawal();

    // Set auto-withdrawal threshold to 5000
    let threshold = 5_000i128;
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &threshold);

    // Set withdrawal recipient
    let recipient = Address::generate(&env);
    shade_client.set_auto_withdrawal_recipient(&merchant, &recipient);

    // Create invoice for 10_000 (will result in ~9500 after 5% fee)
    let invoice_amount = 10_000i128;
    let invoice_id = shade_client.create_invoice(
        &merchant,
        String::from_slice(&env, "Test Invoice"),
        invoice_amount,
        token.clone(),
        None,
    );

    // Payer pays the invoice
    let payer = Address::generate(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&payer, &invoice_amount);

    // Pay invoice - this should trigger auto-withdrawal
    shade_client.pay_invoice(&payer, &invoice_id);

    // Verify invoice is paid
    let invoice = shade_client.get_invoice(&invoice_id);
    assert_eq!(invoice.status, InvoiceStatus::Paid);

    // Verify recipient received funds (should be ~9500 after fee)
    let recipient_balance = token_client.balance(&recipient);
    assert!(recipient_balance > 0, "Recipient should have received funds");
}

#[test]
fn test_auto_withdrawal_not_triggered_below_threshold() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, token) =
        setup_test_with_auto_withdrawal();

    // Set auto-withdrawal threshold to 10_000
    let threshold = 10_000i128;
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &threshold);

    // Set withdrawal recipient
    let recipient = Address::generate(&env);
    shade_client.set_auto_withdrawal_recipient(&merchant, &recipient);

    // Create invoice for 5_000 (below threshold)
    let invoice_amount = 5_000i128;
    let invoice_id = shade_client.create_invoice(
        &merchant,
        String::from_slice(&env, "Small Invoice"),
        invoice_amount,
        token.clone(),
        None,
    );

    // Payer pays the invoice
    let payer = Address::generate(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&payer, &invoice_amount);

    // Pay invoice - should NOT trigger auto-withdrawal
    shade_client.pay_invoice(&payer, &invoice_id);

    // Verify recipient did NOT receive funds
    let recipient_balance = token_client.balance(&recipient);
    assert_eq!(
        recipient_balance, 0,
        "Recipient should not have received funds (below threshold)"
    );
}

#[test]
fn test_auto_withdrawal_uses_merchant_address_as_default_recipient() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, token) =
        setup_test_with_auto_withdrawal();

    // Set auto-withdrawal threshold to 5000 (no explicit recipient set)
    let threshold = 5_000i128;
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &threshold);

    // Create invoice for 10_000
    let invoice_amount = 10_000i128;
    let invoice_id = shade_client.create_invoice(
        &merchant,
        String::from_slice(&env, "Test Invoice"),
        invoice_amount,
        token.clone(),
        None,
    );

    // Payer pays the invoice
    let payer = Address::generate(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&payer, &invoice_amount);

    // Pay invoice - should trigger auto-withdrawal to merchant address
    shade_client.pay_invoice(&payer, &invoice_id);

    // Verify merchant received funds
    let merchant_balance = token_client.balance(&merchant);
    assert!(
        merchant_balance > 0,
        "Merchant should have received funds as default recipient"
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #")]
fn test_set_auto_withdrawal_threshold_negative_amount_fails() {
    let (env, shade_client, _shade_contract_id, _admin, merchant, token) =
        setup_test_with_auto_withdrawal();

    // Try to set negative threshold - should panic
    shade_client.set_auto_withdrawal_threshold(&merchant, &token, &-1000i128);
}

#[test]
fn test_auto_withdrawal_threshold_per_token() {
    let (env, shade_client, _shade_contract_id, admin, merchant, token1) =
        setup_test_with_auto_withdrawal();

    // Create second token
    let token_admin = Address::generate(&env);
    let token2 = env.register_stellar_asset_contract_v2(token_admin);
    shade_client.add_accepted_token(&admin, &token2.address());

    // Set different thresholds for each token
    let threshold1 = 5_000i128;
    let threshold2 = 10_000i128;

    shade_client.set_auto_withdrawal_threshold(&merchant, &token1, &threshold1);
    shade_client.set_auto_withdrawal_threshold(&merchant, &token2.address(), &threshold2);

    // Verify both thresholds are set correctly
    let retrieved1 = shade_client.get_auto_withdrawal_threshold(&1u64, &token1);
    let retrieved2 = shade_client.get_auto_withdrawal_threshold(&1u64, &token2.address());

    assert_eq!(retrieved1, Some(threshold1));
    assert_eq!(retrieved2, Some(threshold2));
}
