use crate::api::dto::{
    get_token_with_error, parse_pubkey_with_error, AddTokenRequest, ArbitrageRequest,
    ArbitrageResponse, ArbitrageTokenResponse, ArbitrageTokensResponse, ErrorResponse,
    PoolInfoResponse, PoolsResponse, QuoteRequest, QuoteResponse, RemoveTokenRequest,
    TokenOperationResponse,
};
use crate::api::AppState;
use crate::pool_manager::PoolManagerStats;
use crate::types::SwapStep;
use crate::types::{ExecutionPriority, SwapParams};
use crate::utils::tokens_equal;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use validator::Validate;

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn get_pool_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PoolManagerStats>, StatusCode> {
    // Placeholder implementation
    let stats = state.aggregator.get_pool_manager().get_stats().await;
    Ok(Json(stats))
}

pub async fn get_quote(
    State(state): State<Arc<AppState>>,
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
    let input_token = get_token_with_error(
        &state.aggregator,
        &input_token_key,
        &request.input_token,
        "Input",
    )
    .await?;
    let output_token = get_token_with_error(
        &state.aggregator,
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
    match state.aggregator.get_swap_route(&swap_params).await {
        Some(best_route) => {
            // first, get all swap step started from the input token
            let mut swap_routes: Vec<SwapStep> = vec![];
            let mut intermediate_tokens: HashSet<Pubkey> = HashSet::new();
            best_route.paths.iter().for_each(|path| {
                path.steps.iter().for_each(|step| {
                    if tokens_equal(&step.input_token, &swap_params.input_token.address) {
                        swap_routes.push(SwapStep {
                            dex: step.dex,
                            input_token: step.input_token.to_string(),
                            output_token: step.output_token.to_string(),
                            pool_address: step.pool_address.to_string(),
                            input_amount: step.input_amount,
                            output_amount: step.output_amount,
                            percent: step.percent,
                        });
                        if !tokens_equal(&step.output_token, &swap_params.output_token.address) {
                            intermediate_tokens.insert(step.output_token);
                        }
                    }
                });
            });

            // run a second to add swap step for intermediate tokens
            best_route.paths.iter().for_each(|path| {
                path.steps.iter().for_each(|step| {
                    if intermediate_tokens.contains(&step.input_token) {
                        swap_routes.push(SwapStep {
                            dex: step.dex,
                            input_token: step.input_token.to_string(),
                            output_token: step.output_token.to_string(),
                            pool_address: step.pool_address.to_string(),
                            input_amount: step.input_amount,
                            output_amount: step.output_amount,
                            percent: step.percent,
                        });
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
                context_slot: best_route.context_slot,
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
    State(state): State<Arc<AppState>>,
    Path((token0, token1)): Path<(String, String)>,
) -> Result<Json<PoolsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let start_time = Instant::now();

    // check if token0 and token1 are valid pubkeys
    let token0_key = Pubkey::try_from(token0.as_str()).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid token0 address".to_string(),
                details: vec!["token0 must be a valid Solana public key".to_string()],
            }),
        )
    })?;
    let token1_key = Pubkey::try_from(token1.as_str()).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid token1 address".to_string(),
                details: vec!["token1 must be a valid Solana public key".to_string()],
            }),
        )
    })?;

    // Get pool information for a token pair
    let pools = state
        .aggregator
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
                time_taken_ms: 0, // Will be set in wrapper response
            }
        })
        .collect();

    let time_taken_ms = start_time.elapsed().as_millis() as u64;

    Ok(Json(PoolsResponse {
        pools: pools_response,
        time_taken_ms,
    }))
}

