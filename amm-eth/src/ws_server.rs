use anyhow::{Context, Result};
use dashmap::DashMap;
use eth_dex_quote::{EthChain, TokenPrice};
use ethers::types::Address;
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// Message types that can be sent to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "price_update")]
    PriceUpdate { data: TokenPriceUpdate },
    #[serde(rename = "heartbeat")]
    Heartbeat { timestamp: u64 },
    #[serde(rename = "welcome")]
    Welcome { client_id: String },
}

/// Token price update message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceUpdate {
    pub token_address: String,
    pub price_in_eth: f64,
    pub price_in_usd: Option<f64>,
    pub last_updated: u64,
    pub pool_address: String,
    pub dex_version: String,
    pub decimals: u8,
    pub pool_token0: Address,
    pub pool_token1: Address,
    pub eth_chain: EthChain,
    pub fee_tier: Option<u32>,
}

impl From<TokenPrice> for TokenPriceUpdate {
    fn from(price: TokenPrice) -> Self {
        Self {
            token_address: format!("{:?}", price.token_address),
            price_in_eth: price.price_in_eth,
            price_in_usd: price.price_in_usd,
            last_updated: price.last_updated,
            pool_address: format!("{:?}", price.pool_address),
            dex_version: format!("{:?}", price.dex_version),
            decimals: price.decimals,
            pool_token0: price.pool_token0,
            pool_token1: price.pool_token1,
            eth_chain: price.eth_chain,
            fee_tier: price.fee_tier,
        }
    }
}

/// Client connection information
struct Client {
    tx: mpsc::UnboundedSender<Message>,
    last_activity: Arc<tokio::sync::RwLock<Instant>>,
}

/// WebSocket server for broadcasting token price updates
pub struct WsServer {
    addr: SocketAddr,
    clients: Arc<DashMap<Uuid, Client>>,
    broadcast_tx: mpsc::UnboundedSender<WsMessage>,
    broadcast_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<WsMessage>>>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
}

impl WsServer {
    /// Create a new WebSocket server
    pub fn new(addr: SocketAddr) -> Self {
        let (broadcast_tx, broadcast_rx) = mpsc::unbounded_channel();

        Self {
            addr,
            clients: Arc::new(DashMap::new()),
            broadcast_tx,
            broadcast_rx: Arc::new(tokio::sync::Mutex::new(broadcast_rx)),
            heartbeat_interval: Duration::from_secs(30),
            client_timeout: Duration::from_secs(90),
        }
    }

    /// Get a sender for broadcasting messages
    pub fn get_broadcaster(&self) -> mpsc::UnboundedSender<WsMessage> {
        self.broadcast_tx.clone()
    }

    /// Start the WebSocket server
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let listener = TcpListener::bind(self.addr)
            .await
            .context("Failed to bind WebSocket server")?;

        info!("WebSocket server listening on: {}", self.addr);

        // Start broadcast handler
        let broadcast_self = self.clone();
        tokio::spawn(async move {
            broadcast_self.handle_broadcasts().await;
        });

        // Start heartbeat task
        let heartbeat_self = self.clone();
        tokio::spawn(async move {
            heartbeat_self.send_heartbeats().await;
        });

        // Start client cleanup task
        let cleanup_self = self.clone();
        tokio::spawn(async move {
            cleanup_self.cleanup_inactive_clients().await;
        });

        // Accept connections
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New WebSocket connection from: {}", addr);
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = server.handle_connection(stream, addr).await {
                            error!("Error handling connection from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a new WebSocket connection
    async fn handle_connection(&self, stream: TcpStream, addr: SocketAddr) -> Result<()> {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .context("Failed to accept WebSocket connection")?;

        let (ws_sender, mut ws_receiver) = ws_stream.split();

        let client_id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let client = Client {
            tx,
            last_activity: Arc::new(tokio::sync::RwLock::new(Instant::now())),
        };

        self.clients.insert(client_id, client);
        info!("Client {} connected from {}", client_id, addr);

        // Send welcome message
        let welcome_msg = WsMessage::Welcome {
            client_id: client_id.to_string(),
        };
        if let Ok(msg_str) = serde_json::to_string(&welcome_msg) {
            let _ = self
                .clients
                .get(&client_id)
                .map(|c| c.tx.send(Message::Text(msg_str.into())));
        }

        // Spawn task to send messages to this client
        let mut ws_sender = ws_sender;
        let sender_clients = self.clients.clone();
        let sender_client_id = client_id;
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_sender.send(msg).await.is_err() {
                    break;
                }
            }
            // Remove client when sender closes
            sender_clients.remove(&sender_client_id);
            info!("Client {} sender closed", sender_client_id);
        });

