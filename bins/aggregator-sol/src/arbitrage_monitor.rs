use crate::aggregator::{DexAggregator, SwapRoute};
use crate::arbitrage_config::ArbitrageConfig;
use crate::pool_manager::ArbitragePoolUpdate;
use crate::tx_execution::transaction_builder::PriorityLevel;
use crate::tx_execution::HeliusSender;
use crate::types::{ExecutionPriority, SwapParams};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::sync::broadcast;

use sqlx::{Pool, Postgres};

// Define arbitrage opportunity structures to match DB schema
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArbitrageOpportunity {
    pub pair_name: String,
    pub token_a: String,
    pub token_b: String,
    pub profit_amount: u64,
    pub profit_percent: f64,
    pub input_amount: u64,
    pub detected_at: u64,
    pub execution_status: String,
    pub error_message: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AbnormalArbitrageOpportunity {
    pub pair_name: String,
    pub token_a: String,
    pub token_b: String,
    pub profit_amount: u64,
    pub profit_percent: f64,
    pub input_amount: u64,
    pub detected_at: u64,
    pub routes: Vec<String>,
}

/// Active arbitrage monitor that watches pool updates and executes on mainnet
#[derive(Clone)]
pub struct ArbitrageMonitor {
    aggregator: Arc<DexAggregator>,
    config: ArbitrageConfig,
    db: Pool<Postgres>,
    helius_sender: Arc<HeliusSender>,
    rpc_client: Arc<RpcClient>,
    /// Pre-built index for O(1) triangle path lookup by token pair
    triangle_index: crate::arbitrage_config::TrianglePathIndex,
}

impl ArbitrageMonitor {
    /// Create a new arbitrage monitor configured for mainnet execution
    pub fn new(
        aggregator: Arc<DexAggregator>,
        config: ArbitrageConfig,
        db: Pool<Postgres>,
        rpc_client: Arc<RpcClient>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Initialize Helius sender from environment
        let helius_sender = HeliusSender::from_env()
            .map_err(|e| format!("Failed to initialize Helius sender: {}", e))?;
        let helius_sender = Arc::new(helius_sender);

        // Start connection warmer (pings every 30s as per Helius docs)
        helius_sender.clone().spawn_connection_warmer();

        // Build triangle path index for fast O(1) lookup
        let triangle_index = config.build_triangle_index();
        let (path_count, pair_count) = triangle_index.stats();
        log::info!(
            "🔺 Triangle index built: {} paths indexed by {} unique token pairs",
            path_count,
            pair_count
        );

        log::info!("🌐 Arbitrage Monitor initialized with Helius Sender");
        log::info!("📍 Signer pubkey: {}", helius_sender.payer_pubkey());

        Ok(Self {
            aggregator,
            config,
            db,
            helius_sender,
            rpc_client,
            triangle_index,
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
        // Get base token (SOL) and monitored tokens
        let base_token = match self.config.get_base_token() {
            Ok(t) => t,
            Err(_) => return,
        };

        let monitored_pubkeys = self.config.get_monitored_token_pubkeys();

        // Identify the "other" token - pool must involve base token (SOL)
        let other_token = if pool_update.token_a == base_token {
            pool_update.token_b
        } else if pool_update.token_b == base_token {
            pool_update.token_a
        } else {
            return; // Pool doesn't involve base token (SOL)
        };

        // Skip if other token is not in monitored list
        if !monitored_pubkeys.contains(&other_token) {
            return;
        }

        log::debug!(
            "💹 Pool update for SOL/{} on {:?} - triggering cross-DEX check",
            other_token,
            pool_update.dex
        );

        // Run 2-leg and 3-leg arbitrage checks in parallel
        tokio::join!(
            // 2-leg cross-DEX arbitrage check
            self.check_arbitrage(base_token, other_token),
            // 3-leg triangle arbitrage check (O(1) path lookup)
            self.check_triangle_arbitrage(pool_update.token_a, pool_update.token_b)
        );
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
            user_wallet: self.helius_sender.payer_pubkey(),
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

                // Case 1: Profitable Arbitrage (Profit > 0 AND meets min_profit_bps)
                let profit_bps = (profit_percent * 100.0) as i64;
                if profit > 0 && profit_bps >= self.config.settings.min_profit_bps as i64 {
                    let forward_dexes: Vec<String> = forward_route
                        .paths
                        .iter()
                        .flat_map(|p| p.steps.iter().map(|s| s.dex.to_string()))
                        .collect();

                    let reverse_dexes: Vec<String> = reverse_route
                        .paths
                        .iter()
                        .flat_map(|p| p.steps.iter().map(|s| s.dex.to_string()))
                        .collect();

                    let details = serde_json::json!({
                        "forward_output": forward_route.output_amount,
                        "reverse_output": reverse_route.output_amount,
                        "forward_dexes": forward_dexes,
                        "reverse_dexes": reverse_dexes
                    });

                    let opportunity = ArbitrageOpportunity {
                        pair_name: format!("{}-{}", token_a, token_b),
                        token_a: token_a.to_string(),
                        token_b: token_b.to_string(),
                        profit_amount: profit as u64,
                        profit_percent,
                        input_amount: self.config.settings.base_amount,
                        detected_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        execution_status: "Pending".to_string(),
                        error_message: None,
                        details,
                    };

                    // Save opportunity with Pending status
                    if let Err(e) = self.save_opportunity(&opportunity).await {
                        log::error!("Failed to save arbitrage opportunity: {}", e);
                    }

                    log::info!(
                        "🎯 ARBITRAGE: {} <-> {} | Profit: {:.4}% ({} lamports)",
                        token_a,
                        token_b,
                        profit_percent,
                        profit
                    );

                    // Execute an arbitrage opportunity
                    self.execute_arbitrade_opportunity(
                        opportunity,
                        forward_route.clone(),
                        reverse_route.clone(),
                        PriorityLevel::High, // Use High priority for profitable opportunities
                    )
                    .await;
                }

                // Case 2: Abnormal Arbitrage (Profit > 5% or < -5%)
                if profit_percent.abs() > 5.0 {
                    let forward_dexes: Vec<String> = forward_route
                        .paths
                        .iter()
                        .flat_map(|p| p.steps.iter().map(|s| s.dex.to_string()))
                        .collect();

                    let reverse_dexes: Vec<String> = reverse_route
                        .paths
                        .iter()
                        .flat_map(|p| p.steps.iter().map(|s| s.dex.to_string()))
                        .collect();

                    let mut routes = forward_dexes.clone();
                    routes.extend(reverse_dexes.clone());

                    let abnormal_opp = AbnormalArbitrageOpportunity {
                        pair_name: format!("{}-{}", token_a, token_b),
                        token_a: token_a.to_string(),
                        token_b: token_b.to_string(),
                        profit_amount: profit as u64,
                        profit_percent,
                        input_amount: self.config.settings.base_amount,
                        detected_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        routes,
                    };

                    if let Err(e) = self.save_abnormal_opportunity(&abnormal_opp).await {
                        log::error!("Failed to save abnormal opportunity: {}", e);
                    }

                    log::warn!(
                        "⚠️ ABNORMAL ARBITRAGE: {} <-> {} | Profit: {:.4}% | Fwd: {:?} | Rev: {:?}",
                        token_a,
                        token_b,
                        profit_percent,
                        forward_dexes,
                        reverse_dexes
                    );
                }
            }
            None => {
                // No profitable arbitrage
                log::debug!("No profitable arbitrage for {} -> {}", token_a, token_b);
            }
        }
    }

    /// Check triangle arbitrage opportunities when a pool update is received
    /// Uses pre-built HashMap index for O(1) lookup of relevant paths by token pair
    async fn check_triangle_arbitrage(&self, token_a: Pubkey, token_b: Pubkey) {
        let input_amount = self.config.settings.base_amount;
        let slippage_bps = self.config.settings.slippage_bps;
        let min_profit_bps = self.config.settings.min_profit_bps;

        // O(1) lookup - only get paths that have a leg involving this exact token pair
        let relevant_paths = self.triangle_index.get_paths_for_pair(&token_a, &token_b);

        if relevant_paths.is_empty() {
            return;
        }

        log::debug!(
            "🔺 Checking {} triangle paths for pair ({}, {})",
            relevant_paths.len(),
            token_a,
            token_b
        );

        for path in relevant_paths {
            match self
                .aggregator
                .calculate_triangle_profit(path, input_amount, slippage_bps)
                .await
            {
                Some((profit, leg1_route, leg2_route, leg3_route)) => {
                    let profit_bps = (profit as f64 / input_amount as f64 * 10000.0) as i64;

                    if profit > 0 && profit_bps as u64 >= min_profit_bps {
                        log::info!(
                            "🔺 TRIANGLE OPPORTUNITY: {} | Profit: {} lamports ({} bps)",
                            path.name,
                            profit,
                            profit_bps
                        );

                        // Log route details
                        log::info!(
                            "   Leg1: {:?}",
                            leg1_route
                                .paths
                                .iter()
                                .map(|p| p.steps.iter().map(|s| s.dex).collect::<Vec<_>>())
                                .collect::<Vec<_>>()
                        );
                        log::info!(
                            "   Leg2: {:?}",
                            leg2_route
                                .paths
                                .iter()
                                .map(|p| p.steps.iter().map(|s| s.dex).collect::<Vec<_>>())
                                .collect::<Vec<_>>()
                        );
                        log::info!(
                            "   Leg3: {:?}",
                            leg3_route
                                .paths
                                .iter()
                                .map(|p| p.steps.iter().map(|s| s.dex).collect::<Vec<_>>())
                                .collect::<Vec<_>>()
                        );

                        // TODO: Execute triangle arbitrage when ready
                        // For now just log the opportunity
                    } else if profit_bps.abs() > 500 {
                        // Log abnormal losses (>5%)
                        log::warn!(
                            "⚠️ ABNORMAL TRIANGLE: {} | Loss: {} lamports ({} bps)",
                            path.name,
                            profit,
                            profit_bps
                        );
                    }
                }
                None => {
                    // Route not found for this triangle - skip silently
                }
            }
        }
    }

    /// Save an arbitrage opportunity to Postgres
    pub async fn save_opportunity(
        &self,
        opp: &ArbitrageOpportunity,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Serialize details to JSONB
        let details_json = serde_json::to_value(&opp.details)?;

        sqlx::query(
            r#"
            INSERT INTO arbitrage_opportunities (
                pair_name, token_a, token_b, profit_amount, profit_percent,
                input_amount, detected_at, execution_status, error_message, details,
                is_abnormal
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(&opp.pair_name)
        .bind(&opp.token_a)
        .bind(&opp.token_b)
        .bind(opp.profit_amount as i64)
        .bind(opp.profit_percent)
        .bind(opp.input_amount as i64)
        .bind(opp.detected_at as i64)
        .bind(&opp.execution_status)
        .bind(opp.error_message.as_deref())
        .bind(details_json)
        .bind(false) // Not abnormal
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Save an abnormal arbitrage opportunity to Postgres
    pub async fn save_abnormal_opportunity(
        &self,
        opp: &AbnormalArbitrageOpportunity,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Similar to above, but strict different fields or just flag it?
        // The schema supports `is_abnormal` flag.
        // Let's map AbnormalArbitrageOpportunity to the table fields.
        let details = serde_json::json!({
            "routes": opp.routes,
            "reason": "Abnormal Profit"
        });

        sqlx::query(
            r#"
            INSERT INTO arbitrage_opportunities (
                pair_name, token_a, token_b, profit_amount, profit_percent,
                input_amount, detected_at, execution_status, error_message, details,
                is_abnormal
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(&opp.pair_name)
        .bind(&opp.token_a)
        .bind(&opp.token_b)
        .bind(opp.profit_amount as i64)
        .bind(opp.profit_percent)
        .bind(opp.input_amount as i64)
        .bind(opp.detected_at as i64)
        .bind("DETECTED") // Status
        .bind(Option::<String>::None)
        .bind(details)
        .bind(true) // Is abnormal
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Get recent opportunities (last N) from Postgres
    pub async fn get_recent_opportunities(&self, limit: usize) -> Vec<ArbitrageOpportunity> {
        let rows = sqlx::query(
            "SELECT * FROM arbitrage_opportunities WHERE is_abnormal = FALSE ORDER BY detected_at DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default(); // Return empty on error for now? Or log?

        rows.into_iter()
            .map(|row| {
                use sqlx::Row;
                let details_val: serde_json::Value =
                    row.try_get("details").unwrap_or(serde_json::Value::Null); // Handle nulls if any

                ArbitrageOpportunity {
                    pair_name: row.get("pair_name"),
                    token_a: row.get("token_a"),
                    token_b: row.get("token_b"),
                    profit_amount: row.get::<i64, _>("profit_amount") as u64,
                    profit_percent: row.get("profit_percent"),
                    input_amount: row.get::<i64, _>("input_amount") as u64,
                    detected_at: row.get::<i64, _>("detected_at") as u64,
                    execution_status: row.get::<String, _>("execution_status"),
                    error_message: row.get("error_message"),
                    details: details_val,
                }
            })
            .collect()
    }

    /// Get recent abnormal opportunities (last N) from Postgres
    pub async fn get_recent_abnormal_opportunities(
        &self,
        limit: usize,
    ) -> Vec<AbnormalArbitrageOpportunity> {
        let rows = sqlx::query(
             "SELECT * FROM arbitrage_opportunities WHERE is_abnormal = true ORDER BY detected_at DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(|row| {
                use sqlx::Row;
                let details: serde_json::Value =
                    row.try_get("details").unwrap_or(serde_json::Value::Null);
                let routes = details
                    .get("routes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                AbnormalArbitrageOpportunity {
                    pair_name: row.get("pair_name"),
                    token_a: row.get("token_a"),
                    token_b: row.get("token_b"),
                    profit_amount: row.get::<i64, _>("profit_amount") as u64,
                    profit_percent: row.get("profit_percent"),
                    input_amount: row.get::<i64, _>("input_amount") as u64,
                    detected_at: row.get::<i64, _>("detected_at") as u64,
                    routes,
                }
            })
            .collect()
    }

    /// Cleanup old opportunities older than specified seconds
    pub async fn cleanup_old_opportunities(
        &self,
        max_age_seconds: u64,
    ) -> Result<u64, sqlx::Error> {
        let cutoff_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - max_age_seconds;

        let result = sqlx::query("DELETE FROM arbitrage_opportunities WHERE detected_at < $1")
            .bind(cutoff_time as i64)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected())
    }

    // Execute an arbitrage opportunity using Helius Sender
    async fn execute_arbitrade_opportunity(
        &self,
        mut opportunity: ArbitrageOpportunity,
        forward_route: SwapRoute,
        reverse_route: SwapRoute,
        priority: PriorityLevel,
    ) {
        log::info!(
            "🔄 Executing arbitrage opportunity: {} ({:.4}% profit)",
            opportunity.pair_name,
            opportunity.profit_percent
        );

        let payer = self.helius_sender.payer_pubkey();

        // Build arbitrage transaction instructions
        match self
            .aggregator
            .build_arbitrage_instructions(&forward_route, &reverse_route, payer)
            .await
        {
            Ok(instructions) => {
                // Send smart transaction via Helius
                match self
                    .helius_sender
                    .send_smart_transaction(instructions, priority)
                    .await
                {
                    Ok(result) => {
                        log::info!(
                            "✅ Trade successful - Signature: {} - CU: {:?}",
                            result.signature,
                            result.compute_units_consumed
                        );
                        opportunity.execution_status = "Success".to_string();
                        opportunity.error_message = None;
                    }
                    Err(e) => {
                        let error_msg = format!("Transaction failed: {}", e);
                        log::error!("❌ {}", error_msg);
                        opportunity.execution_status = "Fail".to_string();
                        opportunity.error_message = Some(error_msg);
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to build instructions: {}", e);
                log::error!("❌ {}", error_msg);
                opportunity.execution_status = "Fail".to_string();
                opportunity.error_message = Some(error_msg);
            }
        }

        // Save updated opportunity with execution status
        if let Err(e) = self.save_opportunity(&opportunity).await {
            log::error!("Failed to save opportunity status: {}", e);
        }
    }
}
