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
}
