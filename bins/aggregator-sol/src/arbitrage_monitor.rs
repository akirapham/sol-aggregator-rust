use crate::aggregator::{DexAggregator, SwapRoute};
use crate::arbitrage_config::ArbitrageConfig;
use crate::pool_manager::ArbitragePoolUpdate;
use crate::tx_execution::transaction_builder::PriorityLevel;
use crate::tx_execution::HeliusSender;
use crate::types::{ExecutionPriority, SwapParams};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};

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
pub struct ArbitrageMonitor {
    aggregator: Arc<DexAggregator>,
    config: ArbitrageConfig,
    db: Pool<Postgres>,
    helius_sender: Arc<HeliusSender>,
    rpc_client: Arc<RpcClient>,
    /// Pre-built index for O(1) triangle path lookup by token pair
    triangle_index: crate::arbitrage_config::TrianglePathIndex,
    /// Debounce cache: (token_a, token_b) -> last check time
    last_check_times: Arc<RwLock<HashMap<(Pubkey, Pubkey), Instant>>>,
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
            last_check_times: Arc::new(RwLock::new(HashMap::new())),
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

    /// Run startup validation of all configured paths
    /// Checks if paths are tradeable given current pool state
    pub fn start_startup_validation(self: Arc<Self>) {
        tokio::spawn(async move {
            // Delay to allow population of pending_pools_to_fetch_tick_arrays
            // This prevents race condition where pools are loaded but ticks not yet queued
            log::info!("⏳ Delaying startup validation by 20s to ensure pending ticks queue population...");
            tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;

            log::info!("⏳ Waiting for pool manager synchronization...");
            let pool_manager = self.aggregator.get_pool_manager();
            
            // Wait up to 60s for sync
            let start = Instant::now();
            loop {
                if pool_manager.is_fully_synced().await {
                    log::info!("✅ Pool manager fully synced! Starting validation...");
                    break;
                }
                
                if start.elapsed().as_secs() > 60 {
                    log::warn!("⚠️ Pool manager sync timed out (60s). Proceeding with validation anyway...");
                    break;
                }
                
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            
            self.validate_all_paths().await;
        });
    }

    async fn validate_all_paths(&self) {
        log::info!("🧪 STARTING FULL PATH VALIDATION 🧪");
        let paths = self.config.get_triangle_paths();
        let mut valid_count = 0;
        let mut fail_count = 0;

        for path in paths {
            // Find a trigger pool for Leg 1 (from -> to)
            let leg1_pair = (path.leg1_from, path.leg1_to);
            let pools = self.aggregator.get_pool_manager().get_pools_for_pair(&leg1_pair.0, &leg1_pair.1).await;
            
            if pools.is_empty() {
                log::info!("❌ Path {} INVALID: No pools found for Leg 1 ({}/{})", 
                    path.name,
                    self.config.get_token_symbol(&path.leg1_from.to_string()),
                    self.config.get_token_symbol(&path.leg1_to.to_string())
                );
                fail_count += 1;
                continue;
            }
            
            let trigger_pool = pools[0].address();
            
            // Force check
            let result = self.aggregator.calculate_triangle_profit(
                &path, 
                self.config.settings.base_amount, 
                self.config.settings.slippage_bps, 
                trigger_pool
            ).await;
            
            match result {
                Some((profit, _, _, _)) => {
                    let profit_sol = profit as f64 / 1_000_000_000.0;
                    log::info!("✅ Path {} VALID (Route found, Profit: {:.6} SOL)", path.name, profit_sol);
                    valid_count += 1;
                },
                None => {
                    log::info!("❌ Path {} FAILED (No route found)", path.name);
                    fail_count += 1;
                }
            }
        }
        
        log::info!("🧪 VALIDATION COMPLETE: {} VALID, {} FAILED 🧪", valid_count, fail_count);
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
            "💹 Pool update for {}/{} on {:?} - triggering cross-DEX check",
            self.config.get_token_symbol(&base_token.to_string()),
            self.config.get_token_symbol(&other_token.to_string()),
            pool_update.dex
        );

        // Debounce: skip if we checked this pair within the last 1 second
        let pair_key = if base_token < other_token {
            (base_token, other_token)
        } else {
            (other_token, base_token)
        };

        let debounce_duration = std::time::Duration::from_millis(500);
        {
            let cache = self.last_check_times.read().await;
            if let Some(last_check) = cache.get(&pair_key) {
                if last_check.elapsed() < debounce_duration {
                    log::debug!(
                        "⏭️ Skipping check for {}/{} - debounced",
                        self.config.get_token_symbol(&base_token.to_string()),
                        self.config.get_token_symbol(&other_token.to_string())
                    );
                    return;
                }
            }
        }

        // Update last check time
        {
            let mut cache = self.last_check_times.write().await;
            cache.insert(pair_key, Instant::now());
        }

        log::info!(
            "🔍 Triggering checks for Pool Update: {} ({} / {})",
            pool_update.pool_address,
            self.config
                .get_token_symbol(&pool_update.token_a.to_string()),
            self.config
                .get_token_symbol(&pool_update.token_b.to_string())
        );

        // Check if pool requires ticks and if they are synced
        let needs_ticks = matches!(
            pool_update.dex,
            crate::pool_data_types::DexType::Orca | crate::pool_data_types::DexType::RaydiumClmm | crate::pool_data_types::DexType::MeteoraDlmm
        );
        
        if needs_ticks {
            if !self.aggregator.get_pool_manager().is_pool_tick_synced(&pool_update.pool_address).await {
                log::debug!(
                    "⏳ Skipping arbitrage check for pool {} (Ticks not synced)",
                    pool_update.pool_address
                );
                return;
            }
        }

        // Run 2-leg and 3-leg arbitrage checks in parallel
        tokio::join!(
            // 2-leg cross-DEX arbitrage check
            self.check_arbitrage(base_token, other_token, pool_update.pool_address),
            // 3-leg triangle arbitrage check (O(1) path lookup)
            self.check_triangle_arbitrage(
                pool_update.token_a,
                pool_update.token_b,
                pool_update.pool_address
            )
        );
    }

