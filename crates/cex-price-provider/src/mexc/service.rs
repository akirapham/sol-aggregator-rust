use crate::mexc::client::MexcClient;
use crate::{FilterAddressType, PriceProvider, TokenPrice};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use axum::body::Bytes;
use dashmap::DashMap;
use futures_util::{future::try_join_all, SinkExt, StreamExt};
use log::{error, info, warn};
use mexc_proto::push_data_v3_api_wrapper::Body;
use mexc_proto::PushDataV3ApiWrapper;
use prost::Message;
use std::str::FromStr;
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

pub const MEXC_WS_URL: &str = "wss://wbs-api.mexc.com/ws";
pub const MEXC_TRADE_STREAM_PREFIX: &str = "spot@public.aggre.deals.v3.api.pb@100ms@";

pub struct MexcService {
    client: MexcClient,
    price_cache: Arc<DashMap<String, TokenPrice>>,
    market_symbol_to_contract: Arc<DashMap<String, String>>,
    contract_to_market_symbol: Arc<DashMap<String, String>>,
    token_status_cache: Arc<DashMap<String, crate::TokenStatus>>, // symbol -> status
    symbol_precision_cache: Arc<DashMap<String, u32>>, // base_asset -> quantity_precision
}

impl MexcService {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: MexcClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            market_symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_market_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
            symbol_precision_cache: Arc::new(DashMap::new()),
        }
    }

    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
    ) -> Self {
        Self {
            client: MexcClient::with_credentials(address_type, api_key, api_secret),
            price_cache: Arc::new(DashMap::new()),
            market_symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_market_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
            symbol_precision_cache: Arc::new(DashMap::new()),
        }
    }

    async fn start_websocket_connection(
        connection_id: usize,
        pairs: &[String],
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        market_symbol_to_contract: &Arc<DashMap<String, String>>,
    ) -> Result<()> {
        let ws_url = MEXC_WS_URL;

        info!(
            "Connection {}: Connecting to MEXC WebSocket: {}",
            connection_id, ws_url
        );

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to MEXC WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to ticker streams for all pairs in this connection
        const MAX_STREAMS_PER_SUBSCRIPTION: usize = 15;

        for chunk in pairs.chunks(MAX_STREAMS_PER_SUBSCRIPTION) {
            let stream_names: Vec<String> = chunk
                .iter()
                .map(|pair| format!("{}{}", MEXC_TRADE_STREAM_PREFIX, pair.clone()))
                .collect();
            let subscribe_msg = serde_json::json!({
                "method": "SUBSCRIPTION",
                "params": stream_names
            });

            let msg = WsMessage::Text(subscribe_msg.to_string().into());
            if let Err(e) = write.send(msg).await {
                error!(
                    "Connection {}: Failed to send batch subscription: {}",
                    connection_id, e
                );
            }

            // Small delay between subscription batches
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Create a ping interval timer
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(20));
        ping_interval.tick().await; // Skip the first immediate tick

        // Handle incoming messages and periodic pings
        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            // Handle text messages if needed
                            log::debug!("Connection {}: Received text message: {}", connection_id, text);
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            if let Err(e) = Self::handle_protobuf_message(
                                &data,
                                price_cache,
                                market_symbol_to_contract,
                                connection_id,
                            ) {
                                error!(
                                    "Connection {}: Error handling protobuf message: {}",
                                    connection_id, e
                                );
                            }
                        }
                        Some(Ok(WsMessage::Ping(data))) => {
                            info!("Connection {}: Received ping, sending pong", connection_id);
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
                    if let Err(e) = write.send(WsMessage::Ping(Bytes::new())).await {
                        error!("Connection {}: Failed to send ping: {}", connection_id, e);
                        break;
                    }
                }
            }
        }

        // Connection ended, return to allow reconnection in the loop
        warn!("Connection {}: WebSocket connection ended", connection_id);
        Ok(())
    }

    fn handle_protobuf_message(
        data: &[u8],
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        market_symbol_to_contract: &Arc<DashMap<String, String>>,
        connection_id: usize,
    ) -> Result<()> {
        match PushDataV3ApiWrapper::decode(data) {
            Ok(message) => {
                let market_symbol = message.symbol.clone().unwrap_or_default();
                if let Some(contract_address) = market_symbol_to_contract.get(&market_symbol) {
                    if let Some(push_data) = message.body { match push_data {
                        Body::PublicAggreDeals(item) => {
                            if let Some(deal) = item.deals.first() {
                                let price = TokenPrice {
                                    symbol: market_symbol
                                        .strip_suffix("USDT")
                                        .unwrap_or(&market_symbol)
                                        .to_string(),
                                    price: f64::from_str(&deal.price).unwrap_or(0.0),
                                };
                                price_cache.insert(contract_address.value().clone(), price);
                            }
                        }
                    } }
                }

                Ok(())
            }
            Err(e) => {
                log::warn!(
                    "Connection {}: Failed to decode protobuf message: {}",
                    connection_id,
                    e
                );
                // Log first 200 bytes in hex for debugging
                log::debug!(
                    "Connection {}: Failed decode - Raw data (first 200 bytes): {:02x?}",
                    connection_id,
                    &data[..data.len().min(200)]
                );

                // Try to decode as UTF-8 string to see if it's actually JSON
                if let Ok(text) = std::str::from_utf8(data) {
                    log::debug!(
                        "Connection {}: Data as UTF-8 string: {}",
                        connection_id,
                        text
                    );
                } else {
                    log::debug!("Connection {}: Data is not valid UTF-8", connection_id);
                }

                Err(anyhow!("Protobuf decode error: {}", e))
            }
        }
    }

    /// Get current ticker price directly from REST API (useful when WebSocket is not running)
    pub async fn get_ticker_price(&self, symbol: &str) -> Result<f64> {
        self.client.get_ticker_price(symbol).await
    }
}

