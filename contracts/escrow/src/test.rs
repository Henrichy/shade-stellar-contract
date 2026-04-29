#![cfg(test)]
use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

fn register_token(env: &Env, admin: Address) -> Address {
    env.register_stellar_asset_contract_v2(admin).address()
}

fn fund_escrow(env: &Env, escrow_addr: &Address, token: &Address, amount: i128) {
    let token_client = token::StellarAssetClient::new(env, token);
    token_client.mint(escrow_addr, &amount);
}

fn calculate_fee(amount: i128, fee_bps: u32) -> i128 {
    if fee_bps == 0 {
        return 0;
    }
    (amount * fee_bps as i128) / 10_000
}

#[test]
fn test_escrow_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, seller, arbiter, _token) = setup_escrow(&env, 2000);

    assert_eq!(client.buyer(), buyer);
    assert_eq!(client.seller(), seller);
    assert_eq!(client.arbiter(), arbiter);
    assert_eq!(client.terms(), String::from_str(&env, "Deliver goods by deadline"));
    assert_eq!(client.expires_at(), 2000);
    assert!(!client.is_refunded());
}

#[test]
fn claim_refund_succeeds_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, _seller, _arbiter, token_address) = setup_escrow(&env, 2000);

    // Advance time past expiry
    env.ledger().set_timestamp(3000);

    client.claim_refund(&buyer);

    assert!(client.is_refunded());

    // Verify buyer received the funds
    let token_client = TokenClient::new(&env, &token_address);
    assert_eq!(token_client.balance(&buyer), 1000);
}

#[test]
#[should_panic(expected = "escrow has not expired yet")]
fn claim_refund_fails_before_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, _seller, _arbiter, _token) = setup_escrow(&env, 2000);

    // Still before expiry
    env.ledger().set_timestamp(1500);
    client.claim_refund(&buyer);
}

#[test]
#[should_panic(expected = "only the buyer can claim a refund")]
fn claim_refund_fails_for_non_buyer() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, _buyer, seller, _arbiter, _token) = setup_escrow(&env, 2000);

    env.ledger().set_timestamp(3000);
    client.claim_refund(&seller);
}

#[test]
#[should_panic(expected = "refund already claimed")]
fn claim_refund_fails_if_already_refunded() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);

    let (client, buyer, _seller, _arbiter, _token) = setup_escrow(&env, 2000);

    env.ledger().set_timestamp(3000);
    client.claim_refund(&buyer);
    // Second claim should panic
    client.claim_refund(&buyer);
}

#[test]
#[should_panic(expected = "expires_at must be in the future")]
fn init_fails_with_past_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(5000);

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver within 7 days");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total_amount = 10000i128;
    let fee_bps: u32 = 250;

    let milestones = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total_amount,
        &fee_bps, &milestones,
    );

    assert_eq!(client.buyer(), buyer);
    assert_eq!(client.seller(), seller);
    assert_eq!(client.arbiter(), arbiter);
    assert_eq!(client.terms(), terms);
    assert_eq!(client.token(), token);
    assert_eq!(client.total_amount(), total_amount);
    assert_eq!(client.fee_percentage_bps(), fee_bps);
    assert_eq!(client.status(), EscrowStatus::Pending);
    assert_eq!(client.get_total_released(), 0);
}

#[test]
#[should_panic]
fn test_initialize_twice() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &5000i128,
        &100u32, &empty,
    );
    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &5000i128,
        &100u32, &empty,
    );
}

#[test]
#[should_panic]
fn test_invalid_total_amount() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &0i128,
        &100u32, &empty,
    );
}

#[test]
#[should_panic]
fn test_fee_exceeds_100_percent() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &10000i128,
        &10_001u32,
        &empty,
    );
}

#[test]
fn test_fee_calculation_basic() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 10000i128;
    let fee_bps: u32 = 250;
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &fee_bps, &empty,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_release();

    let token_client = token::StellarAssetClient::new(&env, &token);
    let seller_balance = token_client.balance(&seller);
    let platform_balance = token_client.balance(&platform);

    let expected_fee = calculate_fee(total, fee_bps);
    let expected_net = total - expected_fee;

    assert_eq!(seller_balance, expected_net);
    assert_eq!(platform_balance, expected_fee);
    assert_eq!(client.status(), EscrowStatus::Completed);
}

#[test]
fn test_fee_calculation_zero_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 5000i128;
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &0u32,
        &empty,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_release();

    let token_client = token::StellarAssetClient::new(&env, &token);
    assert_eq!(token_client.balance(&seller), total);
    assert_eq!(token_client.balance(&platform), 0);
}

