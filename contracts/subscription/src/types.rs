use soroban_sdk::{contracttype, Address, String};

#[contracttype]
pub enum DataKey {
    Admin,
    AcceptedTokens,
    Plan(u64),
    PlanCount,
    Subscription(u64),
    SubscriptionCount,
}

/// A billing plan created by a merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub id: u64,
    pub merchant: Address,
    pub description: String,
    /// Token used for recurring billing.
    pub token: Address,
    /// Amount charged per interval (in token base units).
    pub amount: i128,
    /// Billing interval in seconds (e.g. 2_592_000 = 30 days).
    pub interval: u64,
    pub active: bool,
    pub created_at: u64,
}

/// An active or cancelled subscription held by a customer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    pub id: u64,
    pub plan_id: u64,
    pub customer: Address,
    pub status: SubscriptionStatus,
    pub created_at: u64,
    /// Timestamp of the last successful charge; 0 means never charged.
    pub last_charged: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SubscriptionStatus {
    Active = 0,
    Cancelled = 1,
}