#[async_trait]
impl PriceProvider for MexcService {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice> {
        self.price_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    async fn get_prices(&self, mints: &Vec<String>) -> Vec<Option<TokenPrice>> {
        mints
            .iter()
            .map(|mint| {
                self.price_cache
                    .get(mint)
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
        info!("MEXC: Performing initial token status verification...");
        let safe_market_symbols = match self.refresh_token_status().await {
            Ok(symbols) => {
                info!("MEXC: Successfully verified {} safe tokens", symbols.len());
                symbols
            }
            Err(e) => {
                warn!("MEXC: Initial token status refresh failed: {}", e);
                warn!("Tip: Configure MEXC_API_KEY and MEXC_API_SECRET environment variables to enable deposit/network filtering");
                return Ok(());
            }
        };

        if safe_market_symbols.is_empty() {
            warn!("MEXC: No safe tokens to subscribe to after filtering");
            return Ok(());
        }

        info!(
            "MEXC: Subscribing to {} verified safe tokens",
            safe_market_symbols.len()
        );

        // Split symbols into chunks for multiple WebSocket connections
        const MAX_STREAMS_PER_CONNECTION: usize = 15; // Using 15 instead of 30 for safety margin
        let connection_chunks: Vec<Vec<String>> = safe_market_symbols
            .chunks(MAX_STREAMS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        info!(
            "MEXC: Creating {} WebSocket connections for {} markets",
            connection_chunks.len(),
            safe_market_symbols.len()
        );

        // Start multiple WebSocket connections concurrently
        let mut connection_handles = Vec::new();

        for (connection_id, chunk) in connection_chunks.into_iter().enumerate() {
            let price_cache = self.price_cache.clone();
            let market_symbol_to_contract = self.market_symbol_to_contract.clone();

            let handle = tokio::spawn(async move {
                loop {
                    info!(
                        "Starting WebSocket connection {} for {} markets",
                        connection_id,
                        chunk.len()
                    );

                    if let Err(e) = Self::start_websocket_connection(
                        connection_id,
                        &chunk,
                        &price_cache,
                        &market_symbol_to_contract,
                    )
                    .await
                    {
                        error!("WebSocket connection {} failed: {}", connection_id, e);
                        info!("Reconnecting connection {} in 5 seconds...", connection_id);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }

                    info!(
                        "WebSocket connection {} ended, reconnecting in 5 seconds...",
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
            market_symbol_to_contract: self.market_symbol_to_contract.clone(),
            contract_to_market_symbol: self.contract_to_market_symbol.clone(),
            token_status_cache: self.token_status_cache.clone(),
            symbol_precision_cache: self.symbol_precision_cache.clone(),
        });
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(12 * 3600)); // 12 hours
            interval.tick().await; // Skip first immediate tick

            loop {
                interval.tick().await;
                info!("MEXC: Starting scheduled token status refresh (every 12 hours)...");
                if let Err(e) = refresh_service.refresh_token_status().await {
                    warn!("MEXC: Scheduled token status refresh failed: {}", e);
                }
            }
        });

        // Start a background task to log statistics periodically
        let stats_price_cache = self.price_cache.clone();
        let stats_market_map = self.market_symbol_to_contract.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let token_count = stats_price_cache.len();
                let market_count = stats_market_map.len();

                info!(
                    "MEXC Service Stats - Tokens: {}, Markets: {}",
                    token_count, market_count
                );
            }
        });

        // Wait for all connections (they should run indefinitely)
        let results: Result<Vec<_>, _> = try_join_all(connection_handles).await;
        results.context("One or more WebSocket connections failed")?;

        Ok(())
    }

