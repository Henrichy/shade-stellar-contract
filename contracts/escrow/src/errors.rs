use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    InvalidAmount = 4,
    InvalidFeePercentage = 5,
    MilestoneSumMismatch = 6,
    MilestoneNotFound = 7,
    MilestoneAlreadyReleased = 8,
    InsufficientBalance = 9,
    CannotAddMilestone = 10,
    PlatformAccountNotSet = 11,
    InvalidStatus = 12,
}