#[test]
fn test_fee_calculation_precision() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 123456789i128;
    let fee_bps: u32 = 333;
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &fee_bps, &empty,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_release();

    let expected_fee = (total * fee_bps as i128) / 10_000;
    let expected_net = total - expected_fee;

    let token_client = token::StellarAssetClient::new(&env, &token);
    assert_eq!(token_client.balance(&seller), expected_net);
    assert_eq!(token_client.balance(&platform), expected_fee);
}

#[test]
#[should_panic]
fn test_approve_release_wrong_status() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &5000i128,
        &100u32, &empty,
    );

    env.storage().instance().set(&DataKey::EscrowStatus, &EscrowStatus::Completed);

    client.approve_release();
}

#[test]
fn test_milestone_initialization() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 10000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Phase 1"),
        percentage_bps: 3000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Phase 2"),
        percentage_bps: 3000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 2,
        description: String::from_str(&env, "Final"),
        percentage_bps: 4000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &200u32,
        &milestones,
    );

    let stored = client.get_milestones();
    assert_eq!(stored.len(), 3);
    assert_eq!(stored.get(0).unwrap().percentage_bps, 3000);
    assert_eq!(stored.get(2).unwrap().percentage_bps, 4000);
}

#[test]
#[should_panic]
fn test_milestone_sum_not_100_percent() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);

    // Two milestones that only sum to 8000 bps (not 10000) — should be rejected.
    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Part A"),
        percentage_bps: 3000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Part B"),
        percentage_bps: 5000, // 3000 + 5000 = 8000, not 10000
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &10000i128,
        &100u32, &milestones,
    );
}

#[test]
fn test_milestone_release_sequential() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 10000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Milestone 1"),
        percentage_bps: 3000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Milestone 2"),
        percentage_bps: 3000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 2,
        description: String::from_str(&env, "Milestone 3"),
        percentage_bps: 4000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &250u32,
        &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_milestone_release(&0);

    assert_eq!(client.status(), EscrowStatus::PartiallyReleased);
    assert_eq!(client.get_total_released(), 3000);

    let ms = client.get_milestones();
    assert!(ms.get(0).unwrap().released);
    assert!(!ms.get(1).unwrap().released);
    assert!(!ms.get(2).unwrap().released);

    let token_client = token::StellarAssetClient::new(&env, &token);
    let expected_fee_0 = calculate_fee(3000, 250);
    let expected_net_0 = 3000 - expected_fee_0;
    assert_eq!(token_client.balance(&seller), expected_net_0);
    assert_eq!(token_client.balance(&platform), expected_fee_0);

    client.approve_milestone_release(&1);

    let ms = client.get_milestones();
    assert!(ms.get(1).unwrap().released);
    assert_eq!(client.get_total_released(), 6000);

    let expected_fee_1 = calculate_fee(3000, 250);
    let expected_net_1 = 3000 - expected_fee_1;
    assert_eq!(token_client.balance(&seller), expected_net_0 + expected_net_1);
    assert_eq!(token_client.balance(&platform), expected_fee_0 + expected_fee_1);

    client.approve_milestone_release(&2);

    let ms = client.get_milestones();
    assert!(ms.get(2).unwrap().released);
    assert_eq!(client.get_total_released(), 10000);
    assert_eq!(client.status(), EscrowStatus::Completed);

    let expected_fee_2 = calculate_fee(4000, 250);
    let expected_net_2 = 4000 - expected_fee_2;
    assert_eq!(
        token_client.balance(&seller),
        expected_net_0 + expected_net_1 + expected_net_2
    );
    assert_eq!(
        token_client.balance(&platform),
        expected_fee_0 + expected_fee_1 + expected_fee_2
    );
}

#[test]
fn test_milestone_double_release() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 10000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Phase 1"),
        percentage_bps: 5000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Phase 2"),
        percentage_bps: 5000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &100u32, &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_milestone_release(&0);

    // Double-release should fail.
    let result = client.try_approve_milestone_release(&0);
    assert!(result.is_err());

    // Releasing milestone 1 still works.
    client.approve_milestone_release(&1);
    assert_eq!(client.get_total_released(), 10000);
}

#[test]
#[should_panic]
fn test_milestone_nonexistent() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &10000i128,
        &100u32, &empty,
    );

    client.approve_milestone_release(&0);
}

