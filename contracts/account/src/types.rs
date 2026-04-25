use soroban_sdk::{ contracttype, Address, Vec };

#[contracttype]
pub enum DataKey {
    Manager,
    Merchant,
    Verified,
    Restricted,
    AccountInfo,
    TrackedTokens,
    WithdrawalThreshold,
    CoSigners,
    PendingWithdrawalCounter,
    PendingWithdrawals(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountInfo {
    pub manager: Address,
    pub merchant_id: u64,
    pub merchant: Address,
    pub date_created: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenBalance {
    pub token: Address,
    pub balance: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingWithdrawal {
    pub id: u64,
    pub token: Address,
    pub amount: i128,
    pub recipient: Address,
    pub initiator: Address,
    pub approvals: Vec<Address>,
    pub timestamp: u64,
}
