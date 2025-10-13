use crate::bybit::client::BybitClient;
use crate::{FilterAddressType, PriceProvider, TokenPrice};
use anyhow::{Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::{future::try_join_all, SinkExt, StreamExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

// Bybit WebSocket endpoints
pub const BYBIT_WS_URL_SPOT: &str = "wss://stream.bybit.com/v5/public/spot";

#[derive(Debug, Deserialize, Serialize)]
struct TickerData {
    symbol: String,
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "bid1Price")]
    bid1_price: Option<String>,
    #[serde(rename = "ask1Price")]
    ask1_price: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TickerMessage {
    topic: String,
    #[serde(rename = "type")]
    msg_type: String,
    data: TickerData,
    ts: u64,
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    op: String,
    args: Vec<String>,
}

pub struct BybitService {
    client: BybitClient,
    price_cache: Arc<DashMap<String, TokenPrice>>, // Maps contract_address -> TokenPrice
    symbol_to_contract: Arc<DashMap<String, String>>, // Maps symbol -> contract_address
    contract_to_symbol: Arc<DashMap<String, String>>, // Maps contract_address -> symbol
    token_status_cache: Arc<DashMap<String, crate::TokenStatus>>, // symbol -> status
}

impl BybitService {
    /// Create service without authentication - will not filter by contract address
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: BybitClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
        }
    }

    /// Create service with authentication - can filter by contract address
    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
    ) -> Self {
        Self {
            client: BybitClient::with_credentials(address_type, api_key, api_secret),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
        }
    }

    async fn start_websocket_connection(
        connection_id: usize,
        symbols: &[String],
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
    ) -> Result<()> {
        let ws_url = BYBIT_WS_URL_SPOT;

        info!(
            "Connection {}: Connecting to Bybit WebSocket: {}",
            connection_id, ws_url
        );

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to Bybit WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to ticker streams
        // Bybit allows up to 10 args per subscription for spot
        const MAX_SYMBOLS_PER_SUBSCRIPTION: usize = 10;

        for chunk in symbols.chunks(MAX_SYMBOLS_PER_SUBSCRIPTION) {
            let topics: Vec<String> = chunk
                .iter()
                .map(|symbol| format!("tickers.{}", symbol))
                .collect();

            let subscribe_msg = SubscriptionRequest {
                op: "subscribe".to_string(),
                args: topics,
            };

            let msg_json = serde_json::to_string(&subscribe_msg)
                .context("Failed to serialize subscription message")?;

            info!(
                "Connection {}: Subscribing to {} symbols",
                connection_id,
                chunk.len()
            );

            let msg = WsMessage::Text(msg_json.into());
            if let Err(e) = write.send(msg).await {
                error!(
                    "Connection {}: Failed to send subscription: {}",
                    connection_id, e
                );
            }

            // Small delay between subscription batches
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Create a ping interval timer (Bybit recommends every 20 seconds)
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(20));
        ping_interval.tick().await; // Skip the first immediate tick

        // Handle incoming messages and periodic pings
        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Err(e) = Self::handle_text_message(
                                &text,
                                price_cache,
                                symbol_to_contract,
                                connection_id,
                            ) {
                                log::debug!(
                                    "Connection {}: Error handling message: {}",
                                    connection_id, e
                                );
                            }
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            log::debug!(
                                "Connection {}: Received binary message (length: {})",
                                connection_id,
                                data.len()
                            );
                        }
                        Some(Ok(WsMessage::Ping(data))) => {
                            log::debug!("Connection {}: Received ping, sending pong", connection_id);
                            if let Err(e) = write.send(WsMessage::Pong(data)).await {
                                error!(
                                    "Connection {}: Failed to send pong: {}",
                                    connection_id, e
                                );
                            }
                        }
                        Some(Ok(WsMessage::Pong(_))) => {
                            log::debug!("Connection {}: Received pong", connection_id);
                        }
                        Some(Ok(WsMessage::Close(frame))) => {
                            warn!(
                                "Connection {}: WebSocket connection closed: {:?}",
                                connection_id, frame
                            );
                            break;
                        }
                        Some(Ok(WsMessage::Frame(_))) => {
                            warn!("Connection {}: Received raw frame - unexpected", connection_id);
                        }
                        Some(Err(e)) => {
                            error!("Connection {}: WebSocket error: {}", connection_id, e);
                            break;
                        }
                        None => {
                            warn!("Connection {}: WebSocket stream ended", connection_id);
                            break;
                        }
                    }
                }
                // Send periodic ping
                _ = ping_interval.tick() => {
                    let ping_msg = serde_json::json!({
                        "op": "ping"
                    });
                    if let Err(e) = write.send(WsMessage::Text(ping_msg.to_string().into())).await {
                        error!("Connection {}: Failed to send ping: {}", connection_id, e);
                        break;
                    }
                    log::debug!("Connection {}: Sent ping", connection_id);
                }
            }
        }

        // Connection ended, return to allow reconnection in the loop
        warn!("Connection {}: WebSocket connection ended", connection_id);
        Ok(())
    }

    fn handle_text_message(
        text: &str,
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
        connection_id: usize,
    ) -> Result<()> {
        // Try to parse as TickerMessage
        if let Ok(ticker_msg) = serde_json::from_str::<TickerMessage>(text) {
            if ticker_msg.topic.starts_with("tickers.") {
                let symbol = ticker_msg.data.symbol.clone();

                // Check if we have contract address mapping for this symbol
                if let Some(contract_entry) = symbol_to_contract.get(&symbol) {
                    let contract_address = contract_entry.value().clone();

                    if let Ok(price) = f64::from_str(&ticker_msg.data.last_price) {
                        let token_price = TokenPrice {
                            symbol: symbol.clone(),
                            price,
                        };

                        // Store by contract address (lowercased for consistency)
                        price_cache.insert(contract_address.to_lowercase(), token_price);

                        log::debug!(
                            "Connection {}: Updated price for {} (contract: {}): ${}",
                            connection_id,
                            symbol,
                            contract_address,
                            price
                        );
                    }
                } else {
                    // No contract mapping - this means we're running without authentication
                    // Store by symbol as fallback (for testing without API keys)
                    if let Ok(price) = f64::from_str(&ticker_msg.data.last_price) {
                        let token_price = TokenPrice {
                            symbol: symbol.clone(),
                            price,
                        };

                        // Use symbol as key when no contract address available
                        price_cache.insert(symbol.to_lowercase(), token_price);
                    }
                }
            }
            return Ok(());
        }

        // Log subscription confirmations and other messages at debug level
        log::debug!("Connection {}: Received message: {}", connection_id, text);
        Ok(())
    }
}

