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
}

impl BitgetService {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: BitgetClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
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
        info!("Starting Bitget Service");

        // Get all USDT trading pairs
        let pairs = self
            .client
            .get_token_usdt_pairs()
            .await
            .context("Failed to fetch Bitget trading pairs")?;

        info!("Found {} USDT trading pairs", pairs.len());

        // Fetch contract addresses for each currency in parallel
        log::info!(
            "Fetching currency details for {} pairs in parallel...",
            pairs.len()
        );

        let unique_currencies: std::collections::HashSet<String> =
            pairs.iter().map(|p| p.base_coin.clone()).collect();

        log::info!(
            "Found {} unique currencies to query",
            unique_currencies.len()
        );

        // Fetch all currency details concurrently in batches
        const BATCH_SIZE: usize = 10;
        const BATCH_DELAY_MS: u64 = 500;

        let currencies: Vec<String> = unique_currencies.into_iter().collect();
        let mut all_currency_details = Vec::new();

        for (batch_num, chunk) in currencies.chunks(BATCH_SIZE).enumerate() {
            let futures: Vec<_> = chunk
                .iter()
                .map(|currency| {
                    let client = &self.client;
                    let currency = currency.clone();
                    async move { (currency.clone(), client.get_coin_info(&currency).await) }
                })
                .collect();

            let results = futures_util::future::join_all(futures).await;
            all_currency_details.extend(results);

            // Log progress every 100 currencies
            let currencies_processed = ((batch_num + 1) * BATCH_SIZE).min(currencies.len());
            if currencies_processed % 100 < BATCH_SIZE || currencies_processed == currencies.len() {
                log::info!(
                    "Progress: {}/{} currencies processed",
                    currencies_processed,
                    currencies.len()
                );
            }

            if batch_num + 1 < (currencies.len() + BATCH_SIZE - 1) / BATCH_SIZE {
                tokio::time::sleep(tokio::time::Duration::from_millis(BATCH_DELAY_MS)).await;
            }
        }

        log::info!("Fetched {} currency details", all_currency_details.len());

        // Build a map of currency -> contract addresses (only if deposits are enabled)
        let mut currency_contracts: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();

        for (currency, result) in all_currency_details {
            if let Ok(currency_infos) = result {
                if currency_infos.is_empty() {
                    log::debug!("No currency info returned for {}", currency);
                    continue;
                }

                for currency_info in currency_infos {
                    log::debug!(
                        "Processing {} - found {} chains",
                        currency_info.coin,
                        currency_info.chains.len()
                    );

                    let mut contracts = Vec::new();
                    for chain in &currency_info.chains {
                        log::debug!(
                            "  Chain: {}, Contract: {:?}, Deposit: {}, Withdraw: {}",
                            chain.chain,
                            chain.contract_address,
                            chain.rechargeable,
                            chain.withdrawable
                        );

                        // Only include chains with deposits enabled and valid contract address
                        if let Some(contract_addr) = &chain.contract_address {
                            if !contract_addr.is_empty() && chain.is_deposit_enabled() {
                                contracts.push((chain.chain.clone(), contract_addr.clone()));
                            } else if !contract_addr.is_empty() {
                                log::debug!(
                                    "Skipping {} on chain {} - deposits disabled",
                                    currency,
                                    chain.chain
                                );
                            }
                        } else {
                            log::debug!(
                                "Skipping {} on chain {} - native token (no contract address)",
                                currency,
                                chain.chain
                            );
                        }
                    }
                    if !contracts.is_empty() {
                        let contract_count = contracts.len();
                        currency_contracts.insert(currency.clone(), contracts);
                        log::debug!("Added {} contracts for {}", contract_count, currency);
                        break;
                    }
                }
            } else if let Err(e) = result {
                log::debug!("Failed to fetch coin info for {}: {}", currency, e);
            }
        }

        log::info!(
            "Found contract addresses for {} currencies",
            currency_contracts.len()
        );

        // Map trading pairs to contract addresses
        let mut contract_count = 0;
        let mut filtered_count = 0;

        for pair in &pairs {
            if let Some(contracts) = currency_contracts.get(&pair.base_coin) {
                for (chain_name, contract_address) in contracts {
                    let is_target_chain = match self.client.address_type {
                        FilterAddressType::Ethereum => {
                            chain_name.to_lowercase().contains("eth")
                                || chain_name.to_lowercase().contains("erc20")
                        }
                        FilterAddressType::Solana => {
                            chain_name.to_lowercase().contains("sol")
                                || chain_name.to_lowercase().contains("solana")
                        }
                    };

                    if is_target_chain && self.client.is_valid_address(contract_address) {
                        self.symbol_to_contract
                            .insert(pair.symbol.clone(), contract_address.clone());
                        self.contract_to_symbol
                            .insert(contract_address.to_lowercase(), pair.symbol.clone());
                        contract_count += 1;
                        break;
                    } else if is_target_chain {
                        filtered_count += 1;
                        log::debug!(
                            "Filtered out {} with invalid contract address: {}",
                            pair.symbol,
                            contract_address
                        );
                        break;
                    }
                }
            }
        }

        log::info!(
            "Mapped {} trading pairs to contract addresses ({} filtered out as invalid)",
            contract_count,
            filtered_count
        );

        // Filter pairs: only subscribe to those with valid contract addresses
        let symbols: Vec<String> = if contract_count > 0 {
            pairs
                .iter()
                .filter(|pair| self.symbol_to_contract.contains_key(&pair.symbol))
                .map(|pair| pair.symbol.clone())
                .collect()
        } else {
            log::warn!("No contract addresses found, subscribing to all USDT pairs");
            pairs.iter().map(|pair| pair.symbol.clone()).collect()
        };

        if symbols.is_empty() {
            log::warn!("No symbols to subscribe to after filtering");
            return Ok(());
        }

        log::info!(
            "Subscribing to {} symbols (filtered from {} total)",
            symbols.len(),
            pairs.len()
        );

        // Split symbols into chunks for multiple connections
        const MAX_SYMBOLS_PER_CONNECTION: usize = 50;
        let connection_chunks: Vec<Vec<String>> = symbols
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
