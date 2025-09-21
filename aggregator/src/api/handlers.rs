use crate::api::dto::{
    get_token_with_error, parse_pubkey_with_error, ErrorResponse, PoolInfoResponse, QuoteRequest, QuoteResponse,
};
use crate::types::{ExecutionPriority, SwapParams};
use crate::{aggregator::DexAggregator, types::SwapStep};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use validator::Validate;

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn get_quote(
    State(aggregator): State<Arc<DexAggregator>>,
    Json(request): Json<QuoteRequest>,
) -> Result<Json<QuoteResponse>, (StatusCode, Json<ErrorResponse>)> {
    let start_time = Instant::now();

    // Validate the request
    if let Err(validation_errors) = request.validate() {
        let details: Vec<String> = validation_errors
            .field_errors()
            .iter()
            .flat_map(|(field, errors)| {
                errors.iter().map(move |error| {
                    format!(
                        "{}: {}",
                        field,
                        error.message.as_deref().unwrap_or("Validation failed")
                    )
                })
            })
            .collect();

        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Validation failed".to_string(),
                details,
            }),
        ));
    }

    // check if token0 and token1 are valid pubkeys
    let input_token_key = parse_pubkey_with_error(request.input_token.as_str(), "input_token")?;
    let output_token_key = parse_pubkey_with_error(request.output_token.as_str(), "output_token")?;
    let user_wallet = parse_pubkey_with_error(request.user_wallet.as_str(), "user_wallet")?;

    // Get tokens from pool manager
    let input_token =
        get_token_with_error(&aggregator, &input_token_key, &request.input_token, "Input").await?;
    let output_token = get_token_with_error(
        &aggregator,
        &output_token_key,
        &request.output_token,
        "Output",
    )
    .await?;

    // Create swap params
    let swap_params = SwapParams {
        input_token,
        output_token,
        input_amount: request.input_amount,
        slippage_bps: request.slippage_bps,
        user_wallet,
        priority: ExecutionPriority::Medium,
    };

    // Get best route using the aggregator
    match aggregator.get_swap_route(&swap_params).await {
        Some(best_route) => {
            // first, get all swap step started from the input token
            let mut swap_routes: Vec<SwapStep> = vec![];
            let mut intermediate_tokens: HashSet<String> = HashSet::new();
            best_route.paths.iter().for_each(|path| {
                path.steps.iter().for_each(|step| {
                    if step.input_token == request.input_token {
                        swap_routes.push(step.clone());
                        if step.output_token != request.output_token {
                            intermediate_tokens.insert(step.output_token.clone());
                        }
                    }
                });
            });

            // run a second to add swap step for intermediate tokens
            best_route.paths.iter().for_each(|path| {
                path.steps.iter().for_each(|step| {
                    if intermediate_tokens.contains(step.input_token.as_str()) {
                        swap_routes.push(step.clone());
                    }
                });
            });

            let time_taken_ms = start_time.elapsed().as_millis() as u64;
            let response = QuoteResponse {
                routes: swap_routes,
                input_amount: best_route.input_amount,
                output_amount: best_route.output_amount,
                other_output_amount: best_route.other_output_amount,
                time_taken_ms,
            };
            Ok(Json(response))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "No swap route found".to_string(),
                details: vec![format!(
                    "No available route for swapping {} {} to {}",
                    request.input_amount, request.input_token, request.output_token
                )],
            }),
        )),
    }
}

pub async fn get_pools(
    State(aggregator): State<Arc<DexAggregator>>,
    Path((token0, token1)): Path<(String, String)>,
) -> Result<Json<Vec<PoolInfoResponse>>, StatusCode> {
    // check if token0 and token1 are valid pubkeys
    let token0_key = Pubkey::try_from(token0.as_str()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let token1_key = Pubkey::try_from(token1.as_str()).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get pool information for a token pair
    let pools = aggregator
        .get_pool_manager()
        .get_pools_for_pair(&token0_key, &token1_key)
        .await;
    // read get_tokens function and map to PoolInfoResponse
    let pools_response = pools
        .into_iter()
        .map(|pool| {
            let (base_pk, quote_pk) = pool.get_tokens();
            let (base_reserve, quote_reserve) = pool.get_reserves();
            PoolInfoResponse {
                address: pool.address().to_string(),
                dex: pool.dex().to_string(),
                base_token: base_pk.to_string(),
                quote_token: quote_pk.to_string(),
                last_updated: pool.last_updated(),
                base_reserve,
                quote_reserve,
                slot: pool.get_metadata().slot,
                liquidity: pool.get_liquidity_usd(),
            }
        })
        .collect();

    Ok(Json(pools_response))
}

#[derive(Serialize)]
pub struct PoolInfo {
    pub address: String,
    pub dex: String,
    pub token_a: String,
    pub token_b: String,
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub fee: f64,
    pub last_updated: u64,
}

fn parse_priority(priority: &Option<String>) -> ExecutionPriority {
    match priority.as_deref() {
        Some("high") => ExecutionPriority::High,
        Some("low") => ExecutionPriority::Low,
        _ => ExecutionPriority::Medium,
    }
}
