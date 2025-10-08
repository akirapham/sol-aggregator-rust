use anyhow::Result;
use axum::Router;
use dashmap::DashMap;
use dotenv::dotenv;
use log::{error, info};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower_http::cors::CorsLayer;
use tracing_subscriber;

mod api;
mod dex_price;
mod mexc;
mod types;

use dex_price::{DexPriceClient, DexPriceConfig};
use mexc::MexcService;

use crate::types::{PriceProvider, TokenPriceUpdate};

/// Simulate arbitrage opportunity and log results
async fn process_arbitrage(
    mexc_service: &Arc<MexcService>,
    update: &TokenPriceUpdate,
    cex_price: f64,
    cex_symbol: &str,
    arb_amount_usdt: f64,
    price_diff_percent: f64,
) {
    // Step 1: Calculate how many tokens we'd get on DEX with arb_amount_usdt
    let tokens_from_dex = arb_amount_usdt / update.price_in_usd;

    // Step 2: Estimate USDT output from selling tokens on MEXC
    match mexc_service
        .estimate_sell_output(&update.token_address.to_lowercase(), tokens_from_dex)
        .await
    {
        Ok(usdt_from_cex) => {
            let profit = usdt_from_cex - arb_amount_usdt;
            let profit_percent = (profit / arb_amount_usdt) * 100.0;

            log::info!(
                "🎯 ARBITRAGE OPPORTUNITY - Token: {}, Symbol: {}",
                update.token_address,
                cex_symbol
            );
            log::info!(
                "  DEX Price: ${:.6}, CEX Price: ${:.6}, Price Diff: {:.2}%",
                update.price_in_usd,
                cex_price,
                price_diff_percent
            );
            log::info!(
                "  Simulation: ${:.2} USDT → {:.6} tokens (DEX) → ${:.2} USDT (CEX)",
                arb_amount_usdt,
                tokens_from_dex,
                usdt_from_cex
            );
            log::info!(
                "  💰 Estimated Profit: ${:.2} USDT ({:.2}%)",
                profit,
                profit_percent
            );
        }
        Err(e) => {
            log::warn!(
                "Failed to estimate CEX sell for {}: {}",
                update.token_address,
                e
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting CEX Pricing Service");

    info!("Initializing MEXC service...");
    let mexc_service = Arc::new(MexcService::new().await?);
    info!("MEXC service initialized successfully");

    // Start the WebSocket service in background
    info!("Starting MEXC WebSocket service in background...");
    let mexc_service_clone = mexc_service.clone();
    tokio::spawn(async move {
        if let Err(e) = mexc_service_clone.start().await {
            error!("MEXC service error: {}", e);
        }
    });
    info!("MEXC WebSocket service started in background");

    // Example: Start DEX price client (if DEX_PRICE_STREAM environment variable is set)
    if std::env::var("DEX_PRICE_STREAM").is_ok() {
        let dex_config = DexPriceConfig::from_env();
        info!(
            "DEX_PRICE_STREAM is set, starting DEX price client with URL: {}",
            dex_config.websocket_url
        );

        let (dex_client, mut dex_receiver) = DexPriceClient::new(dex_config);

        // Start DEX client in background
        info!("Starting DEX client in background...");
        tokio::spawn(async move {
            if let Err(e) = dex_client.start().await {
                error!("DEX price client error: {}", e);
            }
        });
        info!("DEX client started in background");

        // Handle DEX price updates in background
        let mexc_service_clone = mexc_service.clone();
        tokio::spawn(async move {
            // Read minimum percentage difference threshold from environment
            let min_percent_diff: f64 = std::env::var("MIN_PERCENT_DIFF")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.0);

            // Read arbitrage simulation amount from environment (default: 400 USDT)
            let arb_amount_usdt: f64 = std::env::var("ARB_SIMULATION_USDT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(400.0);

            // Read minimum cooldown period between arbitrage attempts for same token (default: 3600 seconds)
            let arb_cooldown_secs: u64 = std::env::var("ARB_COOLDOWN_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600);

            // Cache to track last arbitrage processing time per token
            let arb_processing_cache: Arc<DashMap<String, u64>> = Arc::new(DashMap::new());

            while let Some(price_updates) = dex_receiver.recv().await {
                info!("Received {} DEX price updates", price_updates.len());

                for update in &price_updates {
                    // read token price from cex
                    let cex_price = mexc_service_clone
                        .get_price(&update.token_address.to_lowercase())
                        .await;
                    match cex_price {
                        Some(price) => {
                            let price_diff_percent =
                                ((update.price_in_usd - price.price) / price.price) * 100.0;
                            if price_diff_percent < -min_percent_diff {
                                let token_address = update.token_address.to_lowercase();
                                let current_time = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs();

                                // Check if we can process this token (not in cooldown)
                                let can_process = if let Some(last_time) =
                                    arb_processing_cache.get(&token_address)
                                {
                                    current_time - *last_time >= arb_cooldown_secs
                                } else {
                                    true
                                };

                                if can_process {
                                    // Mark token as being processed
                                    arb_processing_cache
                                        .insert(token_address.clone(), current_time);

                                    // Spawn task to process arbitrage
                                    let mexc_service = mexc_service_clone.clone();
                                    let update = update.clone();
                                    let symbol = price.symbol.clone();
                                    let cex_price = price.price;

                                    tokio::spawn(async move {
                                        process_arbitrage(
                                            &mexc_service,
                                            &update,
                                            cex_price,
                                            &symbol,
                                            arb_amount_usdt,
                                            price_diff_percent,
                                        )
                                        .await;
                                    });
                                } else {
                                    let last_time =
                                        arb_processing_cache.get(&token_address).unwrap();
                                    let time_remaining =
                                        arb_cooldown_secs - (current_time - *last_time);
                                    log::debug!(
                                        "Skipping arbitrage for {} - cooldown active ({} seconds remaining)",
                                        token_address,
                                        time_remaining
                                    );
                                }
                            }
                        }
                        None => {
                            // do nothing
                        }
                    }
                }

                // Here you can process the price updates:
                // - Store in database
                // - Update internal caches
                // - Forward to other services
                // - Calculate arbitrage opportunities, etc.
            }
        });

        info!("DEX price client started");
    } else {
        info!("DEX_PRICE_STREAM not set, skipping DEX price client");
    }

    // Start HTTP API server
    info!("Starting HTTP API server...");
    let app = Router::new()
        .merge(api::create_router(mexc_service))
        .layer(CorsLayer::permissive());

    info!("Binding to 0.0.0.0:3001...");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    info!("Successfully bound to 0.0.0.0:3001");

    info!("Starting axum server...");
    axum::serve(listener, app).await?;

    info!("Server shutdown gracefully");
    Ok(())
}
