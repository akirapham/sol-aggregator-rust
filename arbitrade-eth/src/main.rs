use anyhow::Result;
use axum::Router;
use cex_price_provider::bitget::BitgetService;
use cex_price_provider::bybit::BybitService;
use cex_price_provider::gate::GateService;
use cex_price_provider::kucoin::KucoinService;
use cex_price_provider::mexc::MexcService;
use cex_price_provider::PriceProvider;
use dashmap::DashMap;
use dotenv::dotenv;
use log::{error, info};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower_http::cors::CorsLayer;
use tracing_subscriber;
mod api;
mod arbitrage_api;
mod db;
mod dex_price;
mod kyber;
mod types;
use db::{ArbitrageDb, ArbitrageOpportunity};
use dex_price::{DexPriceClient, DexPriceConfig};
use kyber::KyberClient;
use std::env;

use crate::types::TokenPriceUpdate;

struct CexProvider {
    name: &'static str,
    service: Arc<dyn PriceProvider + Send + Sync>,
    // Store concrete service types for orderbook access
    mexc: Option<Arc<MexcService>>,
    bybit: Option<Arc<BybitService>>,
    kucoin: Option<Arc<KucoinService>>,
    bitget: Option<Arc<BitgetService>>,
    gate: Option<Arc<GateService>>,
}

/// Structure to hold CEX opportunity with liquidity info
struct CexOpportunity {
    cex_name: String,
    cex_price: f64,
    cex_symbol: String,
    price_diff_percent: f64,
    liquidity_usdt: f64, // Total USDT liquidity in orderbook
}

