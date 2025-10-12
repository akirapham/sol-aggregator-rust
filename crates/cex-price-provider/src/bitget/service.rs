use crate::bitget::client::BitgetClient;
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
    pub action: String,
    pub arg: TickerArg,
    pub data: Vec<TickerData>,
}

#[derive(Debug, Deserialize)]
struct TickerArg {
    #[serde(rename = "instType")]
    pub inst_type: String,
    pub channel: String,
    #[serde(rename = "instId")]
    pub inst_id: String,
}

#[derive(Debug, Deserialize)]
struct TickerData {
    #[serde(rename = "instId")]
    pub inst_id: String,
    #[serde(rename = "lastPr")]
    pub last_pr: String,
    pub ts: String,
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    op: String,
    args: Vec<SubscriptionArg>,
}

#[derive(Debug, Serialize)]
struct SubscriptionArg {
    #[serde(rename = "instType")]
    inst_type: String,
    channel: String,
    #[serde(rename = "instId")]
    inst_id: String,
}

pub struct BitgetService {
    client: BitgetClient,
    price_cache: Arc<DashMap<String, TokenPrice>>,
    symbol_to_contract: Arc<DashMap<String, String>>,
    contract_to_symbol: Arc<DashMap<String, String>>,
    token_status_cache: Arc<DashMap<String, crate::TokenStatus>>,
}

impl BitgetService {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: BitgetClient::new(address_type),
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
        let ws_url = "wss://ws.bitget.com/v2/ws/public";

        info!(
            "Connection {}: Connecting to Bitget WebSocket: {}",
            connection_id, ws_url
        );

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to Bitget WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        log::info!(
            "Connection {}: WebSocket connected successfully",
            connection_id
        );

        // Subscribe to ticker streams for each symbol
        let args: Vec<SubscriptionArg> = symbols
            .iter()
            .map(|symbol| SubscriptionArg {
                inst_type: "SPOT".to_string(),
                channel: "ticker".to_string(),
                inst_id: symbol.clone(),
            })
            .collect();