#[async_trait]
impl PriceProvider for BybitService {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice> {
        self.price_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    async fn get_prices(&self, symbols: &Vec<String>) -> Vec<Option<TokenPrice>> {
        symbols
            .iter()
            .map(|symbol| {
                self.price_cache
                    .get(symbol)
                    .map(|entry| entry.value().clone())
            })
            .collect()
    }

    async fn get_all_prices(&self) -> Vec<TokenPrice> {
        self.price_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    async fn start(&self) -> Result<()> {
        // Initial token status refresh (with network and deposit verification)
        info!("Bybit: Performing initial token status verification...");
        let safe_market_symbols = match self.refresh_token_status().await {
            Ok(symbols) => {
                info!("Bybit: Successfully verified {} safe tokens", symbols.len());
                symbols
            }
            Err(e) => {
                warn!("Bybit: Initial token status refresh failed: {}", e);
                warn!("Bybit: Tip: Configure BYBIT_API_KEY and BYBIT_API_SECRET environment variables to enable deposit/network filtering");
                return Ok(());
            }
        };

        if safe_market_symbols.is_empty() {
            warn!("Bybit: No safe tokens to subscribe to after filtering");
            return Ok(());
        }

        info!("Bybit: Subscribing to {} verified safe tokens", safe_market_symbols.len());

        // Split symbols into chunks for multiple connections
        const MAX_SYMBOLS_PER_CONNECTION: usize = 50;
        let connection_chunks: Vec<Vec<String>> = safe_market_symbols
            .chunks(MAX_SYMBOLS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        info!(
            "Bybit: Creating {} WebSocket connections for {} markets",
            connection_chunks.len(),
            safe_market_symbols.len()
        );

        // Start multiple WebSocket connections concurrently
        let mut connection_handles = Vec::new();

        for (connection_id, chunk) in connection_chunks.into_iter().enumerate() {
            let price_cache = self.price_cache.clone();
            let symbol_to_contract = self.symbol_to_contract.clone();

            let handle = tokio::spawn(async move {
                loop {
                    info!(
                        "Bybit: Starting WebSocket connection {} for {} markets",
                        connection_id,
                        chunk.len()
                    );

                    if let Err(e) = Self::start_websocket_connection(
                        connection_id,
                        &chunk,
                        &price_cache,
                        &symbol_to_contract,
                    )
                    .await
                    {
                        error!("Bybit: WebSocket connection {} failed: {}", connection_id, e);
                        info!("Bybit: Reconnecting connection {} in 5 seconds...", connection_id);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }

                    info!(
                        "Bybit: WebSocket connection {} ended, reconnecting in 5 seconds...",
                        connection_id
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            });

            connection_handles.push(handle);
        }

        // Start a background task to refresh token status every 12 hours
        let refresh_service = Arc::new(Self {
            client: self.client.clone(),
            price_cache: self.price_cache.clone(),
            symbol_to_contract: self.symbol_to_contract.clone(),
            contract_to_symbol: self.contract_to_symbol.clone(),
            token_status_cache: self.token_status_cache.clone(),
        });
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(12 * 3600)); // 12 hours
            interval.tick().await; // Skip first immediate tick

            loop {
                interval.tick().await;
                info!("Bybit: Starting scheduled token status refresh (every 12 hours)...");
                if let Err(e) = refresh_service.refresh_token_status().await {
                    warn!("Bybit: Scheduled token status refresh failed: {}", e);
                }
            }
        });

        // Start statistics logging task
        let stats_price_cache = self.price_cache.clone();
        let stats_symbol_map = self.symbol_to_contract.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let token_count = stats_price_cache.len();
                let symbol_count = stats_symbol_map.len();

                info!(
                    "Bybit Service Stats - Tokens with prices: {}, Contracts mapped: {}",
                    token_count, symbol_count
                );
            }
        });

