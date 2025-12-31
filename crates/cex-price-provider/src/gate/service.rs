use crate::gate::client::GateClient;
use crate::{FilterAddressType, PriceProvider, TokenPrice};
use anyhow::{Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::{future::try_join_all, SinkExt, StreamExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

#[derive(Debug, Deserialize)]
struct TickerMessage {
    pub time: i64,
    pub channel: String,
    pub event: String,
    pub result: TickerResult,
}

#[derive(Debug, Deserialize)]
struct TickerResult {
    pub currency_pair: String,
    pub last: String,
    pub change_percentage: String,
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    time: i64,
    channel: String,
    event: String,
    payload: Vec<String>,
}

pub struct GateService {
    client: GateClient,
    price_cache: Arc<DashMap<String, TokenPrice>>,
    symbol_to_contract: Arc<DashMap<String, String>>,
    contract_to_symbol: Arc<DashMap<String, String>>,
    token_status_cache: Arc<DashMap<String, crate::TokenStatus>>,
    symbol_precision_cache: Arc<DashMap<String, u32>>,
}

impl GateService {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: GateClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
            symbol_precision_cache: Arc::new(DashMap::new()),
        }
    }

    async fn start_websocket_connection(
        connection_id: usize,
        symbols: &[String],
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
    ) -> Result<()> {
        let ws_url = "wss://api.gateio.ws/ws/v4/";

        info!(
            "Connection {}: Connecting to Gate.io WebSocket: {}",
            connection_id, ws_url
        );

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to Gate.io WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        log::info!(
            "Connection {}: WebSocket connected successfully",
            connection_id
        );

        // Subscribe to ticker streams for each symbol
        let subscription = SubscriptionRequest {
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64,
            channel: "spot.tickers".to_string(),
            event: "subscribe".to_string(),
            payload: symbols.to_vec(),
        };

        let sub_msg = serde_json::to_string(&subscription)?;
        log::debug!(
            "Connection {}: Subscribing to {} symbols",
            connection_id,
            symbols.len()
        );

        write
            .send(WsMessage::Text(sub_msg.into()))
            .await
            .context("Failed to send subscription message")?;

        info!(
            "Connection {}: Subscribed to {} symbols",
            connection_id,
            symbols.len()
        );

        // Gate.io requires ping every 15 seconds
        let write = Arc::new(tokio::sync::Mutex::new(write));
        let ping_write = write.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(15));
            loop {
                interval.tick().await;

                let ping_msg = serde_json::json!({
                    "time": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    "channel": "spot.ping"
                })
                .to_string();

                let mut writer = ping_write.lock().await;
                if let Err(e) = writer.send(WsMessage::Text(ping_msg.into())).await {
                    error!("Connection {}: Failed to send ping: {}", connection_id, e);
                    break;
                }
                log::debug!("Connection {}: Sent ping", connection_id);
            }
        });

        // Process incoming messages
        log::info!(
            "Connection {}: Starting to process messages...",
            connection_id
        );

        while let Some(msg) = read.next().await {
            match msg {
                Ok(WsMessage::Text(text)) => {
                    log::debug!("Connection {}: Received text message", connection_id);
                    Self::handle_text_message(
                        connection_id,
                        &text,
                        price_cache,
                        symbol_to_contract,
                    );
                }
                Ok(WsMessage::Close(frame)) => {
                    warn!(
                        "Connection {}: WebSocket closed by server: {:?}",
                        connection_id, frame
                    );
                    break;
                }
                Err(e) => {
                    error!("Connection {}: WebSocket error: {}", connection_id, e);
                    break;
                }
                _ => {}
            }
        }

        log::warn!("Connection {}: Message loop ended", connection_id);

        Ok(())
    }

    fn handle_text_message(
        connection_id: usize,
        text: &str,
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
    ) {
        log::debug!("Connection {}: Received message: {}", connection_id, text);

        // Handle pong messages
        if text.contains("\"channel\":\"spot.pong\"") {
            log::debug!("Connection {}: Received pong", connection_id);
            return;
        }

        // Handle subscription confirmation
        if text.contains("\"event\":\"subscribe\"")
            && text.contains("\"result\":{\"status\":\"success\"")
        {
            log::info!("Connection {}: Subscription confirmed", connection_id);
            return;
        }

        // Try to parse as ticker message
        if let Ok(ticker_msg) = serde_json::from_str::<TickerMessage>(text) {
            if ticker_msg.event == "update" && ticker_msg.channel == "spot.tickers" {
                if let Ok(price) = ticker_msg.result.last.parse::<f64>() {
                    let symbol = &ticker_msg.result.currency_pair;

                    // Determine the cache key: contract address if available, otherwise symbol
                    let cache_key = symbol_to_contract
                        .get(symbol)
                        .map(|entry| entry.value().clone())
                        .unwrap_or_else(|| symbol.to_lowercase());

                    log::debug!(
                        "Connection {}: Updating price for {} (key: {}) = {}",
                        connection_id,
                        symbol,
                        cache_key,
                        price
                    );

                    price_cache.insert(
                        cache_key.clone(),
                        TokenPrice {
                            symbol: cache_key,
                            price,
                        },
                    );
                }
            }
        } else {
            log::debug!(
                "Connection {}: Failed to parse as ticker message",
                connection_id
            );
        }
    }
}