pub async fn check_arbitrage(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ArbitrageRequest>,
) -> Result<Json<ArbitrageResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Parse and validate pubkeys
    let token_a_key = parse_pubkey_with_error(request.token_a.as_str(), "token_a")?;
    let token_b_key = parse_pubkey_with_error(request.token_b.as_str(), "token_b")?;
    let user_wallet = parse_pubkey_with_error(request.user_wallet.as_str(), "user_wallet")?;

    // Get token A from pool manager
    let token_a =
        get_token_with_error(&state.aggregator, &token_a_key, &request.token_a, "Token A").await?;

    // Create swap params for tokenA -> tokenB
    let swap_params = SwapParams {
        input_token: token_a.clone(),
        output_token: token_a.clone(), // placeholder, will be replaced
        input_amount: request.input_amount,
        slippage_bps: request.slippage_bps,
        user_wallet,
        priority: ExecutionPriority::Medium,
    };

    // Calculate arbitrage profit
    match state
        .aggregator
        .calculate_arbitrage_profit(&swap_params, &token_b_key, request.slippage_bps)
        .await
    {
        Some((profit, forward_route, reverse_route, reverse)) => {
            if reverse {
                let profit_percent = (profit as f64 / request.input_amount as f64) * 100.0;

                // Extract swap steps from forward route
                let mut forward_steps: Vec<SwapStep> = vec![];
                for path in &reverse_route.paths {
                    for step in &path.steps {
                        forward_steps.push(SwapStep {
                            dex: step.dex,
                            input_token: step.input_token.to_string(),
                            output_token: step.output_token.to_string(),
                            pool_address: step.pool_address.to_string(),
                            input_amount: step.input_amount,
                            output_amount: step.output_amount,
                            percent: step.percent,
                        });
                    }
                }

                // Extract swap steps from reverse route
                let mut reverse_steps: Vec<SwapStep> = vec![];
                for path in &forward_route.paths {
                    for step in &path.steps {
                        reverse_steps.push(SwapStep {
                            dex: step.dex,
                            input_token: step.input_token.to_string(),
                            output_token: step.output_token.to_string(),
                            pool_address: step.pool_address.to_string(),
                            input_amount: step.input_amount,
                            output_amount: step.output_amount,
                            percent: step.percent,
                        });
                    }
                }

                let time_taken_ms = start_time.elapsed().as_millis() as u64;
                let response = ArbitrageResponse {
                    profitable: true,
                    profit_amount: profit,
                    profit_percent,
                    forward_route: forward_steps,
                    reverse_route: reverse_steps,
                    forward_output: reverse_route.output_amount,
                    reverse_output: forward_route.output_amount,
                    time_taken_ms,
                    context_slot: reverse_route.context_slot,
                };
                Ok(Json(response))
            } else {
                let profit_percent = (profit as f64 / request.input_amount as f64) * 100.0;

                // Extract swap steps from forward route
                let mut forward_steps: Vec<SwapStep> = vec![];
                for path in &forward_route.paths {
                    for step in &path.steps {
                        forward_steps.push(SwapStep {
                            dex: step.dex,
                            input_token: step.input_token.to_string(),
                            output_token: step.output_token.to_string(),
                            pool_address: step.pool_address.to_string(),
                            input_amount: step.input_amount,
                            output_amount: step.output_amount,
                            percent: step.percent,
                        });
                    }
                }

                // Extract swap steps from reverse route
                let mut reverse_steps: Vec<SwapStep> = vec![];
                for path in &reverse_route.paths {
                    for step in &path.steps {
                        reverse_steps.push(SwapStep {
                            dex: step.dex,
                            input_token: step.input_token.to_string(),
                            output_token: step.output_token.to_string(),
                            pool_address: step.pool_address.to_string(),
                            input_amount: step.input_amount,
                            output_amount: step.output_amount,
                            percent: step.percent,
                        });
                    }
                }

                let time_taken_ms = start_time.elapsed().as_millis() as u64;
                let response = ArbitrageResponse {
                    profitable: true,
                    profit_amount: profit,
                    profit_percent,
                    forward_route: forward_steps,
                    reverse_route: reverse_steps,
                    forward_output: forward_route.output_amount,
                    reverse_output: reverse_route.output_amount,
                    time_taken_ms,
                    context_slot: forward_route.context_slot,
                };
                Ok(Json(response))
            }
        }
        None => {
            // No profitable arbitrage found
            let time_taken_ms = start_time.elapsed().as_millis() as u64;
            let response = ArbitrageResponse {
                profitable: false,
                profit_amount: 0,
                profit_percent: 0.0,
                forward_route: vec![],
                reverse_route: vec![],
                forward_output: 0,
                reverse_output: 0,
                time_taken_ms,
                context_slot: 0,
            };
            Ok(Json(response))
        }
    }
}

/// Get all monitored arbitrage tokens
pub async fn get_arbitrage_tokens(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ArbitrageTokensResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Get arbitrage config
    let arb_config = state.arbitrage_config.read().unwrap();

    let base_token = arb_config.get_base_token().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to get base token".to_string(),
                details: vec![e],
            }),
        )
    })?;

    // Get monitored tokens
    let monitored_tokens: Vec<ArbitrageTokenResponse> = arb_config
        .monitored_tokens
        .iter()
        .map(|t| ArbitrageTokenResponse {
            address: t.address.clone(),
            symbol: t.symbol.clone(),
            enabled: t.enabled,
        })
        .collect();

    Ok(Json(ArbitrageTokensResponse {
        base_token: base_token.to_string(),
        monitored_tokens,
    }))
}