    fn get_price_provider_name(&self) -> &'static str {
        "MEXC"
    }

    async fn is_token_safe_for_arbitrage(
        &self,
        symbol: &str,
        contract_address: Option<&str>,
    ) -> bool {
        let status = self.get_token_status(symbol, contract_address).await;
        match status {
            Some(status) => {
                status.is_trading && status.is_deposit_enabled && status.network_verified
            }
            None => false,
        }
    }

    async fn get_token_status(
        &self,
        symbol: &str,
        contract_address: Option<&str>,
    ) -> Option<crate::TokenStatus> {
        // Try to get from cache first
        if let Some(status) = self.token_status_cache.get(symbol) {
            return Some(status.clone());
        }

        // If not in cache and we have a contract address, try to verify it
        if let Some(contract_addr) = contract_address {
            // Normalize to lowercase for lookup (contract addresses are case-insensitive)
            let normalized_addr = contract_addr.to_lowercase();
            if let Some(market_symbol) = self.contract_to_market_symbol.get(&normalized_addr) {
                return self
                    .token_status_cache
                    .get(market_symbol.value())
                    .map(|s| s.clone());
            }
        }

        None
    }

    async fn refresh_token_status(&self) -> Result<Vec<String>> {
        info!("MEXC: Refreshing token status cache...");

        // Get all trading pairs
        let symbols = self.client.get_token_usdt_pairs().await?;
        let mut safe_symbols = Vec::new();

        // Get coin information with network details (requires auth)
        let coin_info_result = self.client.get_coin_info(None).await;

        match &coin_info_result {
            Ok(infos) => {
                info!(
                    "MEXC: Successfully fetched coin info for {} coins",
                    infos.len()
                );
            }
            Err(e) => {
                warn!("MEXC: Failed to fetch coin info (auth required): {}", e);
                warn!("MEXC: Without coin info, cannot verify deposit networks - all tokens will be marked as unsafe");
            }
        }

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut verified_count = 0;
        let mut failed_count = 0;

        for symbol_info in symbols {
            let market_symbol = format!("{}{}", symbol_info.base_asset, symbol_info.quote_asset);
            let base_asset = symbol_info.base_asset.clone();

            // Default status: trading enabled (from exchange info), but need to verify deposits
            // Normalize contract address to lowercase for consistency
            let normalized_contract = symbol_info.contract_address.to_lowercase();
            let mut status = crate::TokenStatus {
                symbol: market_symbol.clone(),
                base_asset: base_asset.clone(),
                contract_address: Some(normalized_contract.clone()),
                is_trading: symbol_info.status == "1",
                is_deposit_enabled: false,
                network_verified: false,
                last_updated: current_time,
            };

            // If we have coin info, verify deposit status and network
            if let Ok(ref coin_infos) = coin_info_result {
                log::debug!(
                    "MEXC: Checking token {} (contract: {})",
                    base_asset,
                    symbol_info.contract_address
                );

                if let Some(coin_info) = coin_infos.iter().find(|c| c.coin == base_asset) {
                    log::debug!(
                        "MEXC: Found coin_info for {} with {} networks",
                        base_asset,
                        coin_info.network_list.len()
                    );

                    // Check if there's a network that matches our requirements
                    for network in &coin_info.network_list {
                        let network_name = network
                            .network
                            .as_deref()
                            .or(network.net_work.as_deref())
                            .unwrap_or("");
                        log::debug!(
                            "MEXC: {} - checking network '{}', deposit_enable={}, contract={:?}",
                            base_asset,
                            network_name,
                            network.is_deposit_enabled(),
                            network.contract
                        );

                        let is_correct_network = match self.client.address_type {
                            FilterAddressType::Ethereum => {
                                // MEXC returns network names like "Ethereum(ERC20)", "ERC20", "ETH", etc.
                                // Accept any network that contains "Ethereum" or "ERC20" (but not BEP20, TRC20, etc.)
                                let name_lower = network_name.to_lowercase();
                                (name_lower.contains("ethereum") || name_lower.contains("erc20"))
                                    && !name_lower.contains("bep")
                                    && !name_lower.contains("trc")
                                    && !name_lower.contains("arbitrum")
                                    && !name_lower.contains("polygon")
                                    && !name_lower.contains("optimism")
                                    && !name_lower.contains("base")
                                    && !name_lower.contains("linea")
                            }
                            FilterAddressType::Solana => {
                                // Only accept Solana network
                                let name_lower = network_name.to_lowercase();
                                name_lower.contains("solana") || name_lower == "sol"
                            }
                        };

                        if is_correct_network {
                            // Verify the contract address matches
                            if let Some(ref contract) = network.contract {
                                if contract.eq_ignore_ascii_case(&symbol_info.contract_address) {
                                    status.is_deposit_enabled = network.is_deposit_enabled();
                                    status.network_verified = true;

                                    if status.is_trading
                                        && status.is_deposit_enabled
                                        && status.network_verified
                                    {
                                        verified_count += 1;
                                        log::debug!(
                                            "MEXC: ✓ Verified {} - trading:{} deposit:{} network:{}",
                                            base_asset,
                                            status.is_trading,
                                            status.is_deposit_enabled,
                                            status.network_verified
                                        );
                                    }
                                    break;
                                } else {
                                    log::debug!(
                                        "MEXC: Contract mismatch for {} on {}: API={} vs Exchange={}",
                                        base_asset,
                                        network_name,
                                        contract,
                                        symbol_info.contract_address
                                    );
                                }
                            } else {
                                log::debug!(
                                    "MEXC: {} on {} has no contract address in coin info",
                                    base_asset,
                                    network_name
                                );
                            }
                        }
                    }
                } else {
                    log::debug!("MEXC: No coin info found for {}", base_asset);
                }
            } else {
                // No coin_info available (not authenticated)
                log::debug!("MEXC: Coin info API not available (authentication required)");
            }

            if !status.network_verified {
                failed_count += 1;
                log::debug!(
                    "MEXC: Token {} ({}) - network verification failed or deposits disabled",
                    base_asset,
                    symbol_info.contract_address
                );
            }

            // Store in cache
            self.token_status_cache
                .insert(market_symbol.clone(), status.clone());

            // Also store the contract-to-symbol mapping (using normalized lowercase address)
            self.contract_to_market_symbol
                .insert(normalized_contract.clone(), market_symbol.clone());
            self.market_symbol_to_contract
                .insert(market_symbol.clone(), normalized_contract.clone());

            // Store quantity precision (default to 8 if not provided)
            let precision = symbol_info.base_asset_precision.unwrap_or(8);
            self.symbol_precision_cache
                .insert(base_asset.clone(), precision);

            // Add to safe symbols list if verified
            if status.is_trading && status.is_deposit_enabled && status.network_verified {
                safe_symbols.push(market_symbol);
            }
        }

        info!(
            "MEXC: Token status refresh complete. Verified: {}, Failed: {}, Total: {}",
            verified_count,
            failed_count,
            verified_count + failed_count
        );

        Ok(safe_symbols)
    }