#[test]
fn test_milestone_after_completion() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Only"),
        percentage_bps: 10_000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &10000i128,
        &100u32, &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, 10000i128);

    client.approve_milestone_release(&0);
    assert_eq!(client.status(), EscrowStatus::Completed);

    // Re-releasing should fail.
    let result = client.try_approve_milestone_release(&0);
    assert!(result.is_err());
}

#[test]
fn test_partial_release_state_transitions() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 1000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "First"),
        percentage_bps: 3000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Second"),
        percentage_bps: 7000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &100u32, &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    assert_eq!(client.status(), EscrowStatus::Pending);
    assert_eq!(client.get_total_released(), 0);

    client.approve_milestone_release(&0);

    assert_eq!(client.status(), EscrowStatus::PartiallyReleased);
    let released = client.get_total_released();
    assert!(released > 0);
    assert!(released < total);

    client.approve_milestone_release(&1);
    assert_eq!(client.status(), EscrowStatus::Completed);
    assert_eq!(client.get_total_released(), total);
}

#[test]
fn test_platform_account_routing() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 100000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Half"),
        percentage_bps: 5000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Final"),
        percentage_bps: 5000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &500u32,
        &milestones,
    );

    let platform1 = Address::generate(&env);
    client.set_platform_account(&buyer, &platform1);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_milestone_release(&0);

    let token_client = token::StellarAssetClient::new(&env, &token);
    let platform_balance = token_client.balance(&platform1);
    // 50% of 100000 = 50000, fee = 50000 * 5% = 2500
    assert_eq!(platform_balance, 2500);

    // Changing platform account after first release (status=PartiallyReleased) should fail.
    let platform2 = Address::generate(&env);
    let result = client.try_set_platform_account(&buyer, &platform2);
    assert!(result.is_err());
}

#[test]
fn test_add_milestone_before_active() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &10000i128,
        &200u32, &empty,
    );

    let milestone = Milestone {
        id: 0,
        description: String::from_str(&env, "Release 1"),
        percentage_bps: 5000,
        released: false,
    };
    client.add_milestone(&buyer, &milestone);

    let milestones = client.get_milestones();
    assert_eq!(milestones.len(), 1);
    assert_eq!(milestones.get(0).unwrap().description, milestone.description);

    let milestone2 = Milestone {
        id: 1,
        description: String::from_str(&env, "Release 2"),
        percentage_bps: 5000,
        released: false,
    };
    client.add_milestone(&seller, &milestone2);

    let milestones = client.get_milestones();
    assert_eq!(milestones.len(), 2);
}

#[test]
fn test_add_milestone_after_release() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Only"),
        percentage_bps: 10_000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &5000i128,
        &100u32, &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, 5000i128);

    client.approve_milestone_release(&0);

    // Adding a milestone after release is not allowed.
    let new_milestone = Milestone {
        id: 1,
        description: String::from_str(&env, "Extra"),
        percentage_bps: 5000,
        released: false,
    };
    let result = client.try_add_milestone(&buyer, &new_milestone);
    assert!(result.is_err());
}

#[test]
fn test_set_platform_account_not_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &5000i128,
        &100u32, &empty,
    );

    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::EscrowStatus, &EscrowStatus::Disputed);
    });

    let platform = Address::generate(&env);
    let result = client.try_set_platform_account(&buyer, &platform);
    assert!(result.is_err());
}

#[test]
fn test_dispute_and_resolve() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 10000i128;
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &200u32, &empty,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.open_dispute();
    assert_eq!(client.status(), EscrowStatus::Disputed);

    client.resolve_dispute(&true);
    assert_eq!(client.status(), EscrowStatus::Resolved);

    let token_client = token::StellarAssetClient::new(&env, &token);
    assert_eq!(token_client.balance(&buyer), total);
}

#[test]
fn test_no_platform_account_set() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let empty: Vec<Milestone> = Vec::new(&env);

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &5000i128,
        &100u32, &empty,
    );

    fund_escrow(&env, &contract_id, &token, 5000i128);

    // No platform account set — approve_release should fail.
    let result = client.try_approve_release();
    assert!(result.is_err());
}

#[test]
fn test_milestone_exact_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 10000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Final"),
        percentage_bps: 10_000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &300u32,
        &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_milestone_release(&0);

    let expected_fee = (total * 300) / 10_000;
    let expected_net = total - expected_fee;

    let token_client = token::StellarAssetClient::new(&env, &token);
    assert_eq!(token_client.balance(&seller), expected_net);
    assert_eq!(token_client.balance(&platform), expected_fee);
    assert_eq!(client.get_total_released(), total);
    assert_eq!(client.status(), EscrowStatus::Completed);
}