/// Add a token to arbitrage monitoring
pub async fn add_arbitrage_token(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AddTokenRequest>,
) -> Result<Json<TokenOperationResponse>, (StatusCode, Json<ErrorResponse>)> {
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
                error: "Invalid request".to_string(),
                details,
            }),
        ));
    }

    // Parse token address
    let token_pubkey = parse_pubkey_with_error(&request.address, "token address")?;

    // Add to pool manager's monitored list
    state
        .aggregator
        .get_pool_manager()
        .add_arbitrage_token(token_pubkey)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Failed to add token".to_string(),
                    details: vec![e],
                }),
            )
        })?;

    // Also update the config (will be saved to DB by pool manager)
    let mut config = state.arbitrage_config.write().unwrap();
    if let Err(e) = config.add_token(request.symbol.clone(), request.address.clone()) {
        log::warn!(
            "Token added to pool manager but failed to update config: {}",
            e
        );
    }

    // Save updated config to DB
    let db = state.aggregator.get_pool_manager().get_db();
    if let Err(e) =
        crate::arbitrage_config::ArbitrageConfig::save_tokens_to_db(&db, &config.monitored_tokens)
    {
        log::error!("Failed to save config to DB: {}", e);
    }

    Ok(Json(TokenOperationResponse {
        success: true,
        message: format!("Token {} added to arbitrage monitoring", request.symbol),
        token: Some(ArbitrageTokenResponse {
            address: request.address,
            symbol: request.symbol,
            enabled: true,
        }),
    }))
}

/// Remove a token from arbitrage monitoring
pub async fn remove_arbitrage_token(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RemoveTokenRequest>,
) -> Result<Json<TokenOperationResponse>, (StatusCode, Json<ErrorResponse>)> {
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
                error: "Invalid request".to_string(),
                details,
            }),
        ));
    }

    // Parse token address
    let token_pubkey = parse_pubkey_with_error(&request.address, "token address")?;

    // Remove from pool manager's monitored list
    state
        .aggregator
        .get_pool_manager()
        .remove_arbitrage_token(&token_pubkey)
        .await
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Failed to remove token".to_string(),
                    details: vec![e],
                }),
            )
        })?;

    // Also update the config (will be saved to DB by pool manager)
    let mut config = state.arbitrage_config.write().unwrap();
    let removed_token = config.remove_token(&request.address).ok();

    // Save updated config to DB
    let db = state.aggregator.get_pool_manager().get_db();
    if let Err(e) =
        crate::arbitrage_config::ArbitrageConfig::save_tokens_to_db(&db, &config.monitored_tokens)
    {
        log::error!("Failed to save config to DB: {}", e);
    }

    Ok(Json(TokenOperationResponse {
        success: true,
        message: format!(
            "Token {} removed from arbitrage monitoring",
            request.address
        ),
        token: removed_token.map(|t| ArbitrageTokenResponse {
            address: t.address,
            symbol: t.symbol,
            enabled: t.enabled,
        }),
    }))
}

pub async fn get_token_pools(
    State(state): State<Arc<AppState>>,
    Path(token_address): Path<String>,
) -> Result<Json<crate::api::dto::TokenPoolsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let start_time = Instant::now();
    use crate::api::dto::TokenPoolInfo;

    // Parse token address
    let token_pubkey = parse_pubkey_with_error(&token_address, "token address")?;

    // Get pool manager
    let pool_manager = state.aggregator.get_pool_manager();

    // Get all pools containing this token (pass Pubkey)
    let pools_for_token = pool_manager.get_pools_for_token(&token_pubkey).await;

    // Get SOL price for price calculations via pool manager
    let sol_price = pool_manager.get_sol_price();

    // Get token decimals
    let input_token =
        get_token_with_error(&state.aggregator, &token_pubkey, &token_address, "Token").await?;

    let mut pools = Vec::new();

    // Iterate through pools containing this token
    for pool in pools_for_token {
        let (token_a, token_b) = pool.get_tokens();
        let paired_token = if token_a.to_string() == token_address {
            token_b
        } else {
            token_a
        };

        // Get paired token info
        let paired_token_info = state
            .aggregator
            .get_pool_manager()
            .get_token(&paired_token)
            .await;

        let paired_decimals = paired_token_info.map(|t| t.decimals).unwrap_or(6);

        // Calculate prices with correct decimals
        let (price_a, price_b) =
            pool.calculate_token_prices(sol_price, input_token.decimals, paired_decimals);

        let (token_price, paired_token_price) = if token_a.to_string() == token_address {
            (price_a, price_b)
        } else {
            (price_b, price_a)
        };

        let (reserve_a, reserve_b) = pool.get_reserves();
        let reserves = if token_a.to_string() == token_address {
            (reserve_a, reserve_b)
        } else {
            (reserve_b, reserve_a)
        };

        pools.push(TokenPoolInfo {
            pool_address: pool.address().to_string(),
            dex: pool.dex().to_string(),
            paired_token: paired_token.to_string(),
            token_price,
            paired_token_price,
            liquidity_usd: pool.get_liquidity_usd(),
            last_updated: pool.last_updated(),
            reserves,
        });
    }

    // Sort by liquidity descending
    pools.sort_by(|a, b| {
        b.liquidity_usd
            .partial_cmp(&a.liquidity_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_pools = pools.len();
    let time_taken_ms = start_time.elapsed().as_millis() as u64;

    Ok(Json(crate::api::dto::TokenPoolsResponse {
        token: token_address,
        pools,
        total_pools,
        time_taken_ms,
    }))
}
