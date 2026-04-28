#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Bytes as _};
use soroban_sdk::{Address, BytesN, Env, String};

// Helper to generate a fixed 32-byte hash for testing
fn test_qr_hash(env: &Env, value: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = value;
    BytesN::from_slice(&env, &bytes)
}

#[test]
fn test_create_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let name = String::from_str(&env, "Concert 2026");
    let description = String::from_str(&env, "Annual music concert");
    let start_time = 1_750_000_000; // Future timestamp
    let end_time = 1_750_000_600;

    let event_id = client.create_event(
        &organizer,
        &name,
        &description,
        start_time,
        end_time,
        None,
    );

    let event = client.get_event(event_id);
    assert_eq!(event.event_id, event_id);
    assert_eq!(event.organizer, organizer);
    assert_eq!(event.name, name);
    assert_eq!(event.description, description);
    assert_eq!(event.start_time, start_time);
    assert_eq!(event.end_time, end_time);
    assert_eq!(event.max_capacity, None);
}

#[test]
#[should_panic(expected = "TicketingError")]
fn test_create_event_invalid_time_range() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let name = String::from_str(&env, "Test Event");
    let description = String::from_str(&env, "Test");
    let start_time = 100;
    let end_time = 50; // end before start

    client.create_event(
        &organizer,
        &name,
        &description,
        start_time,
        end_time,
        None,
    );
}

#[test]
fn test_issue_ticket_and_duplicate_prevention() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    // Create event
    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event 1"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    // Issue first ticket
    let qr_hash1 = test_qr_hash(&env, 1);
    let ticket_id1 = client.issue_ticket(&organizer, event_id, &holder1, qr_hash1.clone());

    let ticket1 = client.get_ticket(ticket_id1);
    assert_eq!(ticket1.ticket_id, ticket_id1);
    assert_eq!(ticket1.event_id, event_id);
    assert_eq!(ticket1.holder, holder1);
    assert_eq!(ticket1.qr_hash, qr_hash1);
    assert!(!ticket1.checked_in);
    assert_eq!(ticket1.check_in_time, None);

    // Issue second ticket for same event, different holder
    let qr_hash2 = test_qr_hash(&env, 2);
    let ticket_id2 = client.issue_ticket(&organizer, event_id, &holder2, qr_hash2.clone());
    assert_ne!(ticket_id1, ticket_id2);

    // Duplicate QR hash should be rejected
    let qr_hash_dup = test_qr_hash(&env, 1); // same as qr_hash1
    let organizer_auth = organizer.clone();
    env.with_auth(&organizer_auth, || {
        let result = std::panic::catch_unwind(|| {
            client.issue_ticket(&organizer, event_id, &holder2, qr_hash_dup);
        });
        assert!(result.is_err());
    });
}

#[test]
fn test_verify_ticket() {
    let env = Env::default();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let holder = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash = test_qr_hash(&env, 42);
    let ticket_id = client.issue_ticket(&organizer, event_id, &holder, qr_hash.clone());

    // Verify with correct hash
    let verification = client.verify_ticket(ticket_id, qr_hash.clone());
    assert!(verification.valid);
    assert!(!verification.already_checked_in);
    assert_eq!(verification.ticket_id, ticket_id);
    assert_eq!(verification.holder, holder);

    // Verify with wrong hash
    let wrong_hash = test_qr_hash(&env, 99);
    let verification_wrong = client.verify_ticket(ticket_id, wrong_hash);
    assert!(!verification_wrong.valid);
}

#[test]
fn test_check_in_valid_and_duplicate() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let operator = Address::generate(&env);
    let holder = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash = test_qr_hash(&env, 10);
    let ticket_id = client.issue_ticket(&organizer, event_id, &holder, qr_hash);

    // First check-in should succeed
    client.check_in(&operator, ticket_id);

    let ticket = client.get_ticket(ticket_id);
    assert!(ticket.checked_in);
    assert!(ticket.check_in_time.is_some());

    // Duplicate check-in should fail
    let result = std::panic::catch_unwind(|| {
        client.check_in(&operator, ticket_id);
    });
    assert!(result.is_err());

    // Verify CheckInRecord exists
    let check_in_record = client.get_check_in_record(ticket_id);
    assert!(check_in_record.is_some);
    let record = check_in_record.unwrap;
    assert_eq!(record.ticket_id, ticket_id);
    assert_eq!(record.checked_in_by, operator);
}

#[test]
fn test_check_in_with_wrong_operator() {
    let env = Env::default();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let operator1 = Address::generate(&env);
    let operator2 = Address::generate(&env);
    let holder = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash = test_qr_hash(&env, 10);
    let ticket_id = client.issue_ticket(&organizer, event_id, &holder, qr_hash);

    // Only authorized operator can check in
    env.with_auth(&operator1, || {
        let result = std::panic::catch_unwind(|| {
            client.check_in(&operator1, ticket_id);
        });
        assert!(result.is_err()); // should fail because operator not authorized
    });
}

#[test]
fn test_ticket_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash = test_qr_hash(&env, 10);
    let ticket_id = client.issue_ticket(&organizer, event_id, &holder1, qr_hash);

    // Transfer ticket
    client.transfer_ticket(&holder1, ticket_id, &holder2);

    let ticket = client.get_ticket(ticket_id);
    assert_eq!(ticket.holder, holder2);

    // Transfer already checked-in ticket should fail
    let operator = Address::generate(&env);
    client.check_in(&operator, ticket_id);

    let result = std::panic::catch_unwind(|| {
        client.transfer_ticket(&holder2, ticket_id, &holder1);
    });
    assert!(result.is_err());
}

