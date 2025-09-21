use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
// Ensure serde is imported for Json serialization
use axum::{http::StatusCode, Json};
use validator::{Validate, ValidationError};

use crate::{
    aggregator::DexAggregator,
    types::{SwapStep, Token},
};
#[derive(Serialize)] // Required for Json response
pub struct PoolInfoResponse {
    pub address: String,
    pub dex: String,
    pub base_token: String,
    pub quote_token: String,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub slot: u64,
    pub liquidity: f64,
}

#[derive(Deserialize, Serialize, Validate)]
pub struct QuoteRequest {
    #[validate(length(
        min = 32,
        max = 44,
        message = "Input token must be a valid Solana public key (32-44 characters)"
    ))]
    pub input_token: String,

    #[validate(length(
        min = 32,
        max = 44,
        message = "Output token must be a valid Solana public key (32-44 characters)"
    ))]
    pub output_token: String,

    #[validate(length(
        min = 32,
        max = 44,
        message = "User wallet must be a valid Solana public key (32-44 characters)"
    ))]
    pub user_wallet: String,

    #[validate(range(min = 1, message = "Input amount must be greater than 0"))]
    pub input_amount: u64,

    #[validate(range(
        min = 0,
        max = 10000,
        message = "Slippage must be between 0 and 10000 basis points (0-100%)"
    ))]
    pub slippage_bps: u16,
}

#[derive(Serialize)]
pub struct QuoteResponse {
    pub routes: Vec<SwapStep>,
    pub input_amount: u64,
    pub output_amount: u64,
    pub other_output_amount: u64,
    pub time_taken_ms: u64,
}

// Custom validation function for Solana pubkeys
fn validate_solana_pubkey(pubkey_str: &str) -> Result<(), ValidationError> {
    match bs58::decode(pubkey_str).into_vec() {
        Ok(bytes) if bytes.len() == 32 => Ok(()),
        _ => Err(ValidationError::new("Invalid Solana public key format")),
    }
}

// If you want more specific pubkey validation, you can add a custom validator
#[derive(Deserialize, Serialize, Validate)]
pub struct EnhancedQuoteRequest {
    #[validate(custom(function = "validate_solana_pubkey"))]
    pub input_token: String,

    #[validate(custom(function = "validate_solana_pubkey"))]
    pub output_token: String,

    #[validate(custom(function = "validate_solana_pubkey"))]
    pub user_wallet: String,

    #[validate(range(min = 1, message = "Input amount must be greater than 0"))]
    pub input_amount: u64,

    #[validate(range(
        min = 0,
        max = 10000,
        message = "Slippage must be between 0 and 10000 basis points (0-100%)"
    ))]
    pub slippage_bps: u16,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Vec<String>,
}

// Helper function to parse and validate pubkey with detailed error
pub fn parse_pubkey_with_error(
    pubkey_str: &str,
    field_name: &str,
) -> Result<Pubkey, (StatusCode, Json<ErrorResponse>)> {
    Pubkey::try_from(pubkey_str).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid {}", field_name),
                details: vec![format!("'{}' is not a valid Solana public key", pubkey_str)],
            }),
        )
    })
}

// Helper function to get token from pool manager with error handling
pub async fn get_token_with_error(
    aggregator: &Arc<DexAggregator>,
    pubkey: &Pubkey,
    token_str: &str,
    token_type: &str,
) -> Result<Token, (StatusCode, Json<ErrorResponse>)> {
    match aggregator.get_pool_manager().get_token(pubkey).await {
        Some(token) => Ok(token),
        None => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Token not found".to_string(),
                details: vec![format!(
                    "{} token '{}' not found in pool manager",
                    token_type, token_str
                )],
            }),
        )),
    }
}