impl CexProvider {
    /// Calculate total orderbook liquidity (bid side) in USDT
    async fn get_orderbook_liquidity(
        &self,
        token_contract: &str,
        token_amount: f64,
    ) -> Option<f64> {
        // Try to estimate sell output which uses orderbook depth
        if let Some(mexc) = &self.mexc {
            return mexc
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(bybit) = &self.bybit {
            return bybit
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(kucoin) = &self.kucoin {
            return kucoin
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(bitget) = &self.bitget {
            return bitget
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(gate) = &self.gate {
            return gate
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        None
    }
}

/// Structure to hold the best arbitrage opportunity across CEXes
struct BestArbitrageOpportunity {
    cex_name: String,
    cex_price: f64,
    cex_symbol: String,
    price_diff_percent: f64,
    liquidity_usdt: f64,
    usdt_from_cex: f64,
    profit: f64,
    profit_percent: f64,
}

/// Simulate arbitrage opportunity and log results
async fn process_arbitrage(
    kyber_client: &KyberClient,
    cex_providers: &[CexProvider],
    update: &TokenPriceUpdate,
    arb_amount_usdt: f64,
    db: &Arc<ArbitrageDb>,
) {
    log::info!("Processing arbitrage for token: {}", update.token_address);
    // USDT contract address on Ethereum
    const USDT_ADDRESS: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";

    // Step 1: Get real quote from KyberSwap for USDT → Token swap
    // Convert USDT amount to wei (USDT has 6 decimals)
    let usdt_amount_wei = (arb_amount_usdt * 1_000_000.0) as u64;
    let gas_fee_usd: f64;

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
                    log::warn!(
                        "Failed to parse KyberSwap output amount for {}: {}",
                        update.token_address,
                        e
                    );
                    return;
                }
            }
        }
        Err(e) => {
            log::warn!(
                "Failed to get KyberSwap quote for {}: {}",
                update.token_address,
                e
            );
            return;
        }
    };

    // waiting 20s to simulate real-world delay for swap execution
    log::info!("Simulating swap execution delay...");
    tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;

    // waiting 16 confirmations on ethereum with block time ~13s
    log::info!("Waiting for 16 confirmations on Ethereum...");
    tokio::time::sleep(tokio::time::Duration::from_secs(16 * 13)).await;

    // Step 2: Check all CEXes and collect opportunities with good price differences
    let mut opportunities: Vec<CexOpportunity> = Vec::new();

    log::info!("Checking prices across all CEXes...");
    for cex in cex_providers {
        // Get price from CEX
        if let Some(price_info) = cex
            .service
            .get_price(&update.token_address.to_lowercase())
            .await
        {
            let price_diff_percent =
                ((update.price_in_usd - price_info.price) / price_info.price) * 100.0;

            // Check if there's a profitable opportunity (DEX price lower than CEX price)
            if price_diff_percent < 0.0 {
                log::debug!(
                    "  {} - Price: ${:.6}, Diff: {:.2}%",
                    cex.name,
                    price_info.price,
                    price_diff_percent
                );

                // Get orderbook liquidity for this CEX
                if let Some(liquidity) = cex
                    .get_orderbook_liquidity(&update.token_address.to_lowercase(), tokens_from_dex)
                    .await
                {
                    log::debug!(
                        "    {} - Orderbook liquidity: ${:.2} USDT",
                        cex.name,
                        liquidity
                    );

                    opportunities.push(CexOpportunity {
                        cex_name: cex.name.to_string(),
                        cex_price: price_info.price,
                        cex_symbol: price_info.symbol.clone(),
                        price_diff_percent,
                        liquidity_usdt: liquidity,
                    });
                } else {
                    log::debug!("    {} - Failed to get orderbook liquidity", cex.name);
                }
            }
        }
    }

    // Step 3: Pick the CEX with deepest liquidity (highest USDT output from orderbook)
    let best_opportunity = opportunities
        .into_iter()
        .max_by(|a, b| {
            a.liquidity_usdt
                .partial_cmp(&b.liquidity_usdt)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|opp| {
            let profit = opp.liquidity_usdt - arb_amount_usdt - gas_fee_usd;
            let profit_percent = (profit / (arb_amount_usdt + gas_fee_usd)) * 100.0;

            BestArbitrageOpportunity {
                cex_name: opp.cex_name,
                cex_price: opp.cex_price,
                cex_symbol: opp.cex_symbol,
                price_diff_percent: opp.price_diff_percent,
                liquidity_usdt: opp.liquidity_usdt,
                usdt_from_cex: opp.liquidity_usdt,
                profit,
                profit_percent,
            }
        });

    // Log and save the best opportunity
    if let Some(best) = best_opportunity {
        log::info!(
            "🎯 BEST ARBITRAGE OPPORTUNITY - Token: {}, CEX: {} (Deepest Liquidity), Symbol: {}",
            update.token_address,
            best.cex_name,
            best.cex_symbol
        );
        log::info!(
            "  DEX Price: ${:.6}, CEX Price: ${:.6}, Price Diff: {:.2}%",
            update.price_in_usd,
            best.cex_price,
            best.price_diff_percent
        );
        log::info!(
            "  Orderbook Depth: ${:.2} USDT available",
            best.liquidity_usdt
        );
        log::info!(
            "  Simulation: ${:.2} USDT → {:.6} tokens (KyberSwap) → ${:.2} USDT ({})",
            arb_amount_usdt,
            tokens_from_dex,
            best.usdt_from_cex,
            best.cex_name
        );
        log::info!(
            "  💰 Estimated Profit: ${:.2} USDT ({:.2}%)",
            best.profit,
            best.profit_percent
        );

        // Save to database
        let opportunity = ArbitrageOpportunity {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            token_address: update.token_address.clone(),
            token_symbol: update.token_address.clone(), // TODO: Get actual symbol
            dex_price: update.price_in_usd,
            cex_name: best.cex_name.clone(),
            cex_price: best.cex_price,
            cex_symbol: best.cex_symbol.clone(),
            price_diff_percent: best.price_diff_percent,
            liquidity_usdt: best.liquidity_usdt,
            profit_usdt: best.profit,
            profit_percent: best.profit_percent,
            arb_amount_usdt,
            tokens_from_dex,
            gas_fee_usd,
        };

        if let Err(e) = db.save_opportunity(&opportunity) {
            log::error!("Failed to save opportunity to database: {}", e);
        } else {
            log::debug!("Saved opportunity to database");
        }
    } else {
        log::info!(
            "No profitable arbitrage opportunity found across all CEXes for token: {}",
            update.token_address
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("DEBUG: main() started at {:?}", std::time::SystemTime::now());
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Load environment variables from .env file
    dotenv().ok();
    eprintln!("DEBUG: dotenv loaded");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Initialize logging
    tracing_subscriber::fmt::init();
    eprintln!("DEBUG: tracing initialized");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    info!("Starting CEX Pricing Service with Multi-CEX Support");
    eprintln!("INFO: Starting CEX Pricing Service with Multi-CEX Support");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Initialize all CEX services
    info!("Initializing CEX services...");
    let mexc_service = Arc::new(MexcService::new(
        cex_price_provider::FilterAddressType::Ethereum,
    ));

    // Initialize Bybit with or without credentials
    let bybit_service = match (env::var("BYBIT_API_KEY"), env::var("BYBIT_API_SECRET")) {
        (Ok(api_key), Ok(api_secret)) => {
            info!("Bybit API credentials found, initializing with authentication");
            Arc::new(BybitService::with_credentials(
                cex_price_provider::FilterAddressType::Ethereum,
                api_key,
                api_secret,
            ))
        }
        _ => {
            info!("Bybit API credentials not found, initializing without authentication (limited functionality)");
            Arc::new(BybitService::new(
                cex_price_provider::FilterAddressType::Ethereum,
            ))
        }
    };

    let kucoin_service = Arc::new(KucoinService::new(
        cex_price_provider::FilterAddressType::Ethereum,
    ));
    let bitget_service = Arc::new(BitgetService::new(
        cex_price_provider::FilterAddressType::Ethereum,
    ));
    let gate_service = Arc::new(GateService::new(
        cex_price_provider::FilterAddressType::Ethereum,
    ));
    info!("All CEX services initialized successfully");

    // Initialize RocksDB for storing arbitrage opportunities
    info!("Initializing arbitrage database...");
    let db_path = "rocksdb_data/arbitrade-eth";
    let arb_db = Arc::new(ArbitrageDb::open(db_path)?);
    info!("Arbitrage database initialized at {}", db_path);

    info!("Initializing KyberSwap client...");
    let kyber_client = Arc::new(KyberClient::new());
    info!("KyberSwap client initialized successfully");

    // Create a vector of CEX providers for arbitrage checking
    let cex_providers: Vec<CexProvider> = vec![
        CexProvider {
            name: "MEXC",
            service: mexc_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: Some(mexc_service.clone()),
            bybit: None,
            kucoin: None,
            bitget: None,
            gate: None,
        },
        CexProvider {
            name: "Bybit",
            service: bybit_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: Some(bybit_service.clone()),
            kucoin: None,
            bitget: None,
            gate: None,
        },
        CexProvider {
            name: "KuCoin",
            service: kucoin_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: None,
            kucoin: Some(kucoin_service.clone()),
            bitget: None,
            gate: None,
        },
        CexProvider {
            name: "Bitget",
            service: bitget_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: None,
            kucoin: None,
            bitget: Some(bitget_service.clone()),
            gate: None,
        },
        CexProvider {
            name: "Gate.io",
            service: gate_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: None,
            kucoin: None,
            bitget: None,
            gate: Some(gate_service.clone()),
        },
    ];
    let cex_providers = Arc::new(cex_providers);

    // Start all WebSocket services in background
    info!("Starting CEX WebSocket services in background...");

    let mexc_clone = mexc_service.clone();
    tokio::spawn(async move {
        info!("MEXC WebSocket task started");
        if let Err(e) = mexc_clone.start().await {
            error!("MEXC service error: {}", e);
        }
        error!("MEXC WebSocket task exited!");
    });

    let bybit_clone = bybit_service.clone();
    tokio::spawn(async move {
        info!("Bybit WebSocket task started");
        if let Err(e) = bybit_clone.start().await {
            error!("Bybit service error: {}", e);
        }
        error!("Bybit WebSocket task exited!");
    });

    let kucoin_clone = kucoin_service.clone();
    tokio::spawn(async move {
        info!("KuCoin WebSocket task started");
        if let Err(e) = kucoin_clone.start().await {
            error!("KuCoin service error: {}", e);
        }
        error!("KuCoin WebSocket task exited!");
    });

    let bitget_clone = bitget_service.clone();
    tokio::spawn(async move {
        info!("Bitget WebSocket task started");
        if let Err(e) = bitget_clone.start().await {
            error!("Bitget service error: {}", e);
        }
        error!("Bitget WebSocket task exited!");
    });

    let gate_clone = gate_service.clone();
    tokio::spawn(async move {
        info!("Gate.io WebSocket task started");
        if let Err(e) = gate_clone.start().await {
            error!("Gate.io service error: {}", e);
        }
        error!("Gate.io WebSocket task exited!");
    });

    info!("All CEX WebSocket services started in background");

    // Give CEX services time to initialize their connections
    info!("Waiting 10 seconds for CEX services to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    info!("CEX initialization period complete"); // Example: Start DEX price client (if DEX_PRICE_STREAM environment variable is set)
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
            info!("DEX price client task started");
            if let Err(e) = dex_client.start().await {
                error!("DEX price client error: {}", e);
            }
            error!("DEX price client task exited!");
        });
        info!("DEX client started in background");

        // Handle DEX price updates in background
        let cex_providers_clone = cex_providers.clone();
        let kyber_client_clone = kyber_client.clone();
        let arb_db = arb_db.clone();
        tokio::spawn(async move {
            // Read minimum percentage difference threshold from environment
            let min_percent_diff: f64 = std::env::var("MIN_PERCENT_DIFF")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.0);
            log::info!(
                "Using minimum percentage difference threshold: {:.2}%",
                min_percent_diff
            );

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
                    // Check if any CEX has this token at a better price
                    let mut has_opportunity = false;
                    for cex in cex_providers_clone.iter() {
                        if let Some(price) = cex
                            .service
                            .get_price(&update.token_address.to_lowercase())
                            .await
                        {
                            let price_diff_percent =
                                ((update.price_in_usd - price.price) / price.price) * 100.0;
                            if price_diff_percent < -min_percent_diff {
                                has_opportunity = true;
                                break;
                            }
                        }
                    }

                    if has_opportunity {
                        let token_address = update.token_address.to_lowercase();
                        let current_time = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        // Check if we can process this token (not in cooldown)
                        let can_process =
                            if let Some(last_time) = arb_processing_cache.get(&token_address) {
                                current_time - *last_time >= arb_cooldown_secs
                            } else {
                                true
                            };

                        if can_process {
                            // Step 1: Quick profit estimation before full simulation
                            // Get estimated DEX output and check if any CEX offers positive profit
                            let kyber_client = kyber_client_clone.clone();
                            let cex_providers = cex_providers_clone.clone();
                            let update = update.clone();
                            let db = arb_db.clone();
                            let token_address_clone = token_address.clone();
                            let arb_cache = arb_processing_cache.clone();

                            tokio::spawn(async move {
                                const USDT_ADDRESS: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
                                let usdt_amount_wei = (arb_amount_usdt * 1_000_000.0) as u64;

                                // Step 1: Get price differences across all CEXes
                                log::debug!("Step 1: Checking price differences for {}", update.token_address);
                                let mut has_price_opportunity = false;
                                for cex in cex_providers.iter() {
                                    if let Some(price) = cex
                                        .service
                                        .get_price(&update.token_address.to_lowercase())
                                        .await
                                    {
                                        let price_diff_percent =
                                            ((update.price_in_usd - price.price) / price.price) * 100.0;
                                        if price_diff_percent < 0.0 {
                                            has_price_opportunity = true;
                                            log::debug!(
                                                "  {} has favorable price: ${:.6} (diff: {:.2}%)",
                                                cex.name,
                                                price.price,
                                                price_diff_percent
                                            );
                                        }
                                    }
                                }

                                if !has_price_opportunity {
                                    log::debug!("No favorable price differences found, skipping arbitrage");
                                    return;
                                }

                                // Step 2: Get estimated output from Kyber aggregator for DEX swap
                                log::debug!("Step 2: Getting Kyber swap estimate for {} USDT", arb_amount_usdt);
                                let (tokens_from_dex, gas_fee_usd) = match kyber_client
                                    .estimate_swap_output(
                                        USDT_ADDRESS,
                                        &update.token_address,
                                        &usdt_amount_wei.to_string(),
                                    )
                                    .await
                                {
                                    Ok((amount_out_wei, gas_usd_str)) => {
                                        let gas_usd = gas_usd_str.parse::<f64>().unwrap_or(0.0);
                                        match amount_out_wei.parse::<u128>() {
                                            Ok(wei) => {
                                                let divisor = 10_u128.pow(update.decimals as u32);
                                                let tokens = wei as f64 / divisor as f64;
                                                log::debug!(
                                                    "  Kyber estimate: {} USDT → {:.6} tokens (gas: ${:.2})",
                                                    arb_amount_usdt,
                                                    tokens,
                                                    gas_usd
                                                );
                                                (tokens, gas_usd)
                                            }
                                            Err(e) => {
                                                log::warn!("Failed to parse Kyber output amount: {}", e);
                                                return;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("Failed to get Kyber quote: {}", e);
                                        return;
                                    }
                                };

                                // Step 3: Estimate output USDT from each CEX using their orderbook
                                log::debug!("Step 3: Estimating CEX outputs for {:.6} tokens", tokens_from_dex);
                                let mut best_profit: Option<f64> = None;
                                let mut best_cex_name: Option<String> = None;

                                for cex in cex_providers.iter() {
                                    if let Some(usdt_output) = cex
                                        .get_orderbook_liquidity(&update.token_address.to_lowercase(), tokens_from_dex)
                                        .await
                                    {
                                        let profit = usdt_output - arb_amount_usdt - gas_fee_usd;
                                        log::debug!(
                                            "  {} estimated output: ${:.2} USDT → Profit: ${:.2}",
                                            cex.name,
                                            usdt_output,
                                            profit
                                        );

                                        if profit > best_profit.unwrap_or(0.0) {
                                            best_profit = Some(profit);
                                            best_cex_name = Some(cex.name.to_string());
                                        }
                                    }
                                }

                                // Step 4: Only proceed with full arbitrage simulation if we have positive profit
                                if let Some(profit) = best_profit {
                                    if profit > 0.0 {
                                        log::info!(
                                            "✅ Positive profit potential detected for {}: ${:.2} on {}",
                                            update.token_address,
                                            profit,
                                            best_cex_name.unwrap_or_default()
                                        );
                                        log::info!("Proceeding with full arbitrage simulation...");

                                        // Mark token as being processed
                                        arb_cache.insert(token_address_clone, current_time);

                                        // Run full arbitrage simulation with delays
                                        process_arbitrage(
                                            &kyber_client,
                                            &cex_providers,
                                            &update,
                                            arb_amount_usdt,
                                            &db,
                                        )
                                        .await;
                                    } else {
                                        log::debug!(
                                            "❌ No positive profit for {}: best profit ${:.2}, skipping",
                                            update.token_address,
                                            profit
                                        );
                                    }
                                } else {
                                    log::debug!("❌ No CEX liquidity found for {}, skipping", update.token_address);
                                }
                            });
                        } else {
                            let last_time = arb_processing_cache.get(&token_address).unwrap();
                            let time_remaining = arb_cooldown_secs - (current_time - *last_time);
                            log::debug!(
                                "Skipping arbitrage for {} - cooldown active ({} seconds remaining)",
                                token_address,
                                time_remaining
                            );
                        }
                    }
                }
            }
        });

        info!("DEX price client started");
    } else {
        info!("DEX_PRICE_STREAM not set, skipping DEX price client");
    }

    // Start HTTP API server
    info!("Starting HTTP API server...");
    eprintln!("DEBUG: Starting HTTP API server...");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    let app = Router::new()
        .merge(api::create_router(mexc_service))
        .merge(arbitrage_api::create_router(arb_db.clone()))
        .layer(CorsLayer::permissive());

    // Read port from ARBITRADE_PORT environment variable, default to 3001
    let port = env::var("ARBITRADE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(3001);
    let bind_addr = format!("0.0.0.0:{}", port);

    info!("Binding to {}...", bind_addr);
    eprintln!("DEBUG: Binding to {}...", bind_addr);
    std::io::Write::flush(&mut std::io::stderr()).ok();

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => {
            info!("Successfully bound to {}", bind_addr);
            eprintln!("DEBUG: Successfully bound to {}", bind_addr);
            std::io::Write::flush(&mut std::io::stderr()).ok();
            l
        }
        Err(e) => {
            error!("Failed to bind to {}: {}", bind_addr, e);
            eprintln!("ERROR: Failed to bind to {}: {}", bind_addr, e);
            std::io::Write::flush(&mut std::io::stderr()).ok();
            return Err(e.into());
        }
    };

    info!("Starting axum server...");
    eprintln!("DEBUG: Starting axum server, entering main loop...");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Keep the main function alive by running the server
    match axum::serve(listener, app).await {
        Ok(_) => {
            info!("Server shutdown gracefully");
            eprintln!("DEBUG: Server shutdown gracefully");
            std::io::Write::flush(&mut std::io::stderr()).ok();
            Ok(())
        }
        Err(e) => {
            error!("Axum server error: {}", e);
            eprintln!("ERROR: Axum server error: {}", e);
            std::io::Write::flush(&mut std::io::stderr()).ok();
            Err(e.into())
        }
    }
}
