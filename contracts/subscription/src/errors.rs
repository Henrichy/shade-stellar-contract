use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SubscriptionError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    InvalidAmount = 4,
    InvalidInterval = 5,
    PlanNotFound = 6,
    PlanNotActive = 7,
    SubscriptionNotFound = 8,
    SubscriptionNotActive = 9,
    ChargeTooEarly = 10,
    InsufficientAllowance = 11,
    TokenNotAccepted = 12,
    SubscriptionTerminated = 13,
    GraceNotExpired = 14,
    NothingToRefund = 15,
}