    async fn get_deposit_address(
        &self,
        symbol: &str,
        address_type: FilterAddressType,
    ) -> Result<String> {
        self.get_deposit_address_impl(symbol, address_type).await
    }

    async fn sell_token_for_usdt(&self, symbol: &str, amount: f64) -> Result<(String, f64, f64)> {
        self.sell_token_for_usdt_impl(symbol, amount).await
    }

    async fn withdraw_usdt(
        &self,
        address: &str,
        amount: f64,
        address_type: FilterAddressType,
    ) -> Result<String> {
        self.withdraw_usdt_impl(address, amount, address_type).await
    }

    async fn get_portfolio(&self) -> Result<crate::Portfolio> {
        self.get_portfolio_impl().await
    }

    async fn transfer_all_to_trading(&self, _coin: Option<&str>) -> Result<u32> {
        // MEXC has no separate trading/funding accounts, so this is a no-op
        Ok(0)
    }

    async fn transfer_all_to_funding(&self, _coin: Option<&str>) -> Result<u32> {
        // MEXC has no separate trading/funding accounts, so this is a no-op
        Ok(0)
    }

    async fn get_token_symbol_for_contract_address(
        &self,
        contract_address: &str,
    ) -> Option<String> {
        // Get the market symbol (e.g., "LINKUSDT") and extract base asset
        let market_symbol = self
            .contract_to_market_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())?;