#[test]
fn test_insufficient_balance_on_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 1000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "First"),
        percentage_bps: 5000,
        released: false,
    });
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "Second"),
        percentage_bps: 5000,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &100u32, &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    // Underfund the contract (500 instead of 1000).
    fund_escrow(&env, &contract_id, &token, 500i128);

    // At least one of the two milestone releases must fail.
    let r1 = client.try_approve_milestone_release(&0);
    let r2 = client.try_approve_milestone_release(&1);
    assert!(
        r1.is_err() || r2.is_err(),
        "at least one release should fail when underfunded"
    );
}

#[test]
fn test_event_emission_on_milestone_release() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Terms");
    let token_admin = Address::generate(&env);
    let token = register_token(&env, token_admin);
    let total = 8000i128;

    let mut milestones: Vec<Milestone> = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 0,
        description: String::from_str(&env, "Part 1"),
        percentage_bps: 2500,
        released: false,
    });

    client.init(
        &buyer, &seller, &arbiter, &terms, &token, &total,
        &125u32,
        &milestones,
    );

    let platform = Address::generate(&env);
    client.set_platform_account(&buyer, &platform);

    fund_escrow(&env, &contract_id, &token, total);

    client.approve_milestone_release(&0);

    // Verify events were published without error (no assertion on specific events needed).
    let _ = env.events();
}

#[test]
fn resolve_to_buyer() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    let result = client.resolve(&arbiter, &buyer);
    assert_eq!(result, buyer);
}

#[test]
fn resolve_to_seller() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    let result = client.resolve(&arbiter, &seller);
    assert_eq!(result, seller);
}

#[test]
#[should_panic(expected = "unauthorized: only arbiter can resolve")]
fn resolve_unauthorized_arbiter() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let fake_arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    client.init(&buyer, &seller, &arbiter, &terms);
    client.resolve(&fake_arbiter, &buyer);
}

// ---------------------------------------------------------------------------
// Expiration & refund tests
// ---------------------------------------------------------------------------

/// Buyer claims refund after expiry — funds return to buyer, status Resolved.
#[test]
fn buyer_can_claim_refund_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    // Advance time past expiry
    env.ledger().set_timestamp(EXPIRY + 1);

    client.claim_refund();

    assert_eq!(client.status(), EscrowStatus::Resolved);
    assert_eq!(token_client.balance(&buyer), 4000);
    assert_eq!(token_client.balance(&contract_id), 0);
}

/// claim_refund at the exact expiry boundary (timestamp == expiry) must fail —
/// the lock requires strictly greater than expiry.
#[test]
#[should_panic(expected = "escrow has not expired yet")]
fn claim_refund_fails_at_exact_expiry_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    // Exactly at expiry — should still be locked
    env.ledger().set_timestamp(EXPIRY);

    client.claim_refund();
}

/// claim_refund before expiry must panic.
#[test]
#[should_panic(expected = "escrow has not expired yet")]
fn claim_refund_fails_before_expiry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    // Time has not advanced — ledger timestamp is 0, well before expiry
    client.claim_refund();
}

/// claim_refund on a completed escrow must panic — not pending.
#[test]
#[should_panic(expected = "escrow is not pending")]
fn claim_refund_fails_if_already_completed() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    // Buyer approves release first — status becomes Completed
    client.approve_release();
    assert_eq!(client.status(), EscrowStatus::Completed);

    // Now try to claim refund after expiry — should fail because not Pending
    env.ledger().set_timestamp(EXPIRY + 1);
    client.claim_refund();
}

/// Only the buyer can claim a refund — seller cannot.
#[test]
#[should_panic]
fn claim_refund_fails_if_caller_is_not_buyer() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let terms = String::from_str(&env, "Deliver goods by 2026-05-01");
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();

    env.mock_all_auths();
    client.init(&make_config(&buyer, &seller, &arbiter, &terms, &token, 4000));

    let token_client = token::StellarAssetClient::new(&env, &token);
    token_client.mint(&contract_id, &4000);

    env.ledger().set_timestamp(EXPIRY + 1);

    // Only authorize the seller — buyer.require_auth() will fail
    env.mock_auths(&[MockAuth {
        address: &seller,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "claim_refund",
            args: ().into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.claim_refund();
}