        // Wait for all connections
        let results: Result<Vec<_>, _> = try_join_all(connection_handles).await;
        results.context("One or more WebSocket connections failed")?;

        Ok(())
    }

    fn get_price_provider_name(&self) -> &'static str {
        "Bybit"
    }

    async fn is_token_safe_for_arbitrage(&self, symbol: &str, contract_address: Option<&str>) -> bool {
        let status = self.get_token_status(symbol, contract_address).await;
        match status {
            Some(status) => {
                status.is_trading && status.is_deposit_enabled && status.network_verified
            }
            None => false,
        }
    }

    async fn get_token_status(&self, symbol: &str, contract_address: Option<&str>) -> Option<crate::TokenStatus> {
        // Try to get from cache first
        if let Some(status) = self.token_status_cache.get(symbol) {
            return Some(status.clone());
        }

        // If not in cache and we have a contract address, try to verify it
        if let Some(contract_addr) = contract_address {
            // Normalize to lowercase for lookup (contract addresses are case-insensitive)
            let normalized_addr = contract_addr.to_lowercase();
            if let Some(market_symbol) = self.contract_to_symbol.get(&normalized_addr) {
                return self.token_status_cache.get(market_symbol.value()).map(|s| s.clone());
            }
        }

        None
    }

    async fn refresh_token_status(&self) -> Result<Vec<String>> {
        info!("Bybit: Refreshing token status cache...");

        // Get all trading pairs
        let instruments = self.client.get_token_usdt_pairs().await?;

        // Get coin information with network details (requires auth)
        let coin_info_result = self.client.get_coin_info(None).await;

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut verified_count = 0;
        let mut failed_count = 0;

        for instrument in instruments {
            let symbol = instrument.symbol.clone();
            let base_asset = instrument.base_coin.clone();

            // Default status: trading enabled (from instrument info)
            let mut status = crate::TokenStatus {
                symbol: symbol.clone(),
                base_asset: base_asset.clone(),
                contract_address: None,
                is_trading: instrument.status == "Trading",
                is_deposit_enabled: false,
                network_verified: false,
                last_updated: current_time,
            };

            // If we have coin info, verify deposit status and network
            if let Ok(ref coin_info_response) = coin_info_result {
                if let Some(coin_info) = coin_info_response.result.iter().find(|c| c.coin == base_asset) {
                    // Check if there's a chain that matches our requirements
                    for chain in &coin_info.chains {
                        let chain_name = chain.chain.to_uppercase();
                        let is_correct_chain = match self.client.address_type {
                            FilterAddressType::Ethereum => {
                                // Only accept ETH/ERC20 on Ethereum mainnet
                                chain_name == "ETH" ||
                                chain_name.contains("ETHEREUM") ||
                                (chain_name.contains("ERC20") && !chain_name.contains("ARB") && !chain_name.contains("POLYGON"))
                            }
                            FilterAddressType::Solana => {
                                // Only accept Solana network
                                chain_name == "SOL" || chain_name.contains("SOLANA")
                            }
                        };

                        if is_correct_chain && !chain.contract_address.is_empty() {
                            status.contract_address = Some(chain.contract_address.clone());
                            status.is_deposit_enabled = chain.is_deposit_enabled();
                            status.network_verified = true;

                            if status.is_trading && status.is_deposit_enabled && status.network_verified {
                                verified_count += 1;
                            }
                            break;
                        }
                    }
                }
            }

            if !status.network_verified {
                failed_count += 1;
                log::debug!(
                    "Bybit: Token {} ({}) - network verification failed or deposits disabled",
                    base_asset,
                    symbol
                );
            }

            // Store in cache
            self.token_status_cache.insert(symbol.clone(), status.clone());

            // If we have a verified contract address, populate the bidirectional mappings
            if let Some(ref contract_addr) = status.contract_address {
                if status.network_verified {
                    let normalized_contract = contract_addr.to_lowercase();
                    self.contract_to_symbol.insert(normalized_contract.clone(), symbol.clone());
                    self.symbol_to_contract.insert(symbol.clone(), normalized_contract);
                }
            }
        }

        info!(
            "Bybit: Token status refresh complete. Verified: {}, Failed: {}, Total: {}",
            verified_count,
            failed_count,
            verified_count + failed_count
        );

        // Return list of verified safe market symbols
        let safe_symbols: Vec<String> = self.token_status_cache
            .iter()
            .filter_map(|entry| {
                let status = entry.value();
                if status.is_trading && status.is_deposit_enabled && status.network_verified {
                    Some(status.symbol.clone())
                } else {
                    None
                }
            })
            .collect();

        info!("Bybit: Returning {} safe symbols for WebSocket subscription", safe_symbols.len());
        Ok(safe_symbols)
    }

    async fn get_deposit_address(&self, _symbol: &str, _address_type: crate::FilterAddressType) -> Result<String> {
        Err(anyhow::anyhow!("Bybit: get_deposit_address not yet implemented"))
    }

    async fn sell_token_for_usdt(&self, _symbol: &str, _amount: f64) -> Result<(String, f64, f64)> {
        Err(anyhow::anyhow!("Bybit: sell_token_for_usdt not yet implemented"))
    }

    async fn withdraw_usdt(&self, _address: &str, _amount: f64, _address_type: crate::FilterAddressType) -> Result<String> {
        Err(anyhow::anyhow!("Bybit: withdraw_usdt not yet implemented"))
    }

    async fn get_portfolio(&self) -> Result<crate::Portfolio> {
        self.get_portfolio_impl().await
    }
}

