#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String};

#[contract]
pub struct EscrowContract;

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Buyer,
    Seller,
    Arbiter,
    Terms,
    Token,
    Amount,
    ExpiresAt,
    Refunded,
}

#[contractimpl]
impl EscrowContract {
    /// Initialize the escrow. `expires_at` is a Unix timestamp (seconds) after
    /// which the buyer may claim a refund if the seller has not fulfilled.
    pub fn init(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        terms: String,
        token: Address,
        amount: i128,
        expires_at: u64,
    ) {
        if env.storage().instance().has(&DataKey::Buyer) {
            panic!("escrow already initialized");
        }

        if expires_at <= env.ledger().timestamp() {
            panic!("expires_at must be in the future");
        }

        if amount <= 0 {
            panic!("amount must be positive");
        }

        env.storage().instance().set(&DataKey::Buyer, &buyer);
        env.storage().instance().set(&DataKey::Seller, &seller);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        env.storage().instance().set(&DataKey::Terms, &terms);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Amount, &amount);
        env.storage().instance().set(&DataKey::ExpiresAt, &expires_at);
        env.storage().instance().set(&DataKey::Refunded, &false);
    }

    /// Claim a refund after the delivery timeframe has expired without fulfillment.
    /// Only the buyer may call this, and only after `expires_at` has passed.
    pub fn claim_refund(env: Env, buyer: Address) {
        buyer.require_auth();

        let stored_buyer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Buyer)
            .expect("escrow not initialized");

        if buyer != stored_buyer {
            panic!("only the buyer can claim a refund");
        }

        let already_refunded: bool = env
            .storage()
            .instance()
            .get(&DataKey::Refunded)
            .unwrap_or(false);

        if already_refunded {
            panic!("refund already claimed");
        }

        let expires_at: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ExpiresAt)
            .expect("expiration not set");

        if env.ledger().timestamp() < expires_at {
            panic!("escrow has not expired yet");
        }

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .expect("token not set");

        let amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Amount)
            .expect("amount not set");

        // Mark as refunded before transfer (checks-effects-interactions)
        env.storage().instance().set(&DataKey::Refunded, &true);

        let token_client = token::TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &buyer, &amount);
    }

    pub fn buyer(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Buyer).unwrap()
    }

    pub fn seller(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Seller).unwrap()
    }

    pub fn arbiter(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Arbiter).unwrap()
    }

    pub fn terms(env: Env) -> String {
        env.storage().instance().get(&DataKey::Terms).unwrap()
    }

    pub fn expires_at(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::ExpiresAt).unwrap()
    }

    pub fn is_refunded(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Refunded)
            .unwrap_or(false)
    }
}

mod test;