        let subscription = SubscriptionRequest {
            op: "subscribe".to_string(),
            args,
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

        // Bitget requires ping every 30 seconds
        let write = Arc::new(tokio::sync::Mutex::new(write));
        let ping_write = write.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let ping_msg = "ping";

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
        if text == "pong" {
            log::debug!("Connection {}: Received pong", connection_id);
            return;
        }

        // Try to parse as ticker message
        if let Ok(ticker_msg) = serde_json::from_str::<TickerMessage>(text) {
            if ticker_msg.action == "snapshot" || ticker_msg.action == "update" {
                for data in ticker_msg.data {
                    if let Ok(price) = data.last_pr.parse::<f64>() {
                        let symbol = &data.inst_id;

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
impl PriceProvider for BitgetService {
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
        info!("Bitget: Performing initial token status verification...");
        let safe_market_symbols = match self.refresh_token_status().await {
            Ok(symbols) => {
                info!("Bitget: Successfully verified {} safe tokens", symbols.len());
                symbols
            }
            Err(e) => {
                warn!("Bitget: Initial token status refresh failed: {}", e);
                return Ok(());
            }
        };

        if safe_market_symbols.is_empty() {
            warn!("Bitget: No safe tokens to subscribe to after filtering");
            return Ok(());
        }

        info!("Bitget: Subscribing to {} verified safe tokens", safe_market_symbols.len());

        // Split symbols into chunks for multiple connections
        const MAX_SYMBOLS_PER_CONNECTION: usize = 50;
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
        });
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(12 * 3600)); // 12 hours
            interval.tick().await; // Skip first immediate tick

            loop {
                interval.tick().await;
                info!("Bitget: Starting scheduled token status refresh (every 12 hours)...");
                if let Err(e) = refresh_service.refresh_token_status().await {
                    warn!("Bitget: Scheduled token status refresh failed: {}", e);
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
                    "Bitget Service Stats - Tokens with prices: {}, Contracts mapped: {}",
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
        "Bitget"
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
        // Try to get from cache first using market symbol (e.g., "LINKUSDT")
        if let Some(status) = self.token_status_cache.get(symbol) {
            return Some(status.clone());
        }

        // If not in cache and we have a contract address, try to find by contract
        if let Some(contract_addr) = contract_address {
            let normalized_addr = contract_addr.to_lowercase();
            if let Some(market_symbol) = self.contract_to_symbol.get(&normalized_addr) {
                return self.token_status_cache.get(market_symbol.value()).map(|s| s.clone());
            }
        }

        None
    }

    async fn refresh_token_status(&self) -> Result<Vec<String>> {
        info!("Bitget: Refreshing token status cache...");

        // Get all trading pairs
        let pairs = self.client.get_token_usdt_pairs().await?;

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut verified_count = 0;
        let mut failed_count = 0;

        for pair in pairs {
            let market_symbol = pair.symbol.clone(); // e.g., "LINKUSDT"
            let base_asset = pair.base_coin.clone();

            // Default status: trading enabled (from exchange info), but need to verify deposits
            let mut status = crate::TokenStatus {
                symbol: market_symbol.clone(),
                base_asset: base_asset.clone(),
                contract_address: None,
                is_trading: pair.status == "online",
                is_deposit_enabled: false,
                network_verified: false,
                last_updated: current_time,
            };

            // Get coin information to check chain details
            match self.client.get_coin_info(&base_asset).await {
                Ok(coin_infos) => {
                    log::debug!("Bitget: Checking token {} with {} chains", base_asset, coin_infos.len());

                    // Check if there's a network that matches our requirements
                    for coin_info in &coin_infos {
                        for chain_info in &coin_info.chains {
                            let chain_name = chain_info.chain.as_str();

                            let is_correct_network = match self.client.address_type {
                                FilterAddressType::Ethereum => {
                                    // Bitget uses chain names like "ETH", "ERC20", or "Ethereum"
                                    let chain_lower = chain_name.to_lowercase();
                                    (chain_lower.contains("eth") || chain_lower.contains("erc20")) &&
                                    !chain_lower.contains("bsc") &&
                                    !chain_lower.contains("arb") &&
                                    !chain_lower.contains("polygon") &&
                                    !chain_lower.contains("optimism")
                                }
                                FilterAddressType::Solana => {
                                    let chain_lower = chain_name.to_lowercase();
                                    chain_lower.contains("sol") && !chain_lower.contains("bsc")
                                }
                            };

                            if is_correct_network && chain_info.is_deposit_enabled() {
                                if let Some(ref contract) = chain_info.contract_address {
                                    if self.client.is_valid_address(contract) {
                                        let normalized_contract = contract.to_lowercase();
                                        status.contract_address = Some(normalized_contract.clone());
                                        status.is_deposit_enabled = true;
                                        status.network_verified = true;

                                        if status.is_trading && status.is_deposit_enabled && status.network_verified {
                                            verified_count += 1;
                                            log::debug!(
                                                "Bitget: ✓ Verified {} - trading:{} deposit:{} network:{}",
                                                base_asset,
                                                status.is_trading,
                                                status.is_deposit_enabled,
                                                status.network_verified
                                            );
                                        }

                                        // Store contract mapping
                                        self.contract_to_symbol.insert(normalized_contract.clone(), market_symbol.clone());
                                        self.symbol_to_contract.insert(market_symbol.clone(), normalized_contract);
                                        break;
                                    }
                                }
                            }
                        }
                        if status.network_verified {
                            break;
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Bitget: Failed to get coin info for {}: {}", base_asset, e);
                }
            }

            if !status.network_verified {
                failed_count += 1;
                log::debug!(
                    "Bitget: Token {} - network verification failed or deposits disabled",
                    base_asset
                );
            }

            // Store in cache
            self.token_status_cache.insert(market_symbol, status);
        }

        info!(
            "Bitget: Token status refresh complete. Verified: {}, Failed: {}, Total: {}",
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

        info!("Bitget: Returning {} safe symbols for WebSocket subscription", safe_symbols.len());
        Ok(safe_symbols)
    }
}

impl BitgetService {
    /// Estimate how much USDT you'd get by selling a certain amount of tokens on Bitget
    pub async fn estimate_sell_output(
        &self,
        contract_address: &str,
        token_amount: f64,
    ) -> Result<f64> {
        let symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())
            .context("Contract address not found in Bitget markets")?;

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