impl BybitService {
    /// Estimate how much USDT you'd get by selling a certain amount of tokens on Bybit
    /// Uses the orderbook to simulate market sell order
    ///
    /// `contract_address` - The Ethereum/Solana contract address (or symbol if running without auth)
    pub async fn estimate_sell_output(
        &self,
        contract_address: &str,
        token_amount: f64,
    ) -> Result<f64> {
        // Get the trading symbol for this contract address
        let symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())
            .context("Contract address not found in Bybit markets")?;

        // Fetch orderbook (bids = buy orders, we want to sell into these)
        let orderbook = self.client.get_orderbook(&symbol, 200).await?;

        let mut remaining_tokens = token_amount;
        let mut total_usdt = 0.0;

        // Iterate through bids (buy orders) from highest to lowest price
        for bid in orderbook.result.b {
            if remaining_tokens <= 0.0 {
                break;
            }

            let price: f64 = bid[0].parse().context("Failed to parse bid price")?;
            let quantity: f64 = bid[1].parse().context("Failed to parse bid quantity")?;

            let tokens_to_sell = remaining_tokens.min(quantity);
            total_usdt += tokens_to_sell * price;
            remaining_tokens -= tokens_to_sell;
        }

        if remaining_tokens > 0.0 {
            warn!(
                "Orderbook depth insufficient for {} {} (contract: {}), {} tokens remaining unsold",
                token_amount, symbol, contract_address, remaining_tokens
            );
        }

