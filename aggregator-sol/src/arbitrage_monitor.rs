use crate::aggregator::{DexAggregator, SwapRoute};
use crate::arbitrage_config::ArbitrageConfig;
use crate::pool_manager::ArbitragePoolUpdate;
use crate::types::{ExecutionPriority, SwapParams};
use rocksdb::{Options, DB};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Execution status for arbitrage opportunities
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    NotYet,
    Success,
    Fail,
}

/// Detected arbitrage opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub pair_name: String,
    pub token_a: String,
    pub token_b: String,
    pub profit_amount: u64,
    pub profit_percent: f64,
    pub input_amount: u64,
    pub forward_output: u64,
    pub reverse_output: u64,
    pub detected_at: u64,
    pub execution_status: ExecutionStatus,
    pub error_message: Option<String>,
}

/// Active arbitrage monitor that watches pool updates and executes on mainnet
#[derive(Clone)]
pub struct ArbitrageMonitor {
    aggregator: Arc<DexAggregator>,
    config: ArbitrageConfig,
    db: Arc<DB>,
    rpc_client: Arc<RpcClient>,
}

impl ArbitrageMonitor {
    /// Create a new arbitrage monitor configured for mainnet execution
    pub fn new(
        aggregator: Arc<DexAggregator>,
        config: ArbitrageConfig,
        db_path: impl AsRef<Path>,
        rpc_url: &str,
        keypair: Arc<Keypair>,
    ) -> Result<Self, rocksdb::Error> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, db_path)?;
        let rpc_client = Arc::new(RpcClient::new(rpc_url.to_string()));

        log::info!("🌐 Arbitrage Monitor initialized for mainnet: {}", rpc_url);
        log::info!("📍 Signer pubkey: {}", keypair.pubkey());

        Ok(Self {
            aggregator,
            config,
            db: Arc::new(db),
            rpc_client,
        })
    }

    pub fn get_rpc_client(&self) -> Arc<RpcClient> {
        self.rpc_client.clone()
    }
    /// Subscribe to pool update events from the pool manager
    /// This spawns a task that listens for events and triggers arbitrage checks
    pub fn subscribe_to_pool_updates(
        self: Arc<Self>,
        mut pool_update_rx: broadcast::Receiver<ArbitragePoolUpdate>,
    ) {
        tokio::spawn(async move {
            log::info!("Arbitrage monitor subscribed to broadcast pool update events");

            loop {
                match pool_update_rx.recv().await {
                    Ok(pool_update) => {
                        // Spawn event handler to avoid blocking the loop
                        let self_clone = self.clone();
                        tokio::spawn(async move {
                            self_clone.on_broadcast_pool_update(&pool_update).await;
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Subscriber lagged, skip
                        log::warn!("Arbitrage monitor lagged on broadcast channel");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Channel closed
                        log::info!("Arbitrage monitor broadcast channel closed");
                        break;
                    }
                }
            }
        });
    }

    /// Check arbitrage when a broadcast pool update is received
    /// This is triggered by real-time pool updates from the broadcast channel
    async fn on_broadcast_pool_update(&self, pool_update: &ArbitragePoolUpdate) {
        // Quick price check first - round trip should yield profit
        let price_round_trip = pool_update.forward_price * pool_update.reverse_price;
        let price_diff_percent = ((price_round_trip - 1.0) * 100.0).abs();
        let threshold_percent = (self.config.settings.min_profit_bps as f64) / 100.0;

        if price_diff_percent < threshold_percent {
            return; // Not profitable enough
        }

        log::debug!(
            "💹 Price opportunity detected: {:.4}% difference in pool {}",
            price_diff_percent,
            pool_update.pool_address
        );

        // Get base token and monitored tokens
        let base_token = match self.config.get_base_token() {
            Ok(t) => t,
            Err(_) => return,
        };

        let monitored_pubkeys = self.config.get_monitored_token_pubkeys();

        // One token must be base, other must be monitored
        let other_token = if pool_update.token_a == base_token
            && monitored_pubkeys.contains(&pool_update.token_b)
        {
            pool_update.token_b
        } else if pool_update.token_b == base_token
            && monitored_pubkeys.contains(&pool_update.token_a)
        {
            pool_update.token_a
        } else {
            return; // Not a base-to-monitored pair
        };

        log::debug!("✅ Arbitrage pair: base <-> {}", other_token);
        self.check_arbitrage(base_token, other_token).await;
    }

    /// Check arbitrage opportunity for a specific token pair
    /// This performs the actual expensive arbitrage calculation after price checks pass
    async fn check_arbitrage(&self, token_a: Pubkey, token_b: Pubkey) {
        // Get token info from pool manager
        let token_a_info = match self.aggregator.get_pool_manager().get_token(&token_a).await {
            Some(t) => t,
            None => {
                log::debug!("Token A {} not found in pool manager", token_a);
                return;
            }
        };

        let token_b_info = match self.aggregator.get_pool_manager().get_token(&token_b).await {
            Some(t) => t,
            None => {
                log::debug!("Token B {} not found in pool manager", token_b);
                return;
            }
        };

        // STEP: Calculate actual arbitrage profit with full routing
        let swap_params = SwapParams {
            input_token: token_a_info.clone(),
            output_token: token_b_info.clone(),
            input_amount: self.config.settings.base_amount,
            slippage_bps: self.config.settings.slippage_bps,
            user_wallet: Pubkey::default(),
            priority: ExecutionPriority::Medium,
        };

        match self
            .aggregator
            .calculate_arbitrage_profit(&swap_params, &token_b, self.config.settings.slippage_bps)
            .await
        {
            Some((profit, forward_route, reverse_route)) => {
                let profit_percent =
                    (profit as f64 / self.config.settings.base_amount as f64) * 100.0;
                // Check if profit meets minimum threshold
                let profit_bps = (profit_percent * 100.0) as u64;
                if profit_bps >= self.config.settings.min_profit_bps {
                    let opportunity;
                    opportunity = ArbitrageOpportunity {
                        pair_name: format!("{}-{}", token_a, token_b),
                        token_a: token_a.to_string(),
                        token_b: token_b.to_string(),
                        profit_amount: profit,
                        profit_percent,
                        input_amount: self.config.settings.base_amount,
                        forward_output: forward_route.output_amount,
                        reverse_output: reverse_route.output_amount,
                        detected_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        execution_status: ExecutionStatus::NotYet,
                        error_message: None,
                    };

                    // Save opportunity with Pending status
                    if let Err(e) = self.save_opportunity(&opportunity) {
                        log::error!("Failed to save arbitrage opportunity: {}", e);
                    }

                    log::info!(
                        "🎯 ARBITRAGE: {} <-> {} | Profit: {:.4}% ({} lamports)",
                        token_a,
                        token_b,
                        profit_percent,
                        profit
                    );
                    let payer =
                        Pubkey::from_str("7dGrdJRYtsNR8UYxZ3TnifXGjGc9eRYLq9sELwYpuuUu").unwrap();
                    // Execute an arbitrage opportunity
                    self.execute_arbitrade_opportunity(
                        opportunity,
                        forward_route,
                        reverse_route,
                        ExecutionPriority::Medium,
                        Some(payer),
                        &self.rpc_client,
                    )
                    .await;
                }
            }
            None => {
                // No profitable arbitrage
                log::debug!("No profitable arbitrage for {} -> {}", token_a, token_b);
            }
        }
    }

    /// Save an opportunity to RocksDB
    fn save_opportunity(&self, opportunity: &ArbitrageOpportunity) -> Result<(), String> {
        let key = format!("opp:{}:{}", opportunity.detected_at, opportunity.pair_name);
        let value = serde_json::to_vec(opportunity)
            .map_err(|e| format!("Failed to serialize opportunity: {}", e))?;
        self.db
            .put(key.as_bytes(), value)
            .map_err(|e| format!("Failed to save to DB: {}", e))?;
        Ok(())
    }

    /// Get recent opportunities (last N)
    pub fn get_recent_opportunities(&self, limit: usize) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();
        let mut iter = self.db.raw_iterator();

        // Seek to the last key with "opp:" prefix
        iter.seek_to_last();

        while iter.valid() {
            if let Some(key) = iter.key() {
                if let Ok(key_str) = std::str::from_utf8(key) {
                    if key_str.starts_with("opp:") {
                        if let Some(value) = iter.value() {
                            if let Ok(opp) = serde_json::from_slice::<ArbitrageOpportunity>(value) {
                                opportunities.push(opp);
                                if opportunities.len() >= limit {
                                    break;
                                }
                            }
                        }
                    } else {
                        break; // No more opportunities
                    }
                }
            }
            iter.prev();
        }

        opportunities
    }

    /// Clear old opportunities (older than N seconds)
    pub fn cleanup_old_opportunities(&self, max_age_seconds: u64) -> Result<usize, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let cutoff_time = now - max_age_seconds;
        let mut deleted_count = 0;

        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        let mut keys_to_delete = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.starts_with("opp:") {
                    if let Ok(opp) = serde_json::from_slice::<ArbitrageOpportunity>(&value) {
                        if opp.detected_at < cutoff_time {
                            keys_to_delete.push(key.to_vec());
                        }
                    }
                }
            }
        }

        for key in &keys_to_delete {
            self.db
                .delete(key)
                .map_err(|e| format!("Failed to delete key: {}", e))?;
            deleted_count += 1;
        }

        Ok(deleted_count)
    }

    // Execute an arbitrage opportunity (simulation only)
    async fn execute_arbitrade_opportunity(
        &self,
        mut opportunity: ArbitrageOpportunity,
        forward_route: SwapRoute,
        reverse_route: SwapRoute,
        priority: ExecutionPriority,
        payer: Option<Pubkey>,
        rpc_client: &RpcClient,
    ) {
        let payer = payer.unwrap_or_else(|| {
            Pubkey::from_str("7dGrdJRYtsNR8UYxZ3TnifXGjGc9eRYLq9sELwYpuuUu").unwrap()
        });

        log::info!(
            "🔄 Executing arbitrage opportunity: {} ({:.4}% profit)",
            opportunity.pair_name,
            opportunity.profit_percent
        );

        // Build arbitrage transaction
        match self
            .aggregator
            .build_arbitrage_transaction(
                &forward_route,
                &reverse_route,
                priority,
                payer,
                rpc_client,
            )
            .await
        {
            Ok(transaction) => {
                // Simulate the transaction
                match rpc_client.simulate_transaction(&transaction).await {
                    Ok(simulation) => {
                        if let Some(err) = simulation.value.err {
                            // Simulation failed
                            let error_msg = format!("Simulation failed: {:?}", err);
                            log::error!("❌ {}", error_msg);
                            opportunity.execution_status = ExecutionStatus::Fail;
                            opportunity.error_message = Some(error_msg);
                        } else {
                            // Simulation succeeded
                            log::info!(
                                "✅ Simulation successful - compute units: {:?}",
                                simulation.value.units_consumed
                            );
                            opportunity.execution_status = ExecutionStatus::Success;
                            opportunity.error_message = None;
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("RPC error during simulation: {}", e);
                        log::error!("❌ {}", error_msg);
                        opportunity.execution_status = ExecutionStatus::Fail;
                        opportunity.error_message = Some(error_msg);
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to build transaction: {}", e);
                log::error!("❌ {}", error_msg);
                opportunity.execution_status = ExecutionStatus::Fail;
                opportunity.error_message = Some(error_msg);
            }
        }

        // Save updated opportunity with execution status
        if let Err(e) = self.save_opportunity(&opportunity) {
            log::error!("Failed to save opportunity status: {}", e);
        }
    }
}