#[async_trait]
impl PriceProvider for GateService {
    async fn get_price(&self, key: &str) -> Option<TokenPrice> {
        self.price_cache
            .get(&key.to_lowercase())
            .map(|entry| entry.value().clone())
    }

    async fn get_all_prices(&self) -> Vec<TokenPrice> {
        self.price_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    async fn get_prices(&self, keys: &Vec<String>) -> Vec<Option<TokenPrice>> {
        let mut result = Vec::new();
        for key in keys {
            result.push(self.get_price(&key.to_lowercase()).await);
        }
        result
    }

    async fn start(&self) -> Result<()> {
        // Initial token status refresh (with network and deposit verification)
        info!("Gate.io: Performing initial token status verification...");
        let safe_market_symbols = match self.refresh_token_status().await {
            Ok(symbols) => {
                info!(
                    "Gate.io: Successfully verified {} safe tokens",
                    symbols.len()
                );
                symbols
            }
            Err(e) => {
                warn!("Gate.io: Initial token status refresh failed: {}", e);
                return Ok(());
            }
        };

        if safe_market_symbols.is_empty() {
            warn!("Gate.io: No safe tokens to subscribe to after filtering");
            return Ok(());
        }

        info!(
            "Gate.io: Subscribing to {} verified safe tokens",
            safe_market_symbols.len()
        );

        // Split symbols into chunks for multiple connections
        // Gate.io can handle many symbols per connection
        const MAX_SYMBOLS_PER_CONNECTION: usize = 100;
        let connection_chunks: Vec<Vec<String>> = safe_market_symbols
            .chunks(MAX_SYMBOLS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        log::info!("Starting {} WebSocket connections", connection_chunks.len());

        // Start multiple WebSocket connections concurrently
        let mut connection_handles = Vec::new();

        for (connection_id, chunk) in connection_chunks.into_iter().enumerate() {
            let price_cache = self.price_cache.clone();
            let symbol_to_contract = self.symbol_to_contract.clone();

            let handle = tokio::spawn(async move {
                loop {
                    info!(
                        "Starting WebSocket connection {} for {} symbols",
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
            symbol_to_contract: self.symbol_to_contract.clone(),
            contract_to_symbol: self.contract_to_symbol.clone(),
            token_status_cache: self.token_status_cache.clone(),
            symbol_precision_cache: self.symbol_precision_cache.clone(),
        });
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(12 * 3600)); // 12 hours
            interval.tick().await; // Skip first immediate tick

            loop {
                interval.tick().await;
                info!("Gate.io: Starting scheduled token status refresh (every 12 hours)...");
                if let Err(e) = refresh_service.refresh_token_status().await {
                    warn!("Gate.io: Scheduled token status refresh failed: {}", e);
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
                    "Gate.io Service Stats - Tokens with prices: {}, Contracts mapped: {}",
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
        "Gate.io"
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
        // Try to get from cache first using market symbol (e.g., "LINK_USDT")
        if let Some(status) = self.token_status_cache.get(symbol) {
            return Some(status.clone());
        }

        // If not in cache and we have a contract address, try to find by contract
        if let Some(contract_addr) = contract_address {
            let normalized_addr = contract_addr.to_lowercase();
            if let Some(market_symbol) = self.contract_to_symbol.get(&normalized_addr) {
                return self
                    .token_status_cache
                    .get(market_symbol.value())
                    .map(|s| s.clone());
            }
        }

        None
    }

    async fn refresh_token_status(&self) -> Result<Vec<String>> {
        info!("Gate.io: Refreshing token status cache...");

        // Get all trading pairs
        let pairs = self.client.get_token_usdt_pairs().await?;
        info!("Gate.io: Found {} trading pairs", pairs.len());

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Get unique currencies
        let unique_currencies: std::collections::HashSet<String> =
            pairs.iter().map(|p| p.base.clone()).collect();

        info!(
            "Gate.io: Fetching chain info for {} unique currencies...",
            unique_currencies.len()
        );

        // Fetch all currency chains concurrently in batches (same as start() method)
        const BATCH_SIZE: usize = 5;
        const BATCH_DELAY_MS: u64 = 1000;

        let currencies: Vec<String> = unique_currencies.into_iter().collect();
        let mut all_currency_chains = Vec::new();

        for (batch_num, chunk) in currencies.chunks(BATCH_SIZE).enumerate() {
            let futures: Vec<_> = chunk
                .iter()
                .map(|currency| {
                    let client = &self.client;
                    let currency = currency.clone();
                    async move {
                        let result = client.get_currency_chains(&currency).await;
                        (currency.clone(), result)
                    }
                })
                .collect();

            let results = futures_util::future::join_all(futures).await;
            all_currency_chains.extend(results);

            // Log progress
            let currencies_processed = ((batch_num + 1) * BATCH_SIZE).min(currencies.len());
            if currencies_processed % 50 < BATCH_SIZE || currencies_processed == currencies.len() {
                info!(
                    "Gate.io: Progress: {}/{} currencies processed",
                    currencies_processed,
                    currencies.len()
                );
            }

            if batch_num + 1 < currencies.len().div_ceil(BATCH_SIZE) {
                tokio::time::sleep(tokio::time::Duration::from_millis(BATCH_DELAY_MS)).await;
            }
        }

        info!(
            "Gate.io: Fetched chain info for {} currencies",
            all_currency_chains.len()
        );

        // Build a map of currency -> contract addresses
        let mut currency_contracts: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();

        for (currency, result) in all_currency_chains {
            match result {
                Ok(chains) => {
                    let mut contracts = Vec::new();
                    for chain_info in chains {
                        if chain_info.is_disabled == 0 && chain_info.is_deposit_enabled() {
                            if let Some(contract) = chain_info.contract_address {
                                if !contract.is_empty() {
                                    contracts.push((chain_info.chain.clone(), contract));
                                }
                            }
                        }
                    }
                    if !contracts.is_empty() {
                        currency_contracts.insert(currency.clone(), contracts);
                    }
                }
                Err(e) => {
                    log::debug!("Gate.io: Failed to get chain info for {}: {}", currency, e);
                }
            }
        }

        // Now process all pairs with the cached chain info
        let mut verified_count = 0;
        let mut failed_count = 0;

        for pair in pairs {
            let market_symbol = pair.id.clone(); // e.g., "LINK_USDT"
            let base_asset = pair.base.clone();

            // Extract and cache precision from amount_precision
            if let Some(precision) = pair.amount_precision {
                if precision >= 0 {
                    self.symbol_precision_cache
                        .insert(base_asset.clone(), precision as u32);
                }
            }

            // Default status: trading enabled (from exchange info), but need to verify deposits
            let mut status = crate::TokenStatus {
                symbol: market_symbol.clone(),
                base_asset: base_asset.clone(),
                contract_address: None,
                is_trading: pair.trade_status == "tradable",
                is_deposit_enabled: false,
                network_verified: false,
                last_updated: current_time,
            };

            // Check if we have chain info for this currency
            if let Some(contracts) = currency_contracts.get(&base_asset) {
                for (chain_name, contract_address) in contracts {
                    let is_correct_network = match self.client.address_type {
                        FilterAddressType::Ethereum => {
                            let chain_lower = chain_name.to_lowercase();
                            (chain_lower.contains("eth") || chain_lower.contains("erc20"))
                                && !chain_lower.contains("bsc")
                                && !chain_lower.contains("arb")
                                && !chain_lower.contains("polygon")
                                && !chain_lower.contains("optimism")
                        }
                        FilterAddressType::Solana => {
                            let chain_lower = chain_name.to_lowercase();
                            chain_lower.contains("sol") && !chain_lower.contains("bsc")
                        }
                    };

                    if is_correct_network && self.client.is_valid_address(contract_address) {
                        let normalized_contract = contract_address.to_lowercase();
                        status.contract_address = Some(normalized_contract.clone());
                        status.is_deposit_enabled = true;
                        status.network_verified = true;

                        if status.is_trading && status.is_deposit_enabled && status.network_verified
                        {
                            verified_count += 1;
                        }

                        // Store contract mapping
                        self.contract_to_symbol
                            .insert(normalized_contract.clone(), market_symbol.clone());
                        self.symbol_to_contract
                            .insert(market_symbol.clone(), normalized_contract);
                        break;
                    }
                }
            }

            if !status.network_verified {
                failed_count += 1;
            }

            // Store in cache
            self.token_status_cache.insert(market_symbol, status);
        }

        info!(
            "Gate.io: Token status refresh complete. Verified: {}, Failed: {}, Total: {}",
            verified_count,
            failed_count,
            verified_count + failed_count
        );

        // Return list of verified safe market symbols
        let safe_symbols: Vec<String> = self
            .token_status_cache
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

        info!(
            "Gate.io: Returning {} safe symbols for WebSocket subscription",
            safe_symbols.len()
        );
        Ok(safe_symbols)
    }

    async fn get_deposit_address(
        &self,
        _symbol: &str,
        _address_type: crate::FilterAddressType,
    ) -> Result<String> {
        Err(anyhow::anyhow!(
            "Gate.io: get_deposit_address not yet implemented"
        ))
    }

    async fn sell_token_for_usdt(&self, _symbol: &str, _amount: f64) -> Result<(String, f64, f64)> {
        Err(anyhow::anyhow!(
            "Gate.io: sell_token_for_usdt not yet implemented"
        ))
    }

    async fn withdraw_usdt(
        &self,
        _address: &str,
        _amount: f64,
        _address_type: crate::FilterAddressType,
    ) -> Result<String> {
        Err(anyhow::anyhow!(
            "Gate.io: withdraw_usdt not yet implemented"
        ))
    }

    async fn get_portfolio(&self) -> Result<crate::Portfolio> {
        Err(anyhow::anyhow!(
            "Gate.io: get_portfolio not yet implemented"
        ))
    }

    async fn transfer_all_to_trading(&self, _coin: Option<&str>) -> Result<u32> {
        Err(anyhow::anyhow!(
            "Gate.io: transfer_all_to_trading not yet implemented"
        ))
    }

    async fn transfer_all_to_funding(&self, _coin: Option<&str>) -> Result<u32> {
        Err(anyhow::anyhow!(
            "Gate.io: transfer_all_to_funding not yet implemented"
        ))
    }

    async fn get_token_symbol_for_contract_address(
        &self,
        contract_address: &str,
    ) -> Option<String> {
        // Get the market symbol (e.g., "LINK_USDT") and extract base asset
        let market_symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())?;

        // Gate.io uses underscore separator, split on "_USDT"
        if let Some(base) = market_symbol.strip_suffix("_USDT") {
            Some(base.to_string())
        } else {
            Some(market_symbol)
        }
    }

    async fn get_quantity_precision(&self, symbol: &str) -> Result<u32> {
        // Check cache first
        if let Some(precision) = self.symbol_precision_cache.get(symbol) {
            return Ok(*precision);
        }

        // If not in cache, refresh token status and try again
        log::info!(
            "Gate.io: Precision not in cache for {}, refreshing...",
            symbol
        );
        self.refresh_token_status().await?;

        // Check cache again after refresh
        if let Some(precision) = self.symbol_precision_cache.get(symbol) {
            return Ok(*precision);
        }

        // If still not found, return default
        log::warn!(
            "Gate.io: Could not find precision for {}, using default (8)",
            symbol
        );
        Ok(8)
    }
}

impl GateService {
    /// Estimate how much USDT you'd get by selling a certain amount of tokens on Gate.io
    pub async fn estimate_sell_output(
        &self,
        contract_address: &str,
        token_amount: f64,
    ) -> Result<f64> {
        let symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())
            .context("Contract address not found in Gate.io markets")?;

        let orderbook = self.client.get_orderbook(&symbol, 100).await?;

        let mut remaining_tokens = token_amount;
        let mut total_usdt = 0.0;

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
                "Orderbook depth insufficient for {} {} (contract: {}), {} tokens remaining unsold",
                token_amount, symbol, contract_address, remaining_tokens
            );
        }

        Ok(total_usdt)
    }
}
