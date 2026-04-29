use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TicketingError {
    EventNotFound = 1,
    TicketNotFound = 2,
    NotAuthorized = 3,
    EventAtCapacity = 4,
    DuplicateQRHash = 5,
    AlreadyCheckedIn = 6,
    TicketAlreadyCheckedIn = 7,
    InvalidTimeRange = 8,
    TierNotFound = 9,
    TierAtCapacity = 10,
    InvalidTierSupply = 11,
    InvalidTierPrice = 12,
    TierEventMismatch = 13,
    InvalidRoyaltyBps = 14,
    InvalidResalePrice = 15,
    ResaleNotConfigured = 16,
    SameHolder = 17,
}
