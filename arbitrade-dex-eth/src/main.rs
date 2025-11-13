use anyhow::Result;
use arbitrade_dex_eth::{ArbitrageDetector, DexWsClient, PriceCache};
use dashmap::DashMap;
use dotenv::dotenv;
use env_logger::Env;
use log::{error, info};
use std::env;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for arbitrade-dex-eth service
#[derive(Debug, Clone)]
struct Config {
    /// WebSocket URL of amm-eth service
    amm_eth_ws_url: String,
    /// Minimum profit percentage to trigger arbitrage
    min_profit_percent: f64,
    /// Minimum price difference in ETH
    min_price_diff_eth: f64,
    /// Interval (seconds) to check for opportunities
    check_interval_secs: u64,
}

impl Config {
    fn from_env() -> Self {
        let amm_eth_ws_url =
            env::var("AMM_ETH_WS_URL").unwrap_or_else(|_| "ws://localhost:8080".to_string());

        let min_profit_percent = env::var("MIN_PROFIT_PERCENT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.01);

        let min_price_diff_eth = env::var("MIN_PRICE_DIFF_ETH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.000001);

        let check_interval_secs = env::var("CHECK_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        Config {
            amm_eth_ws_url,
            min_profit_percent,
            min_price_diff_eth,
            check_interval_secs,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("DEBUG: arbitrade-dex-eth main() started");

    // Load environment variables
    dotenv().ok();
    eprintln!("DEBUG: dotenv loaded");

    // Initialize logging
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    eprintln!("DEBUG: logger initialized");

    let config = Config::from_env();
    info!("🚀 Starting arbitrade-dex-eth service");
    info!(
        "📊 Configuration: min_profit={}%, min_diff={} ETH, check_interval={}s",
        config.min_profit_percent, config.min_price_diff_eth, config.check_interval_secs
    );
    info!(
        "🔗 Connecting to amm-eth WebSocket: {}",
        config.amm_eth_ws_url
    );

    // Create price cache
    let price_cache = Arc::new(PriceCache::new());
    eprintln!("DEBUG: Price cache created");

    // Create WebSocket client
    let ws_client = DexWsClient::new(config.amm_eth_ws_url.clone());
    eprintln!("DEBUG: WebSocket client created");

    // Track opportunities detected
    let opportunities_detected = Arc::new(DashMap::new());
    let executions_attempted = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Start WebSocket listener in background
    let price_cache_clone = price_cache.clone();
    let ws_client_clone = ws_client.clone();
    tokio::spawn(async move {
        match ws_client_clone.start_with_reconnect().await {
            Ok(mut rx) => {
                while let Some(pool_price) = rx.recv().await {
                    price_cache_clone.update_price(pool_price);
                }
            }
            Err(e) => error!("WebSocket error: {}", e),
        }
    });
    price_cache.get_stats();

    // Wait for initial connection
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    info!("✅ WebSocket connected and listening for price updates");

    // Main arbitrage detection loop
    let detector = ArbitrageDetector::new(
        price_cache.clone(),
        config.min_profit_percent,
        config.min_price_diff_eth,
    );

    let mut check_interval =
        tokio::time::interval(tokio::time::Duration::from_secs(config.check_interval_secs));

    info!("🎯 Starting arbitrage detection loop");

    loop {
        check_interval.tick().await;
        // Get cache statistics
        let stats = price_cache.get_stats();
        if stats.total_pools == 0 {
            log::debug!("Waiting for price data... (0 pools)");
            continue;
        }

        // Find all opportunities
        let opportunities = detector.find_all_opportunities();
        if !opportunities.is_empty() {
            info!(
                "💰 Found {} arbitrage opportunity(ies) | Cache: {} tokens, {} pools",
                opportunities.len(),
                stats.unique_tokens,
                stats.total_pools
            );

            for opp in opportunities.iter().take(5) {
                info!(
                    "   🎯 {} | Profit: {:.6} ETH ({:.2}% of buy price)",
                    opp, opp.potential_profit_eth, opp.price_diff_percent
                );

                if let Some(usd_profit) = opp.potential_profit_usd {
                    info!("      💵 USD Equivalent: ${:.2}", usd_profit);
                }

                // Store opportunity in memory (for API/dashboard later)
                let opp_key = format!(
                    "{}_{}",
                    opp.token_address,
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                );
                opportunities_detected.insert(opp_key, opp.clone());

                // TODO: Execute trade if configured
                // match executor.execute(&opp).await {
                //     Ok(result) => info!("✅ Trade executed: {}", result.tx_hash),
                //     Err(e) => error!("❌ Trade execution failed: {}", e),
                // }
            }

            // Cleanup old opportunities (older than 1 hour)
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            opportunities_detected.retain(|_, opp| (now - opp.detected_at) < 3600);

            executions_attempted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else if stats.tokens_with_multiple_pools > 0 {
            log::debug!(
                "No profitable opportunities yet | {} tokens with multiple pools",
                stats.tokens_with_multiple_pools
            );
        }
    }
}