        // Remove "USDT" suffix to get base asset symbol
        if market_symbol.ends_with("USDT") {
            Some(market_symbol[..market_symbol.len() - 4].to_string())
        } else {
            Some(market_symbol)
        }
    }

    async fn get_quantity_precision(&self, symbol: &str) -> Result<u32> {
        // Check cache first
        if let Some(precision) = self.symbol_precision_cache.get(symbol) {
            return Ok(*precision);
        }

        // If not in cache, fetch exchange info to populate cache
        log::debug!("MEXC: Precision not in cache for {}, refreshing...", symbol);
        let symbols = self.client.get_token_usdt_pairs().await?;

        for symbol_info in symbols {
            let base_asset = symbol_info.base_asset.clone();
            let precision = symbol_info.base_asset_precision.unwrap_or(8);
            self.symbol_precision_cache.insert(base_asset, precision);
        }

        // Try again after refresh
        self.symbol_precision_cache
            .get(symbol)
            .map(|p| *p)
            .ok_or_else(|| anyhow::anyhow!("MEXC: Symbol {} not found after refresh", symbol))
    }
}

impl MexcService {
    /// Estimate how much USDT you'd get by selling a certain amount of tokens on MEXC
    /// Uses the orderbook to simulate market sell order
    pub async fn estimate_sell_output(
        &self,
        token_contract: &str,
        token_amount: f64,
    ) -> Result<f64> {
        // Get the market symbol for this token
        let market_symbol = self
            .contract_to_market_symbol
            .get(&token_contract.to_lowercase())
            .map(|entry| entry.value().clone())
            .context("Token not found in MEXC markets")?;

        // Fetch orderbook (bids = buy orders, we want to sell into these)
        let orderbook = self.client.get_orderbook(&market_symbol, 100).await?;

        let mut remaining_tokens = token_amount;
        let mut total_usdt = 0.0;

        // Iterate through bids (buy orders) from highest to lowest price
        for bid in orderbook.bids {
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
                "Orderbook depth insufficient for {} {}, {} tokens remaining unsold",
                token_amount, market_symbol, remaining_tokens
            );
        }

