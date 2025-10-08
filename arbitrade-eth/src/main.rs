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
mod kyber;
mod mexc;
mod types;

use dex_price::{DexPriceClient, DexPriceConfig};
use kyber::KyberClient;
use mexc::MexcService;

use crate::types::{PriceProvider, TokenPriceUpdate};

/// Simulate arbitrage opportunity and log results
async fn process_arbitrage(
    kyber_client: &KyberClient,
    mexc_service: &Arc<MexcService>,
    update: &TokenPriceUpdate,
    cex_price: f64,
    cex_symbol: &str,
    arb_amount_usdt: f64,
    price_diff_percent: f64,
) {
    log::info!(
        "Processing arbitrage for token: {}, symbol: {} with price difference: {:.2}%",
        update.token_address,
        cex_symbol,
        price_diff_percent
    );
    // USDT contract address on Ethereum
    const USDT_ADDRESS: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";

    // Step 1: Get real quote from KyberSwap for USDT → Token swap
    // Convert USDT amount to wei (USDT has 6 decimals)
    let usdt_amount_wei = (arb_amount_usdt * 1_000_000.0) as u64;
    let mut gas_fee_usd: f64 = 0.0;

    let tokens_from_dex = match kyber_client
        .estimate_swap_output(
            USDT_ADDRESS,
            &update.token_address,
            &usdt_amount_wei.to_string(),
        )
        .await
    {
        Ok((amount_out_wei, gas_usd_str)) => {
            gas_fee_usd = gas_usd_str.parse::<f64>().unwrap_or(0.0);
            // Convert from wei to token amount using decimals
            match amount_out_wei.parse::<u128>() {
                Ok(wei) => {
                    let divisor = 10_u128.pow(update.decimals as u32);
                    wei as f64 / divisor as f64
                }
                Err(e) => {
                    log::warn!("Failed to parse KyberSwap output amount for {}: {}", update.token_address, e);
                    return;
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to get KyberSwap quote for {}: {}", update.token_address, e);
            return;
        }
    };

    // waiting 20s to simulate real-world delay for swap execution
    log::info!("Simulating swap execution delay...");
    tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;

    // waiting 16 confirmations on ethereum with block time ~13s
    log::info!("Waiting for 16 confirmations on Ethereum...");
    tokio::time::sleep(tokio::time::Duration::from_secs(16 * 13)).await;

    // Step 2: Estimate USDT output from selling tokens on MEXC
    match mexc_service
        .estimate_sell_output(&update.token_address.to_lowercase(), tokens_from_dex)
        .await
    {
        Ok(usdt_from_cex) => {
            let profit = usdt_from_cex - arb_amount_usdt - gas_fee_usd;
            let profit_percent = (profit / (arb_amount_usdt + gas_fee_usd)) * 100.0;

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
                "  Simulation: ${:.2} USDT → {:.6} tokens (KyberSwap) → ${:.2} USDT (MEXC)",
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

    info!("Initializing KyberSwap client...");
    let kyber_client = Arc::new(KyberClient::new());
    info!("KyberSwap client initialized successfully");

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
        let kyber_client_clone = kyber_client.clone();
        tokio::spawn(async move {
            // Read minimum percentage difference threshold from environment
            let min_percent_diff: f64 = std::env::var("MIN_PERCENT_DIFF")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.0);
            log::info!("Using minimum percentage difference threshold: {:.2}%", min_percent_diff);

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
                                    let kyber_client = kyber_client_clone.clone();
                                    let mexc_service = mexc_service_clone.clone();
                                    let update = update.clone();
                                    let symbol = price.symbol.clone();
                                    let cex_price = price.price;

                                    tokio::spawn(async move {
                                        process_arbitrage(
                                            &kyber_client,
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