        // Handle incoming messages from client
        let last_activity = self.clients.get(&client_id).unwrap().last_activity.clone();
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Update last activity
                    let mut activity = last_activity.write().await;
                    *activity = Instant::now();

                    info!("Received message from {}: {}", client_id, text);
                    // Handle client messages if needed (e.g., subscription filters)
                }
                Ok(Message::Ping(data)) => {
                    // Update last activity
                    let mut activity = last_activity.write().await;
                    *activity = Instant::now();

                    // Respond with pong
                    if let Some(client) = self.clients.get(&client_id) {
                        let _ = client.tx.send(Message::Pong(data));
                    }
                }
                Ok(Message::Pong(_)) => {
                    // Update last activity
                    let mut activity = last_activity.write().await;
                    *activity = Instant::now();
                }
                Ok(Message::Close(_)) => {
                    info!("Client {} closed connection", client_id);
                    break;
                }
                Err(e) => {
                    error!("WebSocket error for client {}: {}", client_id, e);
                    break;
                }
                _ => {}
            }
        }

        // Remove client on disconnect
        self.clients.remove(&client_id);
        info!("Client {} disconnected from {}", client_id, addr);

        Ok(())
    }

    /// Handle broadcasting messages to all clients
    async fn handle_broadcasts(&self) {
        let mut rx = self.broadcast_rx.lock().await;

        while let Some(ws_msg) = rx.recv().await {
            let msg_str = match serde_json::to_string(&ws_msg) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                    continue;
                }
            };

            let message = Message::Text(msg_str.into());
            let mut disconnected_clients = Vec::new();

            for client_ref in self.clients.iter() {
                let client_id = *client_ref.key();
                if client_ref.value().tx.send(message.clone()).is_err() {
                    disconnected_clients.push(client_id);
                }
            }

            // Remove disconnected clients
            for client_id in disconnected_clients {
                self.clients.remove(&client_id);
                info!("Removed disconnected client: {}", client_id);
            }
        }
    }

    /// Send periodic heartbeats to all clients
    async fn send_heartbeats(&self) {
        let mut interval = tokio::time::interval(self.heartbeat_interval);

        loop {
            interval.tick().await;

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let heartbeat = WsMessage::Heartbeat { timestamp };

            if let Err(e) = self.broadcast_tx.send(heartbeat) {
                error!("Failed to send heartbeat: {}", e);
            }
        }
    }

    /// Clean up inactive clients
    async fn cleanup_inactive_clients(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            let now = Instant::now();
            let mut inactive_clients = Vec::new();

            for client_ref in self.clients.iter() {
                let client_id = *client_ref.key();
                let last_activity = client_ref.value().last_activity.read().await;
                if now.duration_since(*last_activity) > self.client_timeout {
                    inactive_clients.push(client_id);
                }
            }

            for client_id in inactive_clients {
                self.clients.remove(&client_id);
                warn!("Removed inactive client: {}", client_id);
            }

            if !self.clients.is_empty() {
                info!("Active WebSocket clients: {}", self.clients.len());
            }
        }
    }

    /// Get the number of connected clients
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

/// Broadcast a token price update
pub fn broadcast_price_update(
    broadcaster: &mpsc::UnboundedSender<WsMessage>,
    token_address: Address,
    price: TokenPrice,
) {
    // Only broadcast if USD price is available
    if price.price_in_usd.is_none() {
        return;
    }

    let price_update = TokenPriceUpdate::from(price);
    let msg = WsMessage::PriceUpdate { data: price_update };

    if let Err(e) = broadcaster.send(msg) {
        error!(
            "Failed to broadcast price update for {:?}: {}",
            token_address, e
        );
    }
}
