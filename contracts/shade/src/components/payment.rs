use crate::errors::ContractError;
use crate::types::{PaymentPayload, PaymentRoute};
use soroban_sdk::{panic_with_error, Env};

const MAX_SLIPPAGE_BPS: u32 = 10_000;

pub fn validate_payment_payload(env: &Env, payload: &PaymentPayload) {
    match &payload.route {
        PaymentRoute::Direct => {
            if payload.input_token != payload.settlement_token {
                panic_with_error!(env, ContractError::InvalidSwapPath);
            }

            if payload.max_slippage_bps.is_some() {
                panic_with_error!(env, ContractError::InvalidSlippage);
            }
        }
        PaymentRoute::Swap(route) => {
            if route.path.len() < 2 {
                panic_with_error!(env, ContractError::InvalidSwapPath);
            }

            let first_hop = route
                .path
                .get(0)
                .unwrap_or_else(|| panic_with_error!(env, ContractError::InvalidSwapPath));
            let last_hop = route
                .path
                .get(route.path.len() - 1)
                .unwrap_or_else(|| panic_with_error!(env, ContractError::InvalidSwapPath));

            if first_hop != payload.input_token
                || last_hop != payload.settlement_token
                || payload.input_token == payload.settlement_token
            {
                panic_with_error!(env, ContractError::InvalidSwapPath);
            }

            let slippage_bps = payload
                .max_slippage_bps
                .unwrap_or_else(|| panic_with_error!(env, ContractError::InvalidSlippage));

            if slippage_bps > MAX_SLIPPAGE_BPS {
                panic_with_error!(env, ContractError::InvalidSlippage);
            }
        }
    }
}