    /// Check arbitrage opportunity for a specific token pair
    /// This performs the actual expensive arbitrage calculation after price checks pass
    async fn check_arbitrage(&self, token_a: Pubkey, token_b: Pubkey, updated_pool: Pubkey) {
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
            .calculate_arbitrage_profit(
                &swap_params,
                &token_b,
                self.config.settings.slippage_bps,
                updated_pool,
            )
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
                        "🎯 ROUND TRIP ARBITRAGE: {} <-> {} | Profit: {:.4}% ({} lamports)",
                        self.config.get_token_symbol(&token_a.to_string()),
                        self.config.get_token_symbol(&token_b.to_string()),
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
                } else {
                    log::info!(
                        "📉 Round Trip Check: {} <-> {} | Profit: {:.4}% ({} lamports)",
                        self.config.get_token_symbol(&token_a.to_string()),
                        self.config.get_token_symbol(&token_b.to_string()),
                        profit_percent,
                        profit
                    );
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
                        self.config.get_token_symbol(&token_a.to_string()),
                        self.config.get_token_symbol(&token_b.to_string()),
                        profit_percent,
                        forward_dexes,
                        reverse_dexes
                    );
                }
            }
            None => {
                // No valid route found (liquidity issues or no path)
                log::info!(
                    "📉 Round Trip Check: No valid route found for {} <-> {}",
                    self.config.get_token_symbol(&token_a.to_string()),
                    self.config.get_token_symbol(&token_b.to_string())
                );
            }
        }
    }

    /// Check triangle arbitrage opportunities when a pool update is received
    /// Uses pre-built HashMap index for O(1) lookup of relevant paths by token pair
    async fn check_triangle_arbitrage(
        &self,
        token_a: Pubkey,
        token_b: Pubkey,
        updated_pool: Pubkey,
    ) {
        let input_amount = self.config.settings.base_amount;
        let slippage_bps = self.config.settings.slippage_bps;
        let min_profit_bps = self.config.settings.min_profit_bps;

        // O(1) lookup - only get paths that have a leg involving this exact token pair
        let relevant_paths = self.triangle_index.get_paths_for_pair(&token_a, &token_b);

        log::info!(
            "🔍 Paths lookup: Found {} paths for Pair {}/{}",
            relevant_paths.len(),
            self.config.get_token_symbol(&token_a.to_string()),
            self.config.get_token_symbol(&token_b.to_string())
        );

        if relevant_paths.is_empty() {
            return;
        }

        log::info!(
            "🔺 Checking {} triangle paths for pair ({}, {})",
            relevant_paths.len(),
            self.config.get_token_symbol(&token_a.to_string()),
            self.config.get_token_symbol(&token_b.to_string())
        );

        let start = std::time::Instant::now();

        // Create futures for all path calculations - run in parallel
        let futures: Vec<_> = relevant_paths
            .into_iter()
            .map(|path| {
                let aggregator = self.aggregator.clone();
                let path_name = path.name.clone();
                async move {
                    let result = aggregator
                        .calculate_triangle_profit(path, input_amount, slippage_bps, updated_pool)
                        .await;
                    (path_name, result)
                }
            })
            .collect();

        // Execute all path calculations in parallel
        let results = futures::future::join_all(futures).await;
        let elapsed = start.elapsed();
        log::info!("🔺 Triangle check completed in {:?}", elapsed);

        // Process results
        let success_count = results.iter().filter(|(_, r)| r.is_some()).count();
        let fail_count = results.len() - success_count;
        log::info!(
            "🔺 Triangle check: {} paths | {} with routes, {} failed | {:?}",
            results.len(),
            success_count,
            fail_count,
            elapsed
        );

        // Create a result collection to sort by profit
        let mut all_outcomes = Vec::new();

        for (path_name, result) in results {
            if let Some((profit, leg1_route, leg2_route, leg3_route)) = result {
                all_outcomes.push((profit, path_name, leg1_route, leg2_route, leg3_route));
            }
        }

        // Sort by profit descending (highest profit first)
        all_outcomes.sort_by(|a, b| b.0.cmp(&a.0));

        // Separate winners
        let winners: Vec<_> = all_outcomes
            .iter()
            .filter(|(p, _, _, _, _)| *p > 0)
            .collect();

        if !winners.is_empty() {
            // Case A: Profitable opportunities found. Only show these.
            for (profit, path_name, leg1_route, leg2_route, leg3_route) in winners {
                let profit_bps = (*profit as f64 / input_amount as f64 * 10000.0) as i64;

                if profit_bps as u64 >= min_profit_bps {
                    log::info!(
                        "🔺 TRIANGLE OPPORTUNITY: {} | Profit: {} lamports ({} bps)",
                        path_name,
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

                    // Simulate execution / Save
                    // (Assuming you want to keep the saving logic if profitable)
                } else {
                    log::info!(
                        "🔺 Skipped Positive Triangle: {} | Profit: {} ({} bps) < Min BPS ({})",
                        path_name,
                        profit,
                        profit_bps,
                        min_profit_bps
                    );
                }
            }
        } else {
            if all_outcomes.is_empty() {
                 log::info!(
                    "📉 No valid triangle routes found for pair ({}, {}) - all paths failed route checks",
                    self.config.get_token_symbol(&token_a.to_string()),
                    self.config.get_token_symbol(&token_b.to_string())
                );
            } else {
                // Case B: No winners. Log top 2 least negative.
                let top_2_losers = all_outcomes.iter().take(2);
                for (profit, path_name, _, _, _) in top_2_losers {
                    let profit_sol = *profit as f64 / 1_000_000_000.0;
                    let profit_pct = *profit as f64 / input_amount as f64 * 100.0;
                    log::info!(
                        "📉 Best Negative Triangle: {} | Profit: {:.6} SOL ({:.2}%)",
                        path_name,
                        profit_sol,
                        profit_pct
                    );
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