        Ok(total_usdt)
    }

    async fn get_portfolio_impl(&self) -> Result<crate::Portfolio> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut balances: Vec<crate::Balance> = Vec::new();
        let mut total_usdt_value = 0.0;

        // Step 1: Get UNIFIED account balance (main trading account)
        log::info!("Bybit: Checking UNIFIED account...");
        let account_types = vec!["UNIFIED"];

        for account_type in account_types {
            log::info!("Bybit: Checking {} account...", account_type);

            let account_data = match self.client.get_account_balance(account_type).await {
                Ok(data) => data,
                Err(e) => {
                    log::debug!("Bybit: Failed to get {} account balance: {}", account_type, e);
                    continue;
                }
            };



            // Parse Bybit response structure
            if let Some(result) = account_data.get("result") {
                if let Some(list) = result.get("list").and_then(|v| v.as_array()) {
                    for account in list {
                        if let Some(coins) = account.get("coin").and_then(|v| v.as_array()) {
                            for coin_data in coins {
                                let asset = coin_data
                                    .get("coin")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                let wallet_balance: f64 = coin_data
                                    .get("walletBalance")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0.0);

                                let locked: f64 = coin_data
                                    .get("locked")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0.0);

                                let coin_total = wallet_balance + locked;

                                // Get USD value directly from Bybit (more accurate)
                                let usd_value: f64 = coin_data
                                    .get("usdValue")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0.0);

                                // Only include non-zero balances
                                if coin_total > 0.0 {
                                    log::info!("Bybit: Found {} balance: {} (free: {}, locked: {}, USD value: ${})",
                                        asset, coin_total, wallet_balance, locked, usd_value);

                                    total_usdt_value += usd_value;

                                    // Check if we already have this asset in balances (from another account type)
                                    if let Some(existing_balance) = balances.iter_mut().find(|b| b.asset == asset) {
                                        // Aggregate balances from different account types
                                        existing_balance.free += wallet_balance;
                                        existing_balance.locked += locked;
                                        existing_balance.total += coin_total;
                                    } else {
                                        balances.push(crate::Balance {
                                            asset,
                                            free: wallet_balance,
                                            locked,
                                            total: coin_total,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Step 2: Get FUNDING account balance (separate wallet)
        log::info!("Bybit: Checking FUNDING account...");
        match self.client.get_funding_balance(None).await {
            Ok(funding_data) => {


                // Parse funding account response
                if let Some(result) = funding_data.get("result") {
                    if let Some(balance_list) = result.get("balance").and_then(|v| v.as_array()) {
                        for coin_data in balance_list {
                            let asset = coin_data
                                .get("coin")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let wallet_balance: f64 = coin_data
                                .get("walletBalance")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0.0);

                            let locked: f64 = coin_data
                                .get("locked")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0.0);

                            let coin_total = wallet_balance + locked;

                            if coin_total > 0.0 {
                                log::info!("Bybit FUNDING: Found {} balance: {} (free: {}, locked: {})",
                                    asset, coin_total, wallet_balance, locked);

                                // For funding account, estimate USD value
                                let usd_value = if asset == "USDT" {
                                    coin_total
                                } else {
                                    // Try to get price from our cache
                                    let symbol = format!("{}USDT", asset);
                                    if let Some(price_info) = self.get_price(&symbol.to_lowercase()).await {
                                        coin_total * price_info.price
                                    } else {
                                        log::debug!("No price found for {} in cache", symbol);
                                        0.0
                                    }
                                };

                                total_usdt_value += usd_value;

                                // Check if we already have this asset (from UNIFIED account)
                                if let Some(existing_balance) = balances.iter_mut().find(|b| b.asset == asset) {
                                    existing_balance.free += wallet_balance;
                                    existing_balance.locked += locked;
                                    existing_balance.total += coin_total;
                                } else {
                                    balances.push(crate::Balance {
                                        asset,
                                        free: wallet_balance,
                                        locked,
                                        total: coin_total,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::debug!("Bybit: Failed to get FUNDING account balance: {}", e);
            }
        }

        log::info!("Bybit: Portfolio summary - {} assets, total value: ${:.2} USDT",
            balances.len(), total_usdt_value);

        Ok(crate::Portfolio {
            exchange: "Bybit".to_string(),
            balances,
            total_usdt_value,
            timestamp: current_time,
        })
    }
}