#[test]
fn test_transfer_not_authorized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let imposter = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash = test_qr_hash(&env, 10);
    let ticket_id = client.issue_ticket(&organizer, event_id, &holder1, qr_hash);

    // Imposter cannot transfer
    let result = std::panic::catch_unwind(|| {
        client.transfer_ticket(&imposter, ticket_id, &holder2);
    });
    assert!(result.is_err());
}

#[test]
fn test_event_capacity_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let holder = Address::generate(&env);

    // Event with capacity 2
    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Small Event"),
        &String::from_str(&env, "Limited capacity"),
        1000,
        2000,
        Some(2),
    );

    // Issue first ticket
    let qr_hash1 = test_qr_hash(&env, 1);
    client.issue_ticket(&organizer, event_id, &holder, qr_hash1);

    // Issue second ticket
    let qr_hash2 = test_qr_hash(&env, 2);
    client.issue_ticket(&organizer, event_id, &holder, qr_hash2);

    // Third ticket should fail (at capacity)
    let qr_hash3 = test_qr_hash(&env, 3);
    let result = std::panic::catch_unwind(|| {
        client.issue_ticket(&organizer, event_id, &holder, qr_hash3);
    });
    assert!(result.is_err());

    // Verify count
    assert_eq!(client.get_event_ticket_count(event_id), 2);
}

#[test]
fn test_get_event_tickets() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash1 = test_qr_hash(&env, 1);
    let ticket_id1 = client.issue_ticket(&organizer, event_id, &holder1, qr_hash1);

    let qr_hash2 = test_qr_hash(&env, 2);
    let ticket_id2 = client.issue_ticket(&organizer, event_id, &holder2, qr_hash2);

    let tickets = client.get_event_tickets(event_id);
    assert_eq!(tickets.len(), 2);

    let ids: Vec<u64> = tickets.iter().map(|t| t.ticket_id).collect();
    assert!(ids.contains(&ticket_id1));
    assert!(ids.contains(&ticket_id2));
}

#[test]
fn test_check_in_counters() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let operator = Address::generate(&env);
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash1 = test_qr_hash(&env, 1);
    let ticket_id1 = client.issue_ticket(&organizer, event_id, &holder1, qr_hash1);

    let qr_hash2 = test_qr_hash(&env, 2);
    let ticket_id2 = client.issue_ticket(&organizer, event_id, &holder2, qr_hash2);

    assert_eq!(client.get_event_ticket_count(event_id), 2);
    assert_eq!(client.get_event_checked_in_count(event_id), 0);

    client.check_in(&operator, ticket_id1);
    assert_eq!(client.get_event_checked_in_count(event_id), 1);

    client.check_in(&operator, ticket_id2);
    assert_eq!(client.get_event_checked_in_count(event_id), 2);
}

#[test]
fn test_non_organizer_cannot_issue_ticket() {
    let env = Env::default();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let imposter = Address::generate(&env);
    let holder = Address::generate(&env);

    let event_id = {
        env.with_auth(&organizer, || {
            client.create_event(
                &organizer,
                &String::from_str(&env, "Event"),
                &String::from_str(&env, "Desc"),
                1000,
                2000,
                None,
            )
        })
    };

    let qr_hash = test_qr_hash(&env, 1);
    env.with_auth(&imposter, || {
        let result = std::panic::catch_unwind(|| {
            client.issue_ticket(&imposter, event_id, &holder, qr_hash);
        });
        assert!(result.is_err());
    });
}

#[test]
fn test_nonexistent_ticket() {
    let env = Env::default();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let result = std::panic::catch_unwind(|| {
        client.get_ticket(999);
    });
    assert!(result.is_err());
}

#[test]
fn test_verify_after_check_in() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let organizer = Address::generate(&env);
    let operator = Address::generate(&env);
    let holder = Address::generate(&env);

    let event_id = client.create_event(
        &organizer,
        &String::from_str(&env, "Event"),
        &String::from_str(&env, "Desc"),
        1000,
        2000,
        None,
    );

    let qr_hash = test_qr_hash(&env, 42);
    let ticket_id = client.issue_ticket(&organizer, event_id, &holder, qr_hash);

    // Verify before check-in
    let v1 = client.verify_ticket(ticket_id, qr_hash.clone());
    assert!(v1.valid);
    assert!(!v1.already_checked_in);

    // Check in
    client.check_in(&operator, ticket_id);

    // Verify after check-in
    let v2 = client.verify_ticket(ticket_id, qr_hash);
    assert!(v2.valid);
    assert!(v2.already_checked_in);
}

#[test]
fn test_event_not_found() {
    let env = Env::default();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let result = std::panic::catch_unwind(|| {
        client.get_event(9999);
    });
    assert!(result.is_err());
}

#[test]
fn test_ticket_not_found() {
    let env = Env::default();
    let contract_id = env.register(TicketingContract, ());
    let client = TicketingContractClient::new(&env, &contract_id);

    let result = std::panic::catch_unwind(|| {
        client.get_ticket(9999);
    });
    assert!(result.is_err());
}