        Ok(total_usdt)
    }

    /// Get deposit address for a token on MEXC
    pub async fn get_deposit_address_impl(
        &self,
        symbol: &str,
        address_type: FilterAddressType,
    ) -> Result<String> {
        // Map FilterAddressType to MEXC network names
        let network = match address_type {
            FilterAddressType::Ethereum => "ERC20",
            FilterAddressType::Solana => "SOL",
        };

        let deposit_address = self.client.get_deposit_address(symbol, network).await?;
        Ok(deposit_address)
    }

    /// Sell tokens for USDT using market order
    pub async fn sell_token_for_usdt_impl(
        &self,
        symbol: &str,
        amount: f64,
    ) -> Result<(String, f64, f64)> {
        let symbol_pair = format!("{}USDT", symbol);

        // Place market sell order
        let order_result = self
            .client
            .place_market_order(&symbol_pair, "SELL", amount)
            .await?;

        log::info!(
            "MEXC order placement response: {}",
            serde_json::to_string_pretty(&order_result)?
        );

        // Extract order ID
        let order_id = if let Some(id_str) = order_result.get("orderId").and_then(|v| v.as_str()) {
            id_str.to_string()
        } else if let Some(id_num) = order_result.get("orderId").and_then(|v| v.as_i64()) {
            id_num.to_string()
        } else {
            return Err(anyhow::anyhow!("No orderId in response"));
        };

        // Wait a moment for order to execute
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Query order to get execution details
        let order_status = self.client.get_order(&symbol_pair, &order_id).await?;
        log::info!(
            "MEXC order status response: {}",
            serde_json::to_string_pretty(&order_status)?
        );

        // Extract execution details
        let executed_qty = order_status
            .get("executedQty")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| order_status.get("executedQty").and_then(|v| v.as_f64()))
            .unwrap_or(0.0);

        let cumulative_quote_qty = order_status
            .get("cummulativeQuoteQty")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| {
                order_status
                    .get("cummulativeQuoteQty")
                    .and_then(|v| v.as_f64())
            })
            .unwrap_or(0.0);

        Ok((order_id, executed_qty, cumulative_quote_qty))
    }

    /// Withdraw USDT to external wallet
    pub async fn withdraw_usdt_impl(
        &self,
        address: &str,
        amount: f64,
        address_type: FilterAddressType,
    ) -> Result<String> {
        // Map FilterAddressType to MEXC network names
        let network = match address_type {
            FilterAddressType::Ethereum => "ERC20",
            FilterAddressType::Solana => "SOL",
        };

        let withdrawal_id = self
            .client
            .withdraw("USDT", address, amount, network)
            .await?;
        Ok(withdrawal_id)
    }

    /// Get portfolio balances from MEXC
    pub async fn get_portfolio_impl(&self) -> Result<crate::Portfolio> {
        let account_info = self.client.get_account_info().await?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut balances = Vec::new();
        let mut total_usdt_value = 0.0;

        // Extract balances from account info
        if let Some(balance_array) = account_info.get("balances").and_then(|v| v.as_array()) {
            for balance_item in balance_array {
                let asset = balance_item
                    .get("asset")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let free = balance_item
                    .get("free")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);

                let locked = balance_item
                    .get("locked")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);

                let total = free + locked;

                // Only include non-zero balances
                if total > 0.0 {
                    // Calculate USDT value
                    let usdt_value = if asset == "USDT" {
                        total
                    } else {
                        // Try to get price from cache
                        let market_symbol = format!("{}USDT", asset);
                        if let Some(contract) = self.market_symbol_to_contract.get(&market_symbol) {
                            if let Some(price_info) = self.price_cache.get(contract.value()) {
                                total * price_info.price
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        }
                    };

                    total_usdt_value += usdt_value;

                    balances.push(crate::Balance {
                        asset,
                        free,
                        locked,
                        total,
                    });
                }
            }
        }

        // MEXC doesn't have separate trading and funding accounts
        // Both are the same
        let account_balances = crate::AccountBalances {
            balances: balances.clone(),
            total_usdt_value,
        };

        Ok(crate::Portfolio {
            exchange: "MEXC".to_string(),
            trading: account_balances.clone(),
            funding: account_balances,
            total_usdt_value,
            timestamp,
        })
    }
}
