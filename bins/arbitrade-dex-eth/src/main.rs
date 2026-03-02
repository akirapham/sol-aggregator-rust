use anyhow::Result;
use arbitrade_dex_eth::{ArbitrageDetector, ArbitrageHandler, DexWsClient, PriceCache};
use dashmap::DashMap;
use dotenv::dotenv;
use env_logger::Env;
use ethers::types::Address;
use log::{debug, error, info};
use std::env;
use std::str::FromStr;
use std::sync::Arc;

/// Configuration for arbitrade-dex-eth service
#[derive(Debug, Clone)]
struct Config {
    /// WebSocket URL of amm-eth service
    amm_eth_ws_url: String,
    /// HTTP API URL of amm-eth service for pair data
    dex_pair_api_url: String,
    /// Minimum profit percentage to trigger arbitrage
    min_profit_percent: f64,
    /// Minimum price difference in ETH
    min_price_diff_eth: f64,
    /// Interval (seconds) to check for opportunities
    check_interval_secs: u64,
    private_key: String,
    quote_router_address: Address,
    chain_name: String,
    rpc_url: String,
    slippage_tolerance: u16,
    dry_run: bool,
}

impl Config {
    fn from_env() -> Self {
        let amm_eth_ws_url =
            env::var("AMM_ETH_WS_URL").unwrap_or_else(|_| "ws://localhost:8080".to_string());

        let dex_pair_api_url =
            env::var("DEX_PAIR_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

        let min_profit_percent = env::var("MIN_PROFIT_PERCENT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);

        let check_interval_secs = env::var("CHECK_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let private_key = env::var("ETH_ARBITRADE_KEY").expect("ETH_ARBITRADE_KEY not set");

        let quote_router_address =
            env::var("QUOTE_ROUTER_ADDRESS").expect("QUOTE_ROUTER_ADDRESS not set");

        let chain_name = env::var("CHAIN_NAME").expect("CHAIN_NAME not set");

        let rpc_url = env::var("ETH_RPC_URL").expect("ETH_RPC_URL not set");

        let slippage_tolerance = env::var("SLIPPAGE_TOLERANCE_BPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100); // 1%

        let dry_run = env::var("DRY_RUN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        Config {
            amm_eth_ws_url,
            dex_pair_api_url,
            min_profit_percent,
            min_price_diff_eth: 0.0, // Deprecated: we use min_profit_percent only now
            check_interval_secs,
            private_key,
            quote_router_address: Address::from_str(&quote_router_address)
                .expect("Invalid Quote Router address"),
            chain_name,
            rpc_url,
            slippage_tolerance,
            dry_run,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    debug!("DEBUG: arbitrade-dex-eth main() started");

    // Load environment variables
    dotenv().ok();
    debug!("DEBUG: dotenv loaded");

    // Initialize logging
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    debug!("DEBUG: logger initialized");

    let config = Config::from_env();
    info!("🚀 Starting arbitrade-dex-eth service");
    info!(
        "📊 Configuration: min_profit={}%, check_interval={}s",
        config.min_profit_percent, config.check_interval_secs
    );
    info!(
        "🔗 Connecting to amm-eth WebSocket: {}",
        config.amm_eth_ws_url
    );

    // Load DEX configuration for base tokens and contract addresses
    let dex_config =
        eth_dex_quote::DexConfiguration::load().expect("Failed to load eth_dex_config.toml");
    let chain_config = dex_config
        .get_chain(&config.chain_name)
        .unwrap_or_else(|| panic!("Failed to get {} chain config", config.chain_name))
        .clone();

    // Parse base tokens (now tuples of (address, is_stable))
    let base_tokens: Vec<(ethers::types::Address, bool)> = chain_config
        .base_tokens
        .iter()
        .filter_map(|(addr, is_stable)| {
            addr.parse::<ethers::types::Address>()
                .ok()
                .map(|a| (a, *is_stable))
        })
        .collect();
    info!("📋 Loaded {} base tokens for flashloan", base_tokens.len());

    // Setup provider for on-chain queries
    let provider =
        ethers::providers::Provider::<ethers::providers::Http>::try_from(config.rpc_url)?;
    let provider = Arc::new(provider);
    info!("✅ Connected to Ethereum RPC");

    let quote_router_client = Arc::new(eth_dex_quote::quote_router::QuoteRouterClient::new(
        provider.clone(),
        config.quote_router_address,
    ));

    let executor = Arc::new(
        arbitrade_dex_eth::executor::ArbitrageExecutor::new(
            provider.clone(),
            config.private_key,
            config.quote_router_address,
            config.slippage_tolerance,
            config.dry_run,
            chain_config.chain_id,
        )
        .expect("Failed to create ArbitrageExecutor"),
    );

    // Create price cache
    let price_cache = Arc::new(PriceCache::new());
    debug!("DEBUG: Price cache created");

    // Create arbitrage detector
    let detector = Arc::new(ArbitrageDetector::new(
        price_cache.clone(),
        config.min_profit_percent,
        config.min_price_diff_eth,
    ));
    debug!("DEBUG: Arbitrage detector created");

    // Create WebSocket client
    let ws_client = DexWsClient::new(config.amm_eth_ws_url.clone());
    debug!("DEBUG: WebSocket client created");

    // Track opportunities detected
    let opportunities_detected = Arc::new(DashMap::new());

    let arbitrage_handler = Arc::new(ArbitrageHandler::new(
        price_cache.clone(),
        detector.clone(),
        opportunities_detected.clone(),
        provider.clone(),
        base_tokens.clone(),
        quote_router_client.clone(),
        chain_config.clone(),
        executor.clone(),
        config.dex_pair_api_url.clone(),
    ));

    tokio::spawn(async move {
        match ws_client.start_with_reconnect().await {
            Ok(mut rx) => {
                while let Some(price_update) = rx.recv().await {
                    debug!(
                        "Received price update: token={}, pool={}, price={}",
                        price_update.token_address,
                        price_update.pool_address,
                        price_update.price_in_eth
                    );
                    arbitrage_handler
                        .handle_price_update_for_arbitrage(&price_update)
                        .await;
                }
            }
            Err(e) => error!("WebSocket error: {}", e),
        }
    });
    info!("✅ WebSocket connected and listening for price updates");
    info!("🎯 Arbitrage detection is now REACTIVE - checks trigger on each price update");
    info!(
        "⚙️  Configuration: min_profit={}%",
        config.min_profit_percent
    );

    // Keep the service running
    // Opportunities are detected reactively as price updates arrive from WebSocket
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

        let stats = price_cache.get_stats();
        info!(
            "📊 Cache stats - {} tokens, {} pools, {} with multiple pools",
            stats.unique_tokens, stats.total_pools, stats.tokens_with_multiple_pools
        );
    }
}
