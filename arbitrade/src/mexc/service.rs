use crate::mexc::client::MexcClient;
use crate::types::{PriceProvider, TokenPrice};
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
}

impl MexcService {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            client: MexcClient::new(),
            price_cache: Arc::new(DashMap::new()),
            market_symbol_to_contract: Arc::new(DashMap::new()),
        })
    }

    pub async fn start(&self) -> Result<()> {
        // Get Solana USDT pairs
        let pairs = self.client.get_solana_usdt_pairs().await?;
        log::info!("Found {} Solana/USDT pairs", pairs.len());

        if pairs.is_empty() {
            warn!("No Solana/USDT pairs found on MEXC");
            return Ok(());
        }

        pairs.iter().for_each(|pair| {
            self.market_symbol_to_contract
                .insert(pair.symbol.clone(), pair.contract_address.clone());
        });

        // Split pairs into chunks for multiple WebSocket connections
        let market_ids = pairs
            .iter()
            .map(|pair| pair.symbol.clone())
            .collect::<Vec<_>>();

        const MAX_STREAMS_PER_CONNECTION: usize = 15; // Using 15 instead of 30 for safety margin
        let connection_chunks: Vec<Vec<String>> = market_ids
            .chunks(MAX_STREAMS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        info!(
            "Creating {} WebSocket connections for {} markets",
            connection_chunks.len(),
            market_ids.len()
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

        // Wait for all connections (they should run indefinitely)
        let results: Result<Vec<_>, _> = try_join_all(connection_handles).await;
        results.context("One or more WebSocket connections failed")?;

        Ok(())
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

        let (ws_stream, response) = connect_async(ws_url)
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
            } else {
                for stream in &stream_names {
                    info!("  - {}", stream);
                }
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
                    log::info!("Connection {}: Sending ping to keep connection alive", connection_id);
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
                    match message.body {
                        Some(push_data) => match push_data {
                            Body::PublicAggreDeals(item) => {
                                if let Some(deal) = item.deals.first() {
                                    let price = TokenPrice {
                                        symbol: market_symbol
                                            .strip_suffix("USDT")
                                            .unwrap_or(&market_symbol)
                                            .to_string(),
                                        price: f64::from_str(&deal.price).unwrap_or(0.0),
                                        timestamp: deal.time,
                                    };
                                    price_cache
                                        .insert(contract_address.value().clone(), price.clone());
                                }
                            }
                        },
                        None => {}
                    }
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
}
