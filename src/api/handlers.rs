use crate::aggregator::DexAggregator;
use crate::pool_data_types::PoolState;
use crate::types::{BestRoute, SwapParams};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::types::ExecutionPriority;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct QuoteRequest {
    pub input_mint: String,
    pub output_mint: String,
    pub amount: u64,
    pub slippage_tolerance: f64,
    pub max_accounts: Option<u32>,
    pub priority: Option<String>,
}

#[derive(Serialize)]
pub struct QuoteResponse {
    pub routes: Vec<RouteInfo>,
    pub input_amount: u64,
    pub output_amount: u64,
    pub price_impact: f64,
    pub estimated_gas: u64,
    pub execution_time_ms: u64,
    pub route_id: String,
}

#[derive(Serialize)]
pub struct RouteInfo {
    pub dex: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub price_impact: f64,
    pub pool_address: String,
}

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn get_quote(
    State(aggregator): State<Arc<DexAggregator>>,
    Json(request): Json<QuoteRequest>,
) -> Result<Json<QuoteResponse>, StatusCode> {
    // Parse pubkeys
    let input_mint =
        Pubkey::try_from(request.input_mint.as_str()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let output_mint =
        Pubkey::try_from(request.output_mint.as_str()).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Create swap params
    // let swap_params = SwapParams {
    //     input_mint,
    //     output_mint,
    //     amount: request.amount,
    //     slippage_tolerance: request.slippage_tolerance,
    //     max_accounts: request.max_accounts.unwrap_or(64),
    //     priority: parse_priority(&request.priority),
    // };

    // Get best route using the aggregator
    // match aggregator.find_best_route(&swap_params).await {
    //     Ok(best_route) => {
    //         let response = QuoteResponse {
    //             routes: best_route.routes.iter().map(|r| RouteInfo {
    //                 dex: r.dex.to_string(),
    //                 input_amount: r.input_amount,
    //                 output_amount: r.output_amount,
    //                 price_impact: r.price_impact.to_f64().unwrap_or(0.0),
    //                 pool_address: r.pool_address.to_string(),
    //             }).collect(),
    //             input_amount: best_route.total_input_amount,
    //             output_amount: best_route.total_output_amount,
    //             price_impact: best_route.total_price_impact.to_f64().unwrap_or(0.0),
    //             estimated_gas: 150_000, // Estimate based on route complexity
    //             execution_time_ms: 2000,
    //             route_id: uuid::Uuid::new_v4().to_string(),
    //         };
    //         Ok(Json(response))
    //     }
    //     Err(_) => Err(StatusCode::NOT_FOUND),
    // }
    Err(StatusCode::NOT_FOUND)
}

pub async fn get_pools(
    State(aggregator): State<Arc<DexAggregator>>,
    Path((token0, token1)): Path<(String, String)>,
) -> Result<Json<Vec<PoolState>>, StatusCode> {
    // check if token0 and token1 are valid pubkeys
    let token0_key = Pubkey::try_from(token0.as_str()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let token1_key = Pubkey::try_from(token1.as_str()).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get pool information for a token pair
    let pools = aggregator
        .get_pool_manager()
        .get_pools_for_pair(&token0_key, &token1_key)
        .await;

    Ok(Json(pools))
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
