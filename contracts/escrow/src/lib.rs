#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String};

#[contract]
pub struct EscrowContract;

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Buyer,
    Seller,
    Arbiter,
    Terms,
}

#[contractimpl]
impl EscrowContract {
    pub fn init(env: Env, buyer: Address, seller: Address, arbiter: Address, terms: String) {
        if env.storage().instance().has(&DataKey::Buyer) {
            panic!("escrow already initialized");
        }
        env.storage().instance().set(&DataKey::Buyer, &buyer);
        env.storage().instance().set(&DataKey::Seller, &seller);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        env.storage().instance().set(&DataKey::Terms, &terms);
    }

    pub fn resolve(env: Env, caller: Address, recipient: Address) -> Address {
        caller.require_auth();
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        if caller != arbiter {
            panic!("unauthorized: only arbiter can resolve");
        }
        let buyer: Address = env.storage().instance().get(&DataKey::Buyer).unwrap();
        let seller: Address = env.storage().instance().get(&DataKey::Seller).unwrap();
        if recipient != buyer && recipient != seller {
            panic!("recipient must be buyer or seller");
        }
        recipient
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
}

mod test;
