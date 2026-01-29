// Module declarations for traits
pub mod traits;

use crate::pool_manager::traits::{DatabaseTrait, GrpcServiceTrait, PriceServiceTrait};

use crate::fetchers::common::{fetch_account_data, fetch_multiple_accounts, fetch_token};
use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
use crate::grpc::BatchProcessor;
use crate::pool_data_types::{
    dbc, DexType, GetAmmConfig, MeteoraDlmmPoolUpdate, PoolState, PoolUpdateEventType,
    RaydiumClmmAmmConfig, RaydiumClmmPoolState, RaydiumClmmPoolUpdate, RaydiumCpmmAmmConfig,
    WhirlpoolPoolState, WhirlpoolPoolUpdate,
};
use crate::pool_discovery::PoolDiscovery;
use crate::types::Token;
use crate::types::{AggregatorConfig, ChainStateUpdate, PoolUpdateEvent};
use crate::utils::{pool_update_event_to_pool_state, update_pool_state_by_event};
use anyhow::Result;
use async_trait::async_trait;
use borsh::BorshDeserialize;
use futures::stream::{self, StreamExt};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::common::high_performance_clock::get_high_perf_clock;
use solana_streamer_sdk::streaming::event_parser::core::event_parser::{
    PubkeyData, SimplifiedTokenBalance,
};
use std::collections::{HashMap, HashSet};

use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::PoolConfig;
use solana_streamer_sdk::streaming::event_parser::protocols::{
    meteora_dlmm::types::LbPair, orca_whirlpools,
};
use solana_streamer_sdk::streaming::event_parser::UnifiedEvent;
use sqlx::{Pool, Postgres};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::interval;
/// Event broadcasted to arbitrage monitors with pool data and token prices
#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ArbitragePoolUpdate {
    pub pool_address: Pubkey,
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub dex: DexType,
    /// Price of token_b in terms of token_a
    pub forward_price: f64,
    /// Price of token_a in terms of token_b
    pub reverse_price: f64,
    pub timestamp: u64,
}

/// Type alias for the complex pool storage type
type PoolStorage = Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>;
/// Type alias for token pair to pool addresses mapping
type PairToPoolsMap = Arc<RwLock<HashMap<(Pubkey, Pubkey), HashSet<Pubkey>>>>;
/// Type alias for DEX to pool addresses mapping
type DexPoolsMap = Arc<RwLock<HashMap<DexType, HashSet<Pubkey>>>>;
/// Type alias for pending pool updates buffer
type PendingUpdatesMap = Arc<Mutex<HashMap<(Pubkey, PoolUpdateEventType, i32), PoolUpdateEvent>>>;
/// Type alias for batch event receiver
type BatchEventReceiver = mpsc::UnboundedReceiver<
    Vec<(
        Vec<Box<dyn UnifiedEvent>>,
        Vec<PubkeyData>,
        Vec<u64>,
        HashMap<String, SimplifiedTokenBalance>,
    )>,
>;
/// Pool address with its DEX type for tick array fetching
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PoolForTickFetching {
    pub address: Pubkey,
    pub dex_type: DexType,
}
/// Type alias for pending pools to fetch tick arrays (pool address + DEX type)
/// Pending pools for tick fetching
type PendingPoolsForTickFetching = Arc<Mutex<HashSet<PoolForTickFetching>>>;

#[async_trait]
pub trait PoolDataProvider: GetAmmConfig + Send + Sync {
    async fn get_pool_addresses_for_pair(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
    ) -> HashSet<Pubkey>;
    async fn get_pool_state_by_address(&self, pool_address: &Pubkey) -> Option<PoolState>;
    async fn is_pool_tick_synced(&self, pool_address: &Pubkey) -> bool;

    // Added methods
    async fn get_token(&self, token_address: &Pubkey) -> Option<Token>;
    fn get_sol_price(&self) -> f64;
    async fn get_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolState>;
    async fn get_pools_for_token(&self, token_address: &Pubkey) -> Vec<PoolState>;
    async fn get_stats(&self) -> PoolManagerStats;
    fn get_db(&self) -> Arc<dyn DatabaseTrait>;
    async fn add_arbitrage_token(&self, token: Pubkey) -> Result<(), String>;
    async fn remove_arbitrage_token(&self, token: &Pubkey) -> Result<(), String>;
    async fn get_chain_state(&self) -> ChainStateUpdate;
    fn get_rpc_client(&self) -> Option<&Arc<RpcClient>>;
    async fn get_pool(&self, pool_address: &Pubkey) -> Option<Arc<PoolState>>;
    async fn is_fully_synced(&self) -> bool;
}

/// In-memory pool state manager with real-time updates
pub struct PoolStateManager {
    grpc_service: Arc<dyn GrpcServiceTrait>,
    /// Pool states indexed by pool address
    pools: PoolStorage,
    /// Pool addresses indexed by token pair
    pair_to_pools: PairToPoolsMap,
    /// DEX-specific pool addresses
    dex_pools: DexPoolsMap,
    /// Token metadata cache
    token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,

    pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
    rpc_client: Arc<RpcClient>,

    /// Broadcast channel for arbitrage pool updates (only relevant pools with monitored tokens)
    arbitrage_pool_tx: broadcast::Sender<ArbitragePoolUpdate>,

    /// Coalescing buffer: latest update per pool address
    pending_updates: PendingUpdatesMap,
    /// Coalescing buffer: latest update per pool address
    pending_updates_account_event: PendingUpdatesMap,
    pending_pools_to_fetch_tick_arrays: PendingPoolsForTickFetching,
    tick_synced_pools: Arc<Mutex<HashSet<Pubkey>>>,
    /// Database abstraction
    db: Arc<dyn DatabaseTrait>,
    price_service: Arc<dyn PriceServiceTrait>,
    chain_state: Arc<Mutex<ChainStateUpdate>>,
    chain_state_update_tx: mpsc::UnboundedSender<ChainStateUpdate>,
    raydium_clmm_amm_config_cache: Arc<RwLock<HashMap<Pubkey, RaydiumClmmAmmConfig>>>,
    raydium_cpmm_amm_config_cache: Arc<RwLock<HashMap<Pubkey, RaydiumCpmmAmmConfig>>>,
    /// Tokens to monitor for arbitrage broadcasts
    arbitrage_monitored_tokens: Arc<RwLock<HashSet<Pubkey>>>,
    /// Timestamp when the application started (used to filter stale pools)
    startup_time: SystemTime,
    /// Application configuration with DEX enable/disable flags
    config: AggregatorConfig,
    /// Meteora DBC config cache (config_address -> PoolConfig)
    dbc_configs: Arc<RwLock<HashMap<Pubkey, dbc::PoolConfig>>>,
    /// PumpFun discovery service
    pumpfun_discovery: Arc<crate::pool_discovery::pumpfun::PumpFunDiscovery>,
    /// Configured pools for arbitrage filtering (empty = no filtering)
    configured_pools: Arc<HashSet<Pubkey>>,
}

#[allow(dead_code)]
impl PoolStateManager {
    /// Create a new pool manager
    pub async fn new(
        grpc_service: Arc<dyn GrpcServiceTrait>,
        config: AggregatorConfig,
        rpc_client: Arc<RpcClient>,
        price_service: Arc<dyn PriceServiceTrait>,
        arbitrage_pool_tx: broadcast::Sender<ArbitragePoolUpdate>,
        db: Arc<dyn DatabaseTrait>,
    ) -> Self {
        let (chain_state_update_tx, chain_state_update_rx) = mpsc::unbounded_channel();
        let (pool_update_tx, pool_update_rx) = mpsc::unbounded_channel::<Vec<PoolUpdateEvent>>();

        let chain_state = Arc::new(Mutex::new(ChainStateUpdate {
            slot: 0,
            block_time: 0,
            block_hash: String::new(),
        }));

        // Initialize PumpFun discovery
        let pumpfun_discovery = Arc::new(crate::pool_discovery::pumpfun::PumpFunDiscovery::new(
            rpc_client.clone(),
        ));

        // Load configured pools for arbitrage filtering
        let configured_pools: Arc<HashSet<Pubkey>> = {
            let arbitrage_enabled = std::env::var("ENABLE_ARBITRAGE_DETECTION")
                .unwrap_or_else(|_| "false".to_string())
                .to_lowercase()
                == "true";

            if arbitrage_enabled {
                let arb_config_path = std::env::var("ARBITRAGE_CONFIG_PATH")
                    .unwrap_or_else(|_| "arbitrage_config.toml".to_string());
                match crate::arbitrage_config::ArbitrageConfig::from_file(&arb_config_path) {
                    Ok(arb_config) => {
                        let pools: HashSet<Pubkey> = arb_config
                            .get_pools()
                            .iter()
                            .filter_map(|p| p.address.parse().ok())
                            .collect();
                        log::info!(
                            "🎯 Loaded {} configured pools for arbitrage filtering",
                            pools.len()
                        );
                        Arc::new(pools)
                    }
                    Err(e) => {
                        log::warn!("Failed to load arbitrage config: {}", e);
                        Arc::new(HashSet::new())
                    }
                }
            } else {
                Arc::new(HashSet::new()) // Empty = no filtering
            }
        };

        let mut manager = Self {
            grpc_service,
            pools: Arc::new(RwLock::new(HashMap::new())),
            pair_to_pools: Arc::new(RwLock::new(HashMap::new())),
            dex_pools: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            rpc_client,
            arbitrage_pool_tx,
            pending_updates: Arc::new(Mutex::new(HashMap::new())),
            pending_updates_account_event: Arc::new(Mutex::new(HashMap::new())),
            pending_pools_to_fetch_tick_arrays: Arc::new(Mutex::new(HashSet::new())),
            tick_synced_pools: Arc::new(Mutex::new(HashSet::new())),
            db,
            price_service,
            chain_state,
            chain_state_update_tx,
            raydium_clmm_amm_config_cache: Arc::new(RwLock::new(HashMap::new())),
            raydium_cpmm_amm_config_cache: Arc::new(RwLock::new(HashMap::new())),
            arbitrage_monitored_tokens: Arc::new(RwLock::new(HashSet::new())),
            startup_time: SystemTime::now(),
            config,
            dbc_configs: Arc::new(RwLock::new(HashMap::new())),
            pumpfun_discovery,
            configured_pools,
        };

        // Load data from DB on startup
        manager.load_from_db().await;

        manager
            .start_pool_update_event_processing(pool_update_rx)
            .await;

        manager
            .start_chain_state_update_event_processing(chain_state_update_rx)
            .await;

        // start periodic flusher that applies coalesced updates
        manager.start_event_update_flusher();

        // Spawn periodic save task
        let db_clone = manager.get_db();
        let pools_clone = manager.pools.clone();
        let token_cache_clone = manager.token_cache.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30 * 60));
            loop {
                interval.tick().await;
                log::info!("Starting periodic save to Postgres...");

                // Collect tokens
                let tokens: Vec<Token> = {
                    let token_read = token_cache_clone.read().await;
                    token_read.values().cloned().collect()
                };

                // Collect pools
                let pools: Vec<PoolState> = {
                    let pools_read = pools_clone.read().await;
                    let mut entries = Vec::with_capacity(pools_read.len());
                    for v in pools_read.values() {
                        let guard = v.lock().await;
                        entries.push((*guard).clone());
                    }
                    entries
                };

                if let Err(e) = db_clone.save_tokens(&tokens).await {
                    log::error!("Failed to save tokens to Postgres: {}", e);
                } else {
                    log::info!("Saved {} tokens to Postgres", tokens.len());
                }
                if let Err(e) = db_clone.save_pools(&pools).await {
                    // Check if it's a foreign key violation (Postgres error code 23503)
                    let error_msg = e.to_string();
                    if error_msg.contains("violates foreign key constraint") {
                        // This is expected during startup/high load when tokens haven't been fetched yet
                        // We can just log a warning and retry next time
                        log::warn!(
                            "Skipping pool save due to missing tokens (FK violation). Will retry next cycle. Error: {}",
                            error_msg
                        );
                    } else {
                        log::error!("Failed to save pools to Postgres: {}", e);
                    }
                } else {
                    log::info!("Saved {} pools to Postgres", pools.len());
                }
            }
        });

        manager.start_tick_array_fetcher_flusher();

        manager
    }

    #[cfg(test)]
    pub async fn inject_pool(&self, pool: PoolState) {
        let mut pools = self.pools.write().await;
        // Map pair to pools
        let (token_a, token_b) = pool.get_tokens();
        let pool_address = pool.address();

        pools.insert(pool_address, Arc::new(Mutex::new(pool.clone())));

        let mut pair_map = self.pair_to_pools.write().await;

        // Insert both directions
        pair_map
            .entry((token_a, token_b))
            .or_insert_with(HashSet::new)
            .insert(pool_address);

        if token_a != token_b {
            pair_map
                .entry((token_b, token_a))
                .or_insert_with(HashSet::new)
                .insert(pool_address);
        }

        self.tick_synced_pools.lock().await.insert(pool_address);
    }

    #[cfg(test)]
    pub async fn inject_token(&self, token: Token) {
        let mut tokens = self.token_cache.write().await;
        tokens.insert(token.address, token);
    }

    /// Create a new pool manager for testing with mock dependencies
    #[cfg(test)]
    pub async fn new_for_testing(config: AggregatorConfig, rpc_client: Arc<RpcClient>) -> Self {
        use crate::tests::mocks::{MockDatabase, MockGrpcService, MockPriceService};

        let (pool_update_tx, _) = mpsc::unbounded_channel();
        let (chain_state_update_tx, _) = mpsc::unbounded_channel();
        let (arbitrage_pool_tx, _) = broadcast::channel(1);

        let pumpfun_discovery = Arc::new(crate::pool_discovery::pumpfun::PumpFunDiscovery::new(
            rpc_client.clone(),
        ));

        Self {
            grpc_service: Arc::new(MockGrpcService),
            pools: Arc::new(RwLock::new(HashMap::new())),
            pair_to_pools: Arc::new(RwLock::new(HashMap::new())),
            dex_pools: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            rpc_client,
            arbitrage_pool_tx,
            pending_updates: Arc::new(Mutex::new(HashMap::new())),
            pending_updates_account_event: Arc::new(Mutex::new(HashMap::new())),
            db: Arc::new(MockDatabase::new()),
            price_service: Arc::new(MockPriceService::new(150.0)),
            chain_state: Arc::new(Mutex::new(ChainStateUpdate::default())),
            chain_state_update_tx,
            raydium_clmm_amm_config_cache: Arc::new(RwLock::new(HashMap::new())),
            raydium_cpmm_amm_config_cache: Arc::new(RwLock::new(HashMap::new())),
            pending_pools_to_fetch_tick_arrays: Arc::new(Mutex::new(HashSet::new())),
            tick_synced_pools: Arc::new(Mutex::new(HashSet::new())),
            arbitrage_monitored_tokens: Arc::new(RwLock::new(HashSet::new())),
            startup_time: SystemTime::now(),
            config,
            dbc_configs: Arc::new(RwLock::new(HashMap::new())),
            pumpfun_discovery,
            configured_pools: Arc::new(HashSet::new()), // No filtering in tests
        }
    }

    pub fn get_pool_update_sender(&self) -> mpsc::UnboundedSender<Vec<PoolUpdateEvent>> {
        self.pool_update_tx.clone()
    }

    pub fn get_chain_state_update_sender(&self) -> mpsc::UnboundedSender<ChainStateUpdate> {
        self.chain_state_update_tx.clone()
    }

    /// Start pool discovery task
    pub fn start_pool_discovery_task(&self) {
        let discovery = self.pumpfun_discovery.clone();

        // Clone Arcs to use in spawned task
        let pools_cache = self.pools.clone();
        let pair_to_pools_cache = self.pair_to_pools.clone();
        let dex_pools_cache = self.dex_pools.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            log::info!("Starting PumpFun pool discovery task (one-time)...");

            // Initial delay to let other things settle
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            log::info!("Running PumpFun top runner discovery...");
            match discovery.discover_top_pools(1000).await {
                Ok(discovered_pools) => {
                    log::info!("Discovered {} PumpFun pools", discovered_pools.len());
                    if !discovered_pools.is_empty() {
                        let mut new_pools_count = 0;
                        let mut pools_to_save = Vec::new();

                        // Scope for write locks
                        {
                            let mut pools = pools_cache.write().await;
                            let mut pair_map = pair_to_pools_cache.write().await;
                            let mut dex_map = dex_pools_cache.write().await;

                            for pool in discovered_pools {
                                let pool_address = pool.address();

                                // Only add if not exists
                                if !pools.contains_key(&pool_address) {
                                    let (token_a, token_b) = pool.get_tokens();

                                    pools.insert(pool_address, Arc::new(Mutex::new(pool.clone())));

                                    // Update pair map
                                    pair_map
                                        .entry((token_a, token_b))
                                        .or_insert_with(HashSet::new)
                                        .insert(pool_address);

                                    if token_a != token_b {
                                        pair_map
                                            .entry((token_b, token_a))
                                            .or_insert_with(HashSet::new)
                                            .insert(pool_address);
                                    }

                                    // Update dex map (assuming PumpFun dex type)
                                    dex_map
                                        .entry(pool.dex())
                                        .or_insert_with(HashSet::new)
                                        .insert(pool_address);

                                    pools_to_save.push(pool);
                                    new_pools_count += 1;
                                }
                            }
                        }

                        if new_pools_count > 0 {
                            log::info!(
                                "Injected {} new PumpFun pools into manager",
                                new_pools_count
                            );
                            // Save to DB
                            if let Err(e) = db.save_pools(&pools_to_save).await {
                                log::error!("Failed to save discovered pools to DB: {}", e);
                            } else {
                                log::info!("Saved {} new pools to DB", pools_to_save.len());
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Pipeline discovery failed: {}", e);
                }
            }
        });
    }

    pub async fn start(&self) {
        log::info!("🏁 PoolStateManager::start() called!");
        println!("PoolStateManager::start() called via stdout");

        // Only run Pumpfun discovery if arbitrage detection is OFF
        let arbitrage_enabled = std::env::var("ENABLE_ARBITRAGE_DETECTION")
            .unwrap_or_else(|_| "false".to_string())
            .to_lowercase()
            == "true";

        if !arbitrage_enabled {
            log::info!("🔹 Starting pool discovery task (arbitrage off)...");
            self.start_pool_discovery_task();
        } else {
            log::info!("⏭️ Skipping pool discovery (arbitrage mode enabled)");
        }

        log::info!("🔹 Starting tick array fetcher/flusher...");
        self.start_tick_array_fetcher_flusher();

        let grpc_service = self.grpc_service.clone();
        let pool_tx = self.get_pool_update_sender();
        let chain_tx = self.get_chain_state_update_sender();

        log::info!("🔹 Spawning gRPC subscription task...");
        tokio::spawn(async move {
            log::info!("📡 gRPC subscription task executing...");
            if let Err(e) = grpc_service.subscribe_pool_updates(pool_tx, chain_tx).await {
                log::error!("❌ gRPC subscription failed: {}", e);
            } else {
                log::info!("✅ gRPC subscription active");
            }
        });
    }

    pub async fn stop(&self) {
        self.grpc_service.stop().await;
    }

    /// Subscribe to arbitrage pool updates
    /// Returns a receiver that will get pool updates with prices for monitored token pairs
    pub fn subscribe_arbitrage_updates(&self) -> broadcast::Receiver<ArbitragePoolUpdate> {
        self.arbitrage_pool_tx.subscribe()
    }

    /// Set the monitored tokens for arbitrage broadcasting
    /// This allows the pool manager to filter broadcasts to only relevant pools
    /// Only saves to DB if the token set has changed
    pub async fn set_arbitrage_monitored_tokens(&self, tokens: HashSet<Pubkey>) {
        let needs_save = {
            let mut monitored = self.arbitrage_monitored_tokens.write().await;
            // Check if the set is different
            if *monitored == tokens {
                false // No change, don't save
            } else {
                *monitored = tokens.clone();
                log::info!("Arbitrage monitoring {} tokens", monitored.len());
                true // Changed, needs save
            }
        };

        // Save to DB if changed
        if needs_save {
            let db = self.db.clone();
            tokio::spawn(async move {
                let tokens_vec: Vec<Pubkey> = tokens.into_iter().collect();
                if let Err(e) = db.save_arbitrage_tokens(&tokens_vec).await {
                    log::error!("Failed to save arbitrage tokens to DB: {}", e);
                } else {
                    log::info!("Saved arbitrage monitored tokens to DB");
                }
            });
        }
    }

    /// Get current monitored tokens for arbitrage
    pub async fn get_arbitrage_monitored_tokens(&self) -> HashSet<Pubkey> {
        let monitored = self.arbitrage_monitored_tokens.read().await;
        monitored.clone()
    }

    /// Get DBC config cache for caching configs from events
    pub fn get_dbc_config_cache(
        &self,
    ) -> Arc<RwLock<HashMap<Pubkey, crate::pool_data_types::dbc::PoolConfig>>> {
        Arc::clone(&self.dbc_configs)
    }

    /// Add a token to arbitrage monitoring and save to DB
    pub async fn add_arbitrage_token(&self, token: Pubkey) -> Result<(), String> {
        {
            let mut monitored = self.arbitrage_monitored_tokens.write().await;
            if monitored.contains(&token) {
                return Err("Token already monitored".to_string());
            }
            monitored.insert(token);
            log::info!("Added token {} to arbitrage monitoring", token);
        }

        // Save to DB asynchronously
        let db = self.db.clone();
        let tokens = self.get_arbitrage_monitored_tokens().await;
        tokio::spawn(async move {
            let tokens_vec: Vec<Pubkey> = tokens.into_iter().collect();
            if let Err(e) = db.save_arbitrage_tokens(&tokens_vec).await {
                log::error!("Failed to save arbitrage tokens to DB: {}", e);
            }
        });

        Ok(())
    }

    /// Remove a token from arbitrage monitoring and save to DB
    pub async fn remove_arbitrage_token(&self, token: &Pubkey) -> Result<(), String> {
        {
            let mut monitored = self.arbitrage_monitored_tokens.write().await;
            if !monitored.remove(token) {
                return Err("Token not found in monitored list".to_string());
            }
            log::info!("Removed token {} from arbitrage monitoring", token);
        }

        // Save to DB asynchronously
        let db = self.db.clone();
        let tokens = self.get_arbitrage_monitored_tokens().await;
        tokio::spawn(async move {
            let tokens_vec: Vec<Pubkey> = tokens.into_iter().collect();
            if let Err(e) = db.save_arbitrage_tokens(&tokens_vec).await {
                log::error!("Failed to save arbitrage tokens to DB: {}", e);
            }
        });

        Ok(())
    }

    // read set of pools with ticks, sync the pool with ticks, mark it as synced with ticks
    pub fn start_tick_array_fetcher_flusher(&self) {
        let pending_pools_to_fetch_tick_arrays =
            Arc::clone(&self.pending_pools_to_fetch_tick_arrays);
        let pools = Arc::clone(&self.pools);
        let rpc_client = self.rpc_client.clone();
        let tick_synced_pools = Arc::clone(&self.tick_synced_pools);
        let pool_update_tx = self.get_pool_update_sender();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(200));
            // raydium clmm fetcher
            let raydium_clmm_fetcher = Arc::new(TickArrayFetcher::new(
                rpc_client.clone(),
                RaydiumClmmPoolState::get_program_id(),
            ));
            let whirlpool_fetcher = Arc::new(OrcaTickArrayFetcher::new(
                rpc_client.clone(),
                WhirlpoolPoolState::get_program_id(),
            ));
            let meteora_dlmm_fetcher =
                Arc::new(MeteoraDlmmBinArrayFetcher::new(rpc_client.clone()));
            loop {
                ticker.tick().await;

                // drain pending pools to fetch tick arrays
                let draineds: Vec<PoolForTickFetching> = {
                    let mut buf = pending_pools_to_fetch_tick_arrays.lock().await;
                    if buf.is_empty() {
                        Vec::new()
                    } else {
                        let mut v = Vec::with_capacity(buf.len());
                        for p in buf.drain() {
                            v.push(p);
                        }
                        v
                    }
                };

                if draineds.is_empty() {
                    continue;
                }
                log::info!(
                    "Tick array fetcher flusher: Start fetching {} pools to process",
                    draineds.len()
                );

                // bounded concurrency
                let concurrency_limit = 5usize;
                // Process the drained pools in parallel using join!
                stream::iter(draineds.into_iter().map(|pool_for_fetch| {
                    let pools_c: PoolStorage = Arc::clone(&pools);
                    let tick_synced_pools_c = Arc::clone(&tick_synced_pools);
                    let pool_update_tx_clone = pool_update_tx.clone();
                    let raydium_clmm_fetcher_c = raydium_clmm_fetcher.clone();
                    let whirlpool_fetcher_c = whirlpool_fetcher.clone();
                    let meteora_dlmm_fetcher_c = meteora_dlmm_fetcher.clone();
                    async move {
                        let pool_id = pool_for_fetch.address;
                        let dex_type = pool_for_fetch.dex_type;

                        // check if pool_id already synced
                        {
                            let tick_synced = tick_synced_pools_c.lock().await;
                            if tick_synced.contains(&pool_id) {
                                return;
                            }
                        }

                        // first read pool state and clone it from pools
                        let pool_mutex = {
                            let pools_guard = pools_c.read().await;
                            pools_guard.get(&pool_id).cloned()
                        };

                        if let Some(pool_mutex) = &pool_mutex {
                            let pool_guard = pool_mutex.lock().await;
                            let pool_state = (*pool_guard).clone();

                            match dex_type {
                                DexType::RaydiumClmm => {
                                    // Handle Raydium CLMM pools
                                    if let PoolState::RadyiumClmm(ref clmm_pool_state) = pool_state {
                                        let tick_array_state_result = raydium_clmm_fetcher_c.fetch_all_tick_arrays(pool_id, clmm_pool_state).await;
                                        // get recv_us as time receive the tick arrays
                                        let recv_us = get_high_perf_clock();
                                        match tick_array_state_result {
                                            Ok(tick_arrays) => {
                                                // mark ticks synced pools
                                                {
                                                    let mut tick_synced = tick_synced_pools_c.lock().await;
                                                    tick_synced.insert(pool_id);
                                                }
                                                // create raw events and sending it to start_batch_event_processing

                                                tick_arrays.iter().for_each(|tick_array_state| {
                                                    let tick_array_state_event = PoolUpdateEvent::RaydiumClmm(Box::new(RaydiumClmmPoolUpdate {
                                                        slot: 0,    // dont care
                                                        transaction_index: None, // dont care
                                                        address: pool_id,
                                                        pool_state_part: None,
                                                        reserve_part: None,
                                                        tick_array_state: Some(tick_array_state.clone()),
                                                        tick_array_bitmap_extension: None,
                                                        last_updated: recv_us as u64,
                                                        is_account_state_update: true,
                                                        pool_update_event_type: PoolUpdateEventType::RaydiumClmmTickArrayStateAccount,
                                                        additional_event_type: tick_array_state.start_tick_index, // use start tick index as additional event type
                                                    }));
                                                    // send to pool update event processor
                                                    let _ = pool_update_tx_clone.send(vec![tick_array_state_event]);
                                                });
                                            }
                                            Err(e) => {
                                                log::error!(
                                                    "Failed to fetch tick arrays for Raydium CLMM pool {:?}: {:?}",
                                                    clmm_pool_state.address,
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                                DexType::Orca => {
                                    if let PoolState::OrcaWhirlpool(ref whirlpool_pool_state) = pool_state {
                                        let tick_array_state_result = whirlpool_fetcher_c.fetch_all_tick_arrays(pool_id, whirlpool_pool_state).await;
                                        // get recv_us as time receive the tick arrays
                                        let recv_us = get_high_perf_clock();
                                        match tick_array_state_result {
                                            Ok(tick_arrays) => {
                                                // mark ticks synced pools
                                                {
                                                    let mut tick_synced = tick_synced_pools_c.lock().await;
                                                    tick_synced.insert(pool_id);
                                                }
                                                // create raw events and sending it to start_batch_event_processing

                                                tick_arrays.iter().for_each(|tick_array_state| {
                                                    let pu = WhirlpoolPoolUpdate {
                                                        slot: 0,    // dont care
                                                        transaction_index: None, // dont care
                                                        address: pool_id,
                                                        pool_state_part: None,
                                                        reserve_part: None,
                                                        tick_array_state: Some(orca_whirlpools::types::TickArrayState {
                                                            whirlpool: {
                                                                let pk_str = tick_array_state.whirlpool().to_string();
                                                                pk_str.parse().unwrap_or_else(|_| Default::default())
                                                            },
                                                            start_tick_index: tick_array_state.start_tick_index(),
                                                            ticks: {
                                                                let tick_vec: Vec<_> = tick_array_state.ticks().iter().map(|tick| orca_whirlpools::types::Tick {
                                                                    initialized: tick.initialized,
                                                                    liquidity_net: tick.liquidity_net,
                                                                    liquidity_gross: tick.liquidity_gross,
                                                                    fee_growth_outside_a: tick.fee_growth_outside_a,
                                                                    fee_growth_outside_b: tick.fee_growth_outside_b,
                                                                    reward_growths_outside: tick.reward_growths_outside,
                                                                }).collect();
                                                                tick_vec.try_into().unwrap_or_else(|v: Vec<_>| {
                                                                    panic!("Expected a Vec of length 88 but got {}", v.len())
                                                                })
                                                            },
                                                        }),
                                                        oracle_state: None,
                                                        last_updated: recv_us as u64,
                                                        is_account_state_update: true,
                                                        pool_update_event_type: PoolUpdateEventType::WhirlpoolTickArrayStateAccount,
                                                        additional_event_type: tick_array_state.start_tick_index(), // use start tick index as additional event type
                                                    };
                                                    let tick_array_state_event = PoolUpdateEvent::Whirlpool(Box::new(pu));
                                                    // send to pool update event processor
                                                    let _ = pool_update_tx_clone.send(vec![tick_array_state_event]);
                                                });
                                                log::info!("Tick arrays fetched for Orca Whirlpool {:?}", whirlpool_pool_state.address);
                                            }
                                            Err(e) => {
                                                log::warn!(
                                                    "Failed to fetch tick arrays for Orca Whirlpool {:?}: {:?}. Marking as synced anyway to allow routing.",
                                                    whirlpool_pool_state.address,
                                                    e
                                                );
                                                // Even if tick array fetch fails, mark as synced so the pool can be used for routing
                                                // This allows routing with just the current pool state without full tick traversal
                                                {
                                                    let mut tick_synced = tick_synced_pools_c.lock().await;
                                                    tick_synced.insert(pool_id);
                                                }
                                            }
                                        }
                                    }
                                }
                                DexType::MeteoraDlmm => {
                                    if let PoolState::MeteoraDlmm(ref dlmm_pool_state) = pool_state {
                                        let bin_arrays_result = meteora_dlmm_fetcher_c.fetch_all_bin_arrays(pool_id, dlmm_pool_state).await;
                                        let recv_us = get_high_perf_clock();
                                        match bin_arrays_result {
                                            Ok(bin_arrays) => {
                                                // log::info!("Fetched {} bin arrays for Meteora DLMM pool {}", bin_arrays.len(), pool_id);
                                                {
                                                    let mut tick_synced = tick_synced_pools_c.lock().await;
                                                    tick_synced.insert(pool_id);
                                                }
                                                bin_arrays.iter().for_each(|bin_array| {
                                                    let mut bin_arrays_map = HashMap::new();
                                                    bin_arrays_map.insert(bin_array.index as i32, bin_array.clone());
                                                    let event = PoolUpdateEvent::MeteoraDlmm(Box::new(MeteoraDlmmPoolUpdate {
                                                        slot: 0,
                                                        transaction_index: None,
                                                        address: pool_id,
                                                        lbpair: LbPair::default(),
                                                        bin_arrays: Some(bin_arrays_map),
                                                        bitmap_extension: None,
                                                        is_account_state_update: true,
                                                        pool_update_event_type: PoolUpdateEventType::MeteoraDlmmBinArrayAccount,
                                                        additional_event_type: bin_array.index as i32,
                                                        last_updated: recv_us as u64,
                                                        reserve_x: None,
                                                        reserve_y: None,
                                                    }));
                                                    let _ = pool_update_tx_clone.send(vec![event]);
                                                });
                                            }
                                            Err(e) => {
                                                log::error!("Failed to fetch bin arrays for Meteora DLMM pool {:?}: {:?}", dlmm_pool_state.address, e);
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    // Other DEX types don't have tick arrays, mark as synced
                                    log::debug!(
                                        "Pool {:?} (DEX: {:?}) does not support tick arrays",
                                        pool_id,
                                        dex_type
                                    );
                                    {
                                        let mut tick_synced = tick_synced_pools_c.lock().await;
                                        tick_synced.insert(pool_id);
                                    }
                                }
                            }
                        } else {
                            // pool not found, mark as synced to avoid repeated attempts
                            let mut tick_synced = tick_synced_pools_c.lock().await;
                            tick_synced.insert(pool_id);
                        }
                    }
                }))
                .buffer_unordered(concurrency_limit)
                .collect::<Vec<()>>()
                .await;
            }
        });
    }

    // read pending batches of formatted pool update events and apply them to chain state
    pub fn start_event_update_flusher(&self) {
        let pending = Arc::clone(&self.pending_updates);
        let pending_account = Arc::clone(&self.pending_updates_account_event);
        let pending_pools_to_fetch_tick_arrays =
            Arc::clone(&self.pending_pools_to_fetch_tick_arrays);
        let tick_synced_pools = Arc::clone(&self.tick_synced_pools);
        let pools = Arc::clone(&self.pools);
        let pair_to_pools = Arc::clone(&self.pair_to_pools);
        let dex_pools = Arc::clone(&self.dex_pools);
        let token_cache = Arc::clone(&self.token_cache);
        let rpc_client = self.rpc_client.clone();
        let price_service = Arc::clone(&self.price_service);
        let arbitrage_pool_tx = self.arbitrage_pool_tx.clone();
        let arbitrage_monitored_tokens = Arc::clone(&self.arbitrage_monitored_tokens);
        let dbc_configs = Arc::clone(&self.dbc_configs);
        let configured_pools = Arc::clone(&self.configured_pools);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(50));

            // Windowed aggregation for flusher metrics (10s window)
            let mut window_start = std::time::Instant::now();
            let mut window_total_events: u64 = 0;
            let mut window_total_apply_duration = Duration::ZERO;
            let mut window_iterations: u64 = 0;

            loop {
                ticker.tick().await;

                // read sol price
                let sol_price = price_service.get_sol_price();
                if sol_price == 0.0 {
                    log::warn!("SOL price is zero, skipping flusher iteration");
                    continue;
                }

                // measure drain start
                let drain_start = std::time::Instant::now();
                // drain pending updates quickly
                let draineds_account_event: Vec<PoolUpdateEvent> = {
                    let mut buf = pending_account.lock().await;
                    if buf.is_empty() {
                        Vec::new()
                    } else {
                        let mut v = Vec::with_capacity(buf.len());
                        for (_k, v_event) in buf.drain() {
                            v.push(v_event);
                        }
                        v
                    }
                };

                let draineds: Vec<PoolUpdateEvent> = {
                    let mut buf = pending.lock().await;
                    if buf.is_empty() {
                        Vec::new()
                    } else {
                        let mut v = Vec::with_capacity(buf.len());
                        for (_k, v_event) in buf.drain() {
                            v.push(v_event);
                        }
                        v
                    }
                };
                let _ = drain_start.elapsed();

                // instrumentation: how many updates we drained
                let count_account = draineds_account_event.len();
                let count_normal = draineds.len();
                let total_count = count_account + count_normal;

                // bounded concurrency (hybrid)
                let concurrency_limit = 64usize;

                // Get a snapshot of monitored tokens
                let monitored_tokens_snapshot = {
                    let tokens = arbitrage_monitored_tokens.read().await;
                    tokens.clone()
                };

                // Process account events and normal events in parallel using join!
                let apply_start = std::time::Instant::now();
                let (_, _) = tokio::join!(
                    // Account events processing
                    async {
                        let start = std::time::Instant::now();
                        stream::iter(draineds_account_event.into_iter().map(|update| {
                            let pools_c = Arc::clone(&pools);
                            let pair_to_pools_c = Arc::clone(&pair_to_pools);
                            let dex_pools_c = Arc::clone(&dex_pools);
                            let token_cache_c = Arc::clone(&token_cache);
                            let rpc_client_c = Arc::clone(&rpc_client);
                            let pending_pools_to_fetch_tick_arrays_c =
                                Arc::clone(&pending_pools_to_fetch_tick_arrays);
                            let tick_synced_pools_c = Arc::clone(&tick_synced_pools);
                            let arbitrage_pool_tx_c = arbitrage_pool_tx.clone();
                            let monitored_tokens_c = monitored_tokens_snapshot.clone();
                            let dbc_configs_c = Arc::clone(&dbc_configs);
                            let configured_pools_c = Arc::clone(&configured_pools);
                            async move {
                                Self::apply_pool_update(
                                    &update,
                                    pools_c,
                                    pair_to_pools_c,
                                    dex_pools_c,
                                    token_cache_c,
                                    pending_pools_to_fetch_tick_arrays_c,
                                    tick_synced_pools_c,
                                    rpc_client_c,
                                    sol_price,
                                    &arbitrage_pool_tx_c,
                                    &monitored_tokens_c,
                                    dbc_configs_c,
                                    &configured_pools_c,
                                )
                                .await;
                            }
                        }))
                        .buffer_unordered(concurrency_limit)
                        .collect::<Vec<()>>()
                        .await;
                        start.elapsed()
                    },
                    // Normal events processing
                    async {
                        let start = std::time::Instant::now();
                        stream::iter(draineds.into_iter().map(|update| {
                            let pools_c = Arc::clone(&pools);
                            let pair_to_pools_c = Arc::clone(&pair_to_pools);
                            let dex_pools_c = Arc::clone(&dex_pools);
                            let token_cache_c = Arc::clone(&token_cache);
                            let rpc_client_c = Arc::clone(&rpc_client);
                            let pending_pools_to_fetch_tick_arrays_c =
                                Arc::clone(&pending_pools_to_fetch_tick_arrays);
                            let tick_synced_pools_c = Arc::clone(&tick_synced_pools);
                            let arbitrage_pool_tx_c = arbitrage_pool_tx.clone();
                            let monitored_tokens_c = monitored_tokens_snapshot.clone();
                            let dbc_configs_c = Arc::clone(&dbc_configs);
                            let configured_pools_c = Arc::clone(&configured_pools);
                            async move {
                                Self::apply_pool_update(
                                    &update,
                                    pools_c,
                                    pair_to_pools_c,
                                    dex_pools_c,
                                    token_cache_c,
                                    pending_pools_to_fetch_tick_arrays_c,
                                    tick_synced_pools_c,
                                    rpc_client_c,
                                    sol_price,
                                    &arbitrage_pool_tx_c,
                                    &monitored_tokens_c,
                                    dbc_configs_c,
                                    &configured_pools_c,
                                )
                                .await;
                            }
                        }))
                        .buffer_unordered(concurrency_limit)
                        .collect::<Vec<()>>()
                        .await;
                        start.elapsed()
                    }
                );

                let total_apply_ns = apply_start.elapsed();

                // Aggregate metrics into sliding 10s window
                window_total_events = window_total_events.saturating_add(total_count as u64);
                window_total_apply_duration =
                    window_total_apply_duration.saturating_add(total_apply_ns);
                window_iterations = window_iterations.saturating_add(1);

                // Emit aggregated log once every 10s summarizing the last window
                if window_start.elapsed() >= Duration::from_secs(10) {
                    let avg_per_update_ms = if window_total_events > 0 {
                        (window_total_apply_duration.as_millis() as f64)
                            / (window_total_events as f64)
                    } else {
                        0.0
                    };

                    log::info!(
                        "Flusher apply (parallel) last_10s: total_event_count {}, handle time {:?}, avg {:.3} ms/update, iterations {}, concurrency={}",
                        window_total_events,
                        window_total_apply_duration,
                        avg_per_update_ms,
                        window_iterations,
                        concurrency_limit
                    );

                    // reset window
                    window_start = std::time::Instant::now();
                    window_total_events = 0;
                    window_total_apply_duration = Duration::ZERO;
                    window_iterations = 0;
                }
            }
        });
    }

    // add new pools to pending_pools_to_fetch_tick_arrays
    async fn add_new_pools_for_fetch_ticks(
        pending_pools_to_fetch_tick_arrays: PendingPoolsForTickFetching,
        pool_set: Vec<PoolForTickFetching>,
    ) {
        let mut pending = pending_pools_to_fetch_tick_arrays.lock().await;
        pool_set.into_iter().for_each(|p| {
            pending.insert(p);
        });
    }

    // receive raw batches of unified events, parse them into PoolUpdateEvents, and send to pool_update_tx for start_pool_update_event_processing to handle
    pub fn start_batch_event_processing(
        mut batch_rx: BatchEventReceiver,
        pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        chain_state_update_tx: mpsc::UnboundedSender<ChainStateUpdate>,
    ) {
        // run in its own task
        tokio::spawn(async move {
            log::info!("Starting batch event processing loop...");

            while let Some(batch) = batch_rx.recv().await {
                log::debug!("Received batch of {} events for processing", batch.len());

                // Process the batch using the existing method
                BatchProcessor::process_batch(
                    batch,
                    pool_update_tx.clone(),
                    chain_state_update_tx.clone(),
                )
                .await;
            }

            log::info!("Batch event processing loop ended - no more batches to process");
        });
    }

    // receive formatted pool update events and coalesce them into pending_updates
    async fn start_pool_update_event_processing(
        &self,
        mut pool_update_rx: mpsc::UnboundedReceiver<Vec<PoolUpdateEvent>>,
    ) {
        log::info!("Starting pool update event processing loop...");

        let pending = Arc::clone(&self.pending_updates);
        let pending_account = Arc::clone(&self.pending_updates_account_event);

        tokio::spawn(async move {
            while let Some(updates) = pool_update_rx.recv().await {
                // move updates into pending buffers, latest-wins per pool address
                // hold the lock only while inserting (keeps lock time short)
                {
                    let mut buf = pending.lock().await;
                    for update in updates.iter() {
                        // use the event address as key; clone the event for the buffer
                        if !update.is_account_state_update() {
                            let key = (
                                update.address(),
                                update.get_pool_update_event_type(),
                                update.get_additional_event_type(),
                            );
                            if let Some(existing) = buf.get(&key) {
                                // keep the one with the latest last_updated
                                if update.recv_us() > existing.recv_us() {
                                    buf.insert(key, update.clone());
                                }
                            } else {
                                buf.insert(key, update.clone());
                            }
                        }
                    }
                }

                {
                    let mut buf = pending_account.lock().await;
                    for update in updates.iter() {
                        // use the event address as key; clone the event for the buffer
                        if update.is_account_state_update() {
                            let key = (
                                update.address(),
                                update.get_pool_update_event_type(),
                                update.get_additional_event_type(),
                            );
                            if let Some(existing) = buf.get(&key) {
                                // keep the one with the latest last_updated
                                if update.recv_us() > existing.recv_us() {
                                    buf.insert(key, update.clone());
                                }
                            } else {
                                buf.insert(key, update.clone());
                            }
                        }
                    }
                }
            }

            log::info!("Pool update processing loop ended");
        });
    }

    async fn start_chain_state_update_event_processing(
        &self,
        mut chain_state_update_rx: mpsc::UnboundedReceiver<ChainStateUpdate>,
    ) {
        log::info!("Starting chain state update event processing loop...");

        let chain_state = Arc::clone(&self.chain_state);

        tokio::spawn(async move {
            while let Some(update) = chain_state_update_rx.recv().await {
                let mut state = chain_state.lock().await;
                *state = update;
            }

            log::info!("Chain state update processing loop ended");
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_pool_update(
        update: &PoolUpdateEvent,
        pools: PoolStorage,
        pair_to_pools: PairToPoolsMap,
        dex_pools: DexPoolsMap,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        pending_pools_to_fetch_tick_arrays: PendingPoolsForTickFetching,
        tick_synced_pools: Arc<Mutex<HashSet<Pubkey>>>,
        rpc_client: Arc<RpcClient>,
        sol_price: f64,
        arbitrage_pool_tx: &broadcast::Sender<ArbitragePoolUpdate>,
        arbitrage_monitored_tokens: &HashSet<Pubkey>,
        dbc_configs: Arc<RwLock<HashMap<Pubkey, crate::pool_data_types::dbc::PoolConfig>>>,
        configured_pools: &HashSet<Pubkey>,
    ) {
        // In arbitrage mode, skip events for pools not in the configured set
        if !configured_pools.is_empty() {
            let pool_address = update.address();
            if !configured_pools.contains(&pool_address) {
                log::debug!("Skipping update for non-configured pool: {}", pool_address);
                return;
            }
        }

        // Cache DBC config if this is a config update
        if let PoolUpdateEvent::MeteoraDbc(dbc_update) = update {
            if dbc_update.is_config_update {
                if let Some(config) = &dbc_update.pool_config {
                    let mut configs_write = dbc_configs.write().await;
                    configs_write.insert(dbc_update.config, config.clone());
                    log::info!("Cached DBC config: {}", dbc_update.config);

                    // If this is a config-only update (no pool data), return early
                    if dbc_update.base_mint == Pubkey::default() {
                        return;
                    }
                }
            }

            // If this is a pool update without config, try to fetch config from RPC
            if !dbc_update.is_config_update && dbc_update.pool_config.is_none() {
                let config_exists = {
                    let configs_read = dbc_configs.read().await;
                    configs_read.contains_key(&dbc_update.config)
                };

                if !config_exists {
                    log::info!("Fetching DBC config from RPC: {}", dbc_update.config);
                    match fetch_account_data(&rpc_client, &dbc_update.config).await {
                        Ok(data) => {
                            // Skip 8-byte discriminator for Anchor accounts
                            if data.len() > 8 {
                                match borsh::from_slice::<solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::PoolConfig>(&data[8..]) {
                                    Ok(config) => {
                                        let mut configs_write = dbc_configs.write().await;
                                        configs_write.insert(dbc_update.config, config.clone());
                                        log::info!("Successfully fetched and cached DBC config: {}", dbc_update.config);
                                    }
                                    Err(e) => {
                                        log::error!("Failed to deserialize DBC config {}: {:?}", dbc_update.config, e);
                                    }
                                }
                            } else {
                                log::error!(
                                    "DBC config account {} data too short: {} bytes",
                                    dbc_update.config,
                                    data.len()
                                );
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to fetch DBC config {} from RPC: {:?}",
                                dbc_update.config,
                                e
                            );
                        }
                    }
                }
            }
        }

        let pool_address = update.address();
        let mut pool_with_ticks = false;
        let mut pool_dex_type: Option<DexType> = None;

        // check if pool exists
        let pool_exists = {
            let pools_read = pools.read().await;
            pools_read.contains_key(&pool_address)
        };

        if pool_exists {
            // Get the pool's individual mutex (no blocking other pools)
            let pool_mutex = {
                let pools_read = pools.read().await;
                pools_read.get(&pool_address).cloned()
            };

            if let Some(pool_mutex) = pool_mutex {
                let mut pool_guard = pool_mutex.lock().await;
                pool_with_ticks = update_pool_state_by_event(update, &mut pool_guard, sol_price);
                pool_dex_type = Some(pool_guard.dex());
            }
        } else {
            // Insert new pool
            let (pool_state, is_pool_with_ticks) = {
                let dbc_configs_read = dbc_configs.read().await;
                pool_update_event_to_pool_state(update, sol_price, Some(&*dbc_configs_read))
            };
            pool_with_ticks = is_pool_with_ticks;

            if let Some(pool_state) = pool_state {
                let (token0, token1) = pool_state.get_tokens();
                if token0 == Pubkey::default() || token1 == Pubkey::default() {
                    return;
                }
                pool_dex_type = Some(pool_state.dex());

                Self::insert_new_pool(
                    pool_state,
                    pools.clone(),
                    pair_to_pools,
                    dex_pools,
                    token_cache.clone(),
                    rpc_client,
                )
                .await;
            }
        }

        if pool_with_ticks {
            if let Some(dex_type) = pool_dex_type {
                let tick_synced = tick_synced_pools.lock().await;
                if !tick_synced.contains(&pool_address) {
                    drop(tick_synced); // release lock early
                    let mut pending_fetch = pending_pools_to_fetch_tick_arrays.lock().await;
                    pending_fetch.insert(PoolForTickFetching {
                        address: pool_address,
                        dex_type,
                    });
                }
            }
        }

        // Broadcast arbitrage update if pool contains monitored tokens
        if !arbitrage_monitored_tokens.is_empty() {
            if let Some(pool_mutex) = {
                let pools_read = pools.read().await;
                pools_read.get(&pool_address).cloned()
            } {
                let pool_guard = pool_mutex.lock().await;
                let (token_a, token_b) = pool_guard.get_tokens();

                // Check if this pool involves any monitored tokens
                if arbitrage_monitored_tokens.contains(&token_a)
                    || arbitrage_monitored_tokens.contains(&token_b)
                {
                    // Get decimals from token cache
                    let token_cache_read = token_cache.read().await;
                    let decimals_a = token_cache_read
                        .get(&token_a)
                        .map(|t| t.decimals)
                        .unwrap_or(6);
                    let decimals_b = token_cache_read
                        .get(&token_b)
                        .map(|t| t.decimals)
                        .unwrap_or(9);
                    drop(token_cache_read);

                    let (price_a, price_b) =
                        pool_guard.calculate_token_prices(sol_price, decimals_a, decimals_b);

                    // Calculate prices in both directions
                    let (forward_price, reverse_price) =
                        if arbitrage_monitored_tokens.contains(&token_a) {
                            // Forward: token_a -> token_b, Reverse: token_b -> token_a
                            (price_b, price_a)
                        } else {
                            // Swap if token_b is primary
                            (price_b, price_a)
                        };

                    let broadcast_event = ArbitragePoolUpdate {
                        pool_address,
                        token_a,
                        token_b,
                        dex: pool_guard.dex(),
                        forward_price,
                        reverse_price,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };

                    // Broadcast to all subscribers (ignore if no receivers)
                    let _ = arbitrage_pool_tx.send(broadcast_event);
                }
            }
        }
    }

    async fn insert_new_pool(
        pool_state: PoolState,
        pools: PoolStorage,
        pair_to_pools: PairToPoolsMap,
        dex_pools: DexPoolsMap,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        rpc_client: Arc<RpcClient>,
    ) {
        let pool_address = pool_state.address();
        let dex = pool_state.dex();
        let (token_a, token_b) = pool_state.get_tokens();

        // Insert pool
        {
            let mut pools_write = pools.write().await;
            match pools_write.entry(pool_address) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    // move pool_state in (no clone)
                    v.insert(Arc::new(Mutex::new(pool_state)));
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    // Another task inserted concurrently — keep existing one
                    log::warn!(
                        "Pool {:?} was inserted concurrently, skipping insert",
                        pool_address
                    );
                    return;
                }
            }
        }

        // Update mappings
        {
            let mut pair_to_pools_write = pair_to_pools.write().await;
            pair_to_pools_write
                .entry((token_a, token_b))
                .or_insert_with(HashSet::new)
                .insert(pool_address);
            if (token_a, token_b) != (token_b, token_a) {
                pair_to_pools_write
                    .entry((token_b, token_a))
                    .or_insert_with(HashSet::new)
                    .insert(pool_address);
            }
        }

        {
            let mut dex_pools_write = dex_pools.write().await;
            dex_pools_write
                .entry(dex)
                .or_insert_with(HashSet::new)
                .insert(pool_address);
        }

        // check if token metadata is cached, if not cache it
        let token_cache_read = token_cache.read().await;
        let token_a_cached = token_cache_read.contains_key(&token_a);
        let token_b_cached = token_cache_read.contains_key(&token_b);
        drop(token_cache_read); // release read lock early

        // fetch and cache token metadata if not cached
        if !token_a_cached {
            if let Ok(token_a_info) = fetch_token(&token_a, &rpc_client).await {
                let mut token_cache_write = token_cache.write().await;
                token_cache_write.insert(token_a, token_a_info);
            }
        }

        if !token_b_cached {
            if let Ok(token_b_info) = fetch_token(&token_b, &rpc_client).await {
                let mut token_cache_write = token_cache.write().await;
                token_cache_write.insert(token_b, token_b_info);
            }
        }
    }

    /// Check if a pool is stale (hasn't been updated since app startup)
    /// Returns true if the pool's last update was before the application started + 10 seconds
    fn is_pool_stale(&self, pool: &PoolState) -> bool {
        let pool_last_update = SystemTime::UNIX_EPOCH + Duration::from_micros(pool.last_updated());
        pool_last_update < self.startup_time + Duration::from_secs(10)
    }

    pub async fn is_pool_tick_synced(&self, pool_address: &Pubkey) -> bool {
        let tick_synced = self.tick_synced_pools.lock().await;
        tick_synced.contains(pool_address)
    }

    /// Get pool state by address
    pub async fn get_pool(&self, pool_address: &Pubkey) -> Option<PoolState> {
        let pools = self.pools.read().await;
        if let Some(pool_mutex) = pools.get(pool_address) {
            let pool_guard = pool_mutex.lock().await;
            Some((*pool_guard).clone())
        } else {
            None
        }
    }

    /// Get all pools for a token pair (excluding stale pools)
    pub async fn get_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolState> {
        // Step 1: Get pool addresses (quick map read)
        let pool_addresses = {
            let pair_to_pools = self.pair_to_pools.read().await;
            let key = if token_a < token_b {
                (*token_a, *token_b)
            } else {
                (*token_b, *token_a)
            };
            pair_to_pools.get(&key).cloned().unwrap_or_default()
        };

        // Step 2: Get pool mutexes (another quick map read)
        let pool_mutexes = {
            let pools = self.pools.read().await;
            pool_addresses
                .iter()
                .filter_map(|addr| pools.get(addr).cloned())
                .collect::<Vec<_>>()
        };

        // Step 3: Read pools CONCURRENTLY and filter out stale ones
        let tasks: Vec<_> = pool_mutexes
            .into_iter()
            .map(|mutex| {
                tokio::spawn(async move {
                    let pool_guard = mutex.lock().await;
                    (*pool_guard).clone()
                })
            })
            .collect();

        let mut results = Vec::new();
        for task in tasks {
            if let Ok(pool) = task.await {
                // Exclude stale pools
                if !self.is_pool_stale(&pool) {
                    results.push(pool);
                }
            }
        }
        results
    }

    pub async fn get_pool_states_by_addresses(
        &self,
        pool_addresses: &HashSet<Pubkey>,
    ) -> HashMap<Pubkey, PoolState> {
        let pool_mutexes = {
            let pools = self.pools.read().await;
            pool_addresses
                .iter()
                .filter_map(|addr| pools.get(addr).cloned())
                .collect::<Vec<_>>()
        };

        // Read pools CONCURRENTLY
        let tasks: Vec<_> = pool_mutexes
            .into_iter()
            .map(|mutex| {
                tokio::spawn(async move {
                    let pool_guard = mutex.lock().await;
                    (*pool_guard).clone()
                })
            })
            .collect();

        let mut results = HashMap::new();
        for task in tasks {
            if let Ok(pool) = task.await {
                // Exclude stale pools
                if !self.is_pool_stale(&pool) {
                    results.insert(pool.address(), pool);
                }
            }
        }
        results
    }

    pub async fn get_pool_state_by_address(&self, pool_address: &Pubkey) -> Option<PoolState> {
        let pools = self.pools.read().await;
        if let Some(pool_mutex) = pools.get(pool_address) {
            let pool_guard = pool_mutex.lock().await;
            if !self.is_pool_stale(&(*pool_guard).clone()) {
                Some((*pool_guard).clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get access to the underlying Postgres pool
    pub fn get_db(&self) -> Arc<dyn DatabaseTrait> {
        self.db.clone()
    }

    /// Get access to the RPC client
    pub fn get_rpc_client(&self) -> Arc<RpcClient> {
        Arc::clone(&self.rpc_client)
    }

    pub async fn get_pool_addresses_for_pair(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
    ) -> HashSet<Pubkey> {
        let pair_to_pools = self.pair_to_pools.read().await;
        let key = if token_a < token_b {
            (*token_a, *token_b)
        } else {
            (*token_b, *token_a)
        };
        pair_to_pools.get(&key).cloned().unwrap_or_default()
    }

    pub async fn get_pool_count_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> usize {
        let pair_to_pools = self.pair_to_pools.read().await;
        let key = if token_a < token_b {
            (*token_a, *token_b)
        } else {
            (*token_b, *token_a)
        };
        pair_to_pools.get(&key).map(|s| s.len()).unwrap_or(0)
    }

    /// Get token metadata from cache
    pub async fn get_token(&self, token_address: &Pubkey) -> Option<Token> {
        let cache = self.token_cache.read().await;
        cache.get(token_address).cloned()
    }

    /// Store token metadata in cache
    pub async fn store_token(&self, token: Token) {
        let mut cache = self.token_cache.write().await;
        cache.insert(token.address, token);
    }

    /// Get pools for a specific DEX (excluding stale pools)
    pub async fn get_pools_for_dex(&self, dex: DexType) -> Vec<PoolState> {
        // Step 1: Get pool addresses for this DEX
        let pool_addresses = {
            let dex_pools = self.dex_pools.read().await;
            dex_pools.get(&dex).cloned().unwrap_or_default()
        };

        // Step 2: Get pool mutexes
        let pool_mutexes = {
            let pools = self.pools.read().await;
            pool_addresses
                .iter()
                .filter_map(|addr| pools.get(addr).cloned())
                .collect::<Vec<_>>()
        };

        // Step 3: Read all pools concurrently and filter out stale ones
        let tasks: Vec<_> = pool_mutexes
            .into_iter()
            .map(|mutex| {
                tokio::spawn(async move {
                    let pool_guard = mutex.lock().await;
                    (*pool_guard).clone()
                })
            })
            .collect();

        let mut results = Vec::new();
        for task in tasks {
            if let Ok(pool) = task.await {
                // Exclude stale pools
                if !self.is_pool_stale(&pool) {
                    results.push(pool);
                }
            }
        }
        results
    }

    /// Remove a pool from the manager
    pub async fn remove_pool(&self, pool_address: &Pubkey) {
        let mut pools = self.pools.write().await;
        pools.remove(pool_address);

        // Note: We don't remove from other mappings for performance reasons
        // These will be cleaned up periodically
    }

    /// Get all cached tokens
    pub async fn get_all_tokens(&self) -> Vec<Token> {
        let token_cache = self.token_cache.read().await;
        token_cache.values().cloned().collect()
    }

    /// Get pool statistics
    pub async fn get_stats(&self) -> PoolManagerStats {
        let pools = self.pools.read().await;
        let pair_to_pools = self.pair_to_pools.read().await;
        let dex_pools = self.dex_pools.read().await;
        let token_cache = self.token_cache.read().await;

        PoolManagerStats {
            total_pools: pools.len(),
            total_pairs: pair_to_pools.len(),
            total_tokens: token_cache.len(),
            pools_by_dex: dex_pools
                .iter()
                .map(|(dex, pools)| (*dex, pools.len()))
                .collect(),
        }
    }

    /// Get all pools containing a specific token
    pub async fn get_pools_for_token(&self, token_address: &Pubkey) -> Vec<PoolState> {
        // Collect all pool mutexes first under read lock
        let pool_mutexes = {
            let pools = self.pools.read().await;
            pools.values().cloned().collect::<Vec<_>>()
        };

        let mut results = Vec::new();
        for mutex in pool_mutexes {
            let pool_guard = mutex.lock().await;
            let pool_cloned = (*pool_guard).clone();
            let (token_a, token_b) = pool_cloned.get_tokens();
            if (&token_a == token_address || &token_b == token_address)
                && !self.is_pool_stale(&pool_cloned)
            {
                results.push(pool_cloned);
            }
        }
        results
    }

    /// Load data from Postgres into in-memory structures
    async fn load_from_db(&mut self) {
        let arbitrage_enabled = std::env::var("ENABLE_ARBITRAGE_DETECTION")
            .unwrap_or_else(|_| "false".to_string())
            .to_lowercase()
            == "true";

        // 1. Load Pools (skip postgres in arbitrage mode - fetch fresh from RPC)
        if arbitrage_enabled {
            log::info!("⚡ Arbitrage mode: fetching configured pools from RPC...");

            // Load arbitrage config to get pool list
            let config_path = std::env::var("ARBITRAGE_CONFIG_PATH")
                .unwrap_or_else(|_| "arbitrage_config.toml".to_string());

            match crate::arbitrage_config::ArbitrageConfig::from_file(&config_path) {
                Ok(arb_config) => {
                    let enabled_pools = arb_config.get_pools();
                    log::info!("  Found {} enabled pools in config", enabled_pools.len());

                    // Fetch all pools from RPC
                    let fetched_pools = crate::fetchers::pool_fetcher::fetch_configured_pools(
                        &self.rpc_client,
                        &enabled_pools,
                    )
                    .await;

                    // Collect unique tokens from fetched pools for caching
                    let mut unique_tokens: HashSet<Pubkey> = HashSet::new();
                    for pool_state in &fetched_pools {
                        let (token_a, token_b) = pool_state.get_tokens();
                        if token_a != Pubkey::default() {
                            unique_tokens.insert(token_a);
                        }
                        if token_b != Pubkey::default() {
                            unique_tokens.insert(token_b);
                        }
                    }

                    // Inject into pool manager
                    let mut pools_write = self.pools.write().await;
                    for pool_state in fetched_pools {
                        pools_write.insert(pool_state.address(), Arc::new(Mutex::new(pool_state)));
                    }
                    drop(pools_write);
                    log::info!("✅ Loaded {} pools from RPC", self.pools.read().await.len());

                    // Fetch token metadata from RPC (critical for arbitrage routing)
                    log::info!("⚡ Fetching {} unique token metadata from RPC...", unique_tokens.len());
                    let mut token_cache_write = self.token_cache.write().await;
                    for token_addr in unique_tokens {
                        if !token_cache_write.contains_key(&token_addr) {
                            match fetch_token(&token_addr, &self.rpc_client).await {
                                Ok(token_info) => {
                                    token_cache_write.insert(token_addr, token_info);
                                }
                                Err(e) => {
                                    log::warn!("Failed to fetch token {}: {:?}", token_addr, e);
                                }
                            }
                        }
                    }
                    log::info!("✅ Cached {} tokens from RPC", token_cache_write.len());
                }
                Err(e) => {
                    log::error!("Failed to load arbitrage config: {}", e);
                }
            }
        } else {
            log::info!("Loading pool state from Postgres...");
            match self.db.load_pools().await {
                Ok(pools) => {
                    let mut pools_write = self.pools.write().await;
                    for pool_state in pools {
                        pools_write.insert(pool_state.address(), Arc::new(Mutex::new(pool_state)));
                    }
                    log::info!("Loaded {} pools from Postgres", pools_write.len());
                }
                Err(e) => {
                    log::error!("Failed to load pools from Postgres: {}", e);
                }
            }
        }

        // 2. Load Tokens (always needed for symbol/decimals)
        match self.db.load_tokens().await {
            Ok(tokens) => {
                let mut token_write = self.token_cache.write().await;
                for token in tokens {
                    token_write.insert(token.address, token);
                }
                log::info!("Loaded {} tokens from Postgres", token_write.len());
            }
            Err(e) => {
                log::error!("Failed to load tokens from Postgres: {}", e);
            }
        }

        // 3. Load Arbitrage Tokens
        match self.db.load_arbitrage_tokens().await {
            Ok(tokens) => {
                let mut set = self.arbitrage_monitored_tokens.write().await;
                for t in tokens {
                    set.insert(t);
                }
                log::info!("Loaded {} arbitrage tokens from DB", set.len());
            }
            Err(e) => {
                log::error!("Failed to load arbitrage tokens from DB: {}", e);
            }
        }

        // Rebuild mappings
        self.rebuild_mappings_from_pools().await;
    }

    /// Rebuild pair_to_pools and dex_pools mappings from existing pools
    async fn rebuild_mappings_from_pools(&self) {
        let pools_read = self.pools.read().await;
        let mut pair_to_pools_map: HashMap<(Pubkey, Pubkey), HashSet<Pubkey>> = HashMap::new();
        let mut dex_pools_map: HashMap<DexType, HashSet<Pubkey>> = HashMap::new();
        let mut raydium_clmm_amm_configs_set: HashSet<Pubkey> = HashSet::new();
        let mut raydium_cpmm_amm_configs_set: HashSet<Pubkey> = HashSet::new();

        // Check if arbitrage mode is enabled - if so, only fetch ticks for configured pools
        let arbitrage_enabled = std::env::var("ENABLE_ARBITRAGE_DETECTION")
            .unwrap_or_else(|_| "false".to_string())
            .to_lowercase()
            == "true";

        // Load configured pool addresses if arbitrage is enabled
        let configured_pools: HashSet<Pubkey> = if arbitrage_enabled {
            let arb_config_path = std::env::var("ARBITRAGE_CONFIG_PATH")
                .unwrap_or_else(|_| "arbitrage_config.toml".to_string());
            match crate::arbitrage_config::ArbitrageConfig::from_file(&arb_config_path) {
                Ok(config) => {
                    let pools: HashSet<Pubkey> = config
                        .get_pools()
                        .iter()
                        .filter_map(|p| p.address.parse().ok())
                        .collect();
                    log::info!(
                        "⚡ Arbitrage mode: Only fetching ticks for {} configured pools",
                        pools.len()
                    );
                    pools
                }
                Err(e) => {
                    log::warn!("Failed to load arbitrage config for tick filtering: {}", e);
                    HashSet::new()
                }
            }
        } else {
            HashSet::new() // Empty = fetch all
        };

        log::info!("Rebuilding mappings from {} pools...", pools_read.len());
        let _tick_fetcher = TickArrayFetcher::new(
            self.rpc_client.clone(),
            RaydiumClmmPoolState::get_program_id(),
        );

        for (pool_address, pool_mutex) in pools_read.iter() {
            // Get pool state (we know these exist since we just loaded them)
            let pool_guard = pool_mutex.lock().await; // Safe since we're loading on startup
            let pool_state = &*pool_guard;

            match &pool_state {
                PoolState::Pumpfun(_) => {
                    if !self.config.enable_pumpfun {
                        log::debug!(
                            "Skipping Pumpfun pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                }
                PoolState::PumpSwap(_) => {
                    if !self.config.enable_pumpfun_swap {
                        log::debug!(
                            "Skipping PumpSwap pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                }
                PoolState::RaydiumAmmV4(_) => {
                    if !self.config.enable_raydium_amm_v4 {
                        log::debug!(
                            "Skipping Raydium AMM V4 pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                }
                PoolState::RaydiumCpmm(cpmm_pool_state) => {
                    if !self.config.enable_raydium_cpmm {
                        log::debug!(
                            "Skipping Raydium CPMM pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                    raydium_cpmm_amm_configs_set.insert(cpmm_pool_state.amm_config);
                }
                PoolState::Bonk(_) => {
                    if !self.config.enable_bonk {
                        log::debug!(
                            "Skipping Bonk pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                }
                PoolState::RadyiumClmm(clmm_pool_state) => {
                    if !self.config.enable_raydium_clmm {
                        log::debug!(
                            "Skipping Raydium CLMM pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }

                    // add pool to pending pools to fetch tick arrays (skip if arbitrage mode and not configured)
                    if configured_pools.is_empty() || configured_pools.contains(pool_address) {
                        let mut pending = self.pending_pools_to_fetch_tick_arrays.lock().await;
                        pending.insert(PoolForTickFetching {
                            address: *pool_address,
                            dex_type: DexType::RaydiumClmm,
                        });
                        drop(pending);
                    }

                    raydium_clmm_amm_configs_set.insert(clmm_pool_state.amm_config);
                }
                PoolState::MeteoraDbc(_) => {
                    if !self.config.enable_meteora_dbc {
                        log::debug!(
                            "Skipping Meteora DBC pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                }
                PoolState::MeteoraDammV2(_) => {
                    if !self.config.enable_meteora_dammv2 {
                        log::debug!(
                            "Skipping Meteora DAMMV2 pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }
                }
                PoolState::MeteoraDlmm(_) => {
                    if !self.config.enable_meteora_dlmm {
                        log::debug!(
                            "Skipping Meteora DLMMPool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }

                    // add pool to pending pools to fetch bin arrays (skip if arbitrage mode and not configured)
                    if configured_pools.is_empty() || configured_pools.contains(pool_address) {
                        let mut pending = self.pending_pools_to_fetch_tick_arrays.lock().await;
                        pending.insert(PoolForTickFetching {
                            address: *pool_address,
                            dex_type: DexType::MeteoraDlmm,
                        });
                        drop(pending);
                    }
                }
                PoolState::OrcaWhirlpool(_) => {
                    if !self.config.enable_orca_whirlpools {
                        log::debug!(
                            "Skipping OrcaWhirlpool pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }

                    // add pool to pending pools to fetch tick arrays (skip if arbitrage mode and not configured)
                    if configured_pools.is_empty() || configured_pools.contains(pool_address) {
                        let mut pending = self.pending_pools_to_fetch_tick_arrays.lock().await;
                        pending.insert(PoolForTickFetching {
                            address: *pool_address,
                            dex_type: DexType::Orca,
                        });
                        drop(pending);
                    }
                }
            }

            let (token_a, token_b) = pool_state.get_tokens();
            let dex_type = pool_state.dex();

            // Skip pools with invalid tokens
            if token_a == Pubkey::default() || token_b == Pubkey::default() {
                continue;
            }

            // Add to pair_to_pools mapping (both directions)
            pair_to_pools_map
                .entry((token_a, token_b))
                .or_default()
                .insert(*pool_address);

            if (token_a, token_b) != (token_b, token_a) {
                pair_to_pools_map
                    .entry((token_b, token_a))
                    .or_default()
                    .insert(*pool_address);
            }

            // Add to dex_pools mapping
            dex_pools_map
                .entry(dex_type)
                .or_default()
                .insert(*pool_address);
        }

        drop(pools_read); // Release the pools read lock

        // Update the mappings
        {
            let mut pair_to_pools_write = self.pair_to_pools.write().await;
            *pair_to_pools_write = pair_to_pools_map;
            log::info!("Rebuilt {} pair mappings", pair_to_pools_write.len());
        }

        {
            let mut dex_pools_write = self.dex_pools.write().await;
            *dex_pools_write = dex_pools_map;
            log::info!("Rebuilt {} DEX mappings", dex_pools_write.len());
        }

        // fetching amm configs from on-chain in background to avoid blocking startup
        let rpc_client = self.rpc_client.clone();
        let raydium_clmm_amm_config_cache = self.raydium_clmm_amm_config_cache.clone();
        let raydium_cpmm_amm_config_cache = self.raydium_cpmm_amm_config_cache.clone();

        tokio::spawn(async move {
            log::info!("Starting background fetch of Raydium AMM configs...");

            // fetch clmm configs
            let clmm_configs_to_fetch: Vec<Pubkey> = {
                let cache = raydium_clmm_amm_config_cache.read().await;
                raydium_clmm_amm_configs_set
                    .iter()
                    .filter(|&k| !cache.contains_key(k))
                    .cloned()
                    .collect()
            };

            if !clmm_configs_to_fetch.is_empty() {
                log::info!(
                    "Fetching {} Raydium CLMM AMM configs...",
                    clmm_configs_to_fetch.len()
                );
                match fetch_multiple_accounts(&rpc_client, &clmm_configs_to_fetch).await {
                    Ok(results) => {
                        let mut cache_write = raydium_clmm_amm_config_cache.write().await;
                        for (i, opt_data) in results.into_iter().enumerate() {
                            let amm_config = clmm_configs_to_fetch[i];
                            if let Some(data) = opt_data {
                                if data.len() < 8 {
                                    continue;
                                }
                                match RaydiumClmmAmmConfig::try_from_slice(&data[8..]) {
                                    Ok(config) => {
                                        cache_write.insert(amm_config, config);
                                    }
                                    Err(e) => log::error!(
                                        "Failed to deserialize RaydiumClmmAmmConfig {}: {}",
                                        amm_config,
                                        e
                                    ),
                                }
                            }
                        }
                    }
                    Err(e) => log::error!("Failed to fetch Raydium CLMM AMM configs: {}", e),
                }
            }

            // fetch cpmm configs
            let cpmm_configs_to_fetch: Vec<Pubkey> = {
                let cache = raydium_cpmm_amm_config_cache.read().await;
                raydium_cpmm_amm_configs_set
                    .iter()
                    .filter(|&k| !cache.contains_key(k))
                    .cloned()
                    .collect()
            };

            if !cpmm_configs_to_fetch.is_empty() {
                log::info!(
                    "Fetching {} Raydium CPMM AMM configs...",
                    cpmm_configs_to_fetch.len()
                );
                match fetch_multiple_accounts(&rpc_client, &cpmm_configs_to_fetch).await {
                    Ok(results) => {
                        let mut cache_write = raydium_cpmm_amm_config_cache.write().await;
                        for (i, opt_data) in results.into_iter().enumerate() {
                            let amm_config = cpmm_configs_to_fetch[i];
                            if let Some(data) = opt_data {
                                if data.len() < 8 {
                                    continue;
                                }
                                match RaydiumCpmmAmmConfig::try_from_slice(&data[8..]) {
                                    Ok(config) => {
                                        cache_write.insert(amm_config, config);
                                    }
                                    Err(e) => log::error!(
                                        "Failed to deserialize RaydiumCpmmAmmConfig {}: {}",
                                        amm_config,
                                        e
                                    ),
                                }
                            }
                        }
                    }
                    Err(e) => log::error!("Failed to fetch Raydium CPMM AMM configs: {}", e),
                }
            }

            log::info!(
                "Loaded {} Raydium CLMM AMM configs from on-chain (background task complete)",
                raydium_clmm_amm_configs_set.len()
            );
            log::info!(
                "Loaded {} Raydium CPMM AMM configs from on-chain (background task complete)",
                raydium_cpmm_amm_configs_set.len()
            );
        });
    }

    /// Save in-memory data to Database
    pub async fn save_pools(&self) -> Result<(), Box<dyn std::error::Error>> {
        let msg = "Saving pools to database...";
        log::info!("{}", msg);

        // Collect tokens
        let tokens: Vec<Token> = {
            let token_read = self.token_cache.read().await;
            token_read.values().cloned().collect()
        };

        // Collect pools
        let pools: Vec<PoolState> = {
            let pools_read = self.pools.read().await;
            let mut entries = Vec::with_capacity(pools_read.len());
            for v in pools_read.values() {
                let guard = v.lock().await;
                entries.push((*guard).clone());
            }
            entries
        };

        self.db.save_tokens(&tokens).await?;
        self.db.save_pools(&pools).await?;
        Ok(())
    }

    async fn save_to_db(
        db: &Pool<Postgres>,
        pools: &PoolStorage,
        token_cache: &Arc<RwLock<HashMap<Pubkey, Token>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Save Tokens (Must come before pools due to FK constraints)
        let token_read = token_cache.read().await;
        // ... (Similar logic for tokens)
        let tokens: Vec<Token> = token_read.values().cloned().collect();
        drop(token_read);

        for chunk in tokens.chunks(500) {
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT INTO tokens (address, symbol, name, decimals, is_token2022, logo_uri, data) "
            );

            query_builder.push_values(chunk, |mut b, token| {
                b.push_bind(token.address.to_string())
                    .push_bind(token.symbol.clone())
                    .push_bind(token.name.clone())
                    .push_bind(token.decimals as i16)
                    .push_bind(token.is_token_2022)
                    .push_bind(token.logo_uri.clone())
                    .push_bind(sqlx::types::Json(token));
            });

            query_builder.push(
                " ON CONFLICT (address) DO UPDATE SET 
                symbol = EXCLUDED.symbol,
                name = EXCLUDED.name,
                decimals = EXCLUDED.decimals,
                is_token2022 = EXCLUDED.is_token2022,
                logo_uri = EXCLUDED.logo_uri,
                data = EXCLUDED.data,
                updated_at = NOW()",
            );

            let query = query_builder.build();
            query.execute(db).await?;
        }

        log::info!("Saved {} tokens to Postgres", tokens.len());

        // 2. Save Pools
        let pools_read = pools.read().await;
        let mut pool_count = 0;

        // Convert to vector for batch processing if needed, OR just loop insert
        // For 1000s of pools, batching is recommended, but let's do simple loop first or chunks
        // Creating a large JSONB object might be heavy, so we can iterate.
        // However, converting PoolState to JSON is the key here.

        let pool_entries: Vec<PoolState> = {
            let mut entries = Vec::with_capacity(pools_read.len());
            for v in pools_read.values() {
                let guard = v.lock().await;
                entries.push((*guard).clone());
            }
            entries
        };
        drop(pools_read); // Release lock

        for chunk in pool_entries.chunks(500) {
            // We can't easily use UNNEST with heterogeneous JSON types unless we serialize to a specific struct
            // So we loop for now, or use a specific bulk insert query.
            // Let's use loop with spawn for now to avoid complexity, or query builder.

            // Using QueryBuilder for bulk insert is efficient in sqlx
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT INTO pools (address, dex_type, token_a, token_b, data, last_updated_ts) ",
            );

            query_builder.push_values(chunk, |mut b, pool| {
                let (token_a, token_b) = pool.get_tokens();
                b.push_bind(pool.address().to_string())
                    .push_bind(pool.dex().to_string())
                    .push_bind(token_a.to_string())
                    .push_bind(token_b.to_string())
                    .push_bind(sqlx::types::Json(pool)) // This requires PoolState to deserialize to JSON which implies Serialize
                    .push_bind(pool.last_updated() as i64);
            });

            query_builder.push(
                " ON CONFLICT (address) DO UPDATE SET 
                dex_type = EXCLUDED.dex_type,
                token_a = EXCLUDED.token_a,
                token_b = EXCLUDED.token_b,
                data = EXCLUDED.data,
                last_updated_ts = EXCLUDED.last_updated_ts,
                updated_at = NOW()",
            );

            let query = query_builder.build();
            query.execute(db).await?;
            pool_count += chunk.len();
        }

        log::info!("Saved {} pools to Postgres", pool_count);

        Ok(())
    }

    pub fn get_sol_price(&self) -> f64 {
        self.price_service.get_sol_price()
    }

    pub async fn get_chain_state(&self) -> ChainStateUpdate {
        let state = self.chain_state.lock().await;
        state.clone()
    }
}

#[async_trait]
impl GetAmmConfig for PoolStateManager {
    async fn get_raydium_clmm_amm_config(
        &self,
        amm_config_address: &Pubkey,
    ) -> Result<Option<RaydiumClmmAmmConfig>> {
        let cache = self.raydium_clmm_amm_config_cache.read().await;
        if let Some(amm_config) = cache.get(amm_config_address) {
            return Ok(Some(amm_config.clone()));
        }
        drop(cache);
        match fetch_account_data(&self.rpc_client, amm_config_address).await {
            Ok(data) => {
                if data.len() < 8 {
                    log::error!(
                        "Account data too short for RaydiumClmmAmmConfig at {:?}",
                        amm_config_address
                    );
                    return Ok(None);
                }
                match RaydiumClmmAmmConfig::try_from_slice(&data[8..]) {
                    Ok(amm_config) => {
                        let mut cache_write = self.raydium_clmm_amm_config_cache.write().await;
                        cache_write.insert(*amm_config_address, amm_config.clone());
                        Ok(Some(amm_config))
                    }
                    Err(e) => {
                        log::error!("Failed to deserialize RaydiumClmmAmmConfig from on-chain data at {:?}: {:?}", amm_config_address, e);
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to fetch account data for RaydiumClmmAmmConfig at {:?}: {:?}",
                    amm_config_address,
                    e
                );
                Ok(None)
            }
        }
    }

    async fn get_raydium_cpmm_amm_config(
        &self,
        amm_config_address: &Pubkey,
    ) -> Result<Option<RaydiumCpmmAmmConfig>> {
        let cache = self.raydium_cpmm_amm_config_cache.read().await;
        if let Some(amm_config) = cache.get(amm_config_address) {
            Ok(Some(amm_config.clone()))
        } else {
            // fetch from on-chain
            drop(cache); // release read lock early
            match fetch_account_data(&self.rpc_client, amm_config_address).await {
                Ok(data) => {
                    if let Ok(amm_config) = RaydiumCpmmAmmConfig::try_from_slice(&data[8..]) {
                        // cache it
                        let mut cache_write = self.raydium_cpmm_amm_config_cache.write().await;
                        cache_write.insert(*amm_config_address, amm_config.clone());
                        Ok(Some(amm_config))
                    } else {
                        Ok(None)
                    }
                }
                Err(_) => Ok(None),
            }
        }
    }

    async fn get_dbc_pool_config(&self, _pool_address: &Pubkey) -> Result<Option<PoolConfig>> {
        // DBC pool config is not cached in this implementation
        Ok(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PoolManagerStats {
    pub total_pools: usize,
    pub total_pairs: usize,
    pub total_tokens: usize,
    pub pools_by_dex: HashMap<DexType, usize>,
}

#[async_trait]
impl PoolDataProvider for PoolStateManager {
    async fn get_pool_addresses_for_pair(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
    ) -> HashSet<Pubkey> {
        self.get_pool_addresses_for_pair(token_a, token_b).await
    }

    async fn get_pool_state_by_address(&self, pool_address: &Pubkey) -> Option<PoolState> {
        self.get_pool_state_by_address(pool_address).await
    }

    async fn is_pool_tick_synced(&self, pool_address: &Pubkey) -> bool {
        self.is_pool_tick_synced(pool_address).await
    }

    async fn get_token(&self, token_address: &Pubkey) -> Option<Token> {
        self.get_token(token_address).await
    }

    fn get_sol_price(&self) -> f64 {
        self.get_sol_price()
    }

    async fn get_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolState> {
        self.get_pools_for_pair(token_a, token_b).await
    }

    async fn get_pools_for_token(&self, token_address: &Pubkey) -> Vec<PoolState> {
        self.get_pools_for_token(token_address).await
    }

    async fn get_stats(&self) -> PoolManagerStats {
        self.get_stats().await
    }

    fn get_db(&self) -> Arc<dyn DatabaseTrait> {
        self.get_db()
    }

    async fn add_arbitrage_token(&self, token: Pubkey) -> Result<(), String> {
        self.add_arbitrage_token(token).await
    }

    async fn remove_arbitrage_token(&self, token: &Pubkey) -> Result<(), String> {
        self.remove_arbitrage_token(token).await
    }

    async fn get_chain_state(&self) -> ChainStateUpdate {
        self.get_chain_state().await
    }

    fn get_rpc_client(&self) -> Option<&Arc<RpcClient>> {
        Some(&self.rpc_client)
    }

    async fn get_pool(&self, pool_address: &Pubkey) -> Option<Arc<PoolState>> {
        if let Some(pool_mutex) = self.pools.read().await.get(pool_address) {
            let pool_state = pool_mutex.lock().await.clone();
            Some(Arc::new(pool_state))
        } else {
            None
        }
    }

    async fn is_fully_synced(&self) -> bool {
        // Check if we have loaded pools
        let pool_count = self.pools.read().await.len();
        if pool_count == 0 {
            return false;
        }

        // Check pending tick fetches - must be zero
        let pending_count = self.pending_pools_to_fetch_tick_arrays.lock().await.len();
        if pending_count > 0 {
            return false;
        }

        // Strict verification: Ensure ALL pools requiring ticks (CLMM/Whirlpool/DLMM) are marked as synced.
        // We use dex_pools map to avoid locking individual pool mutexes.
        let dex_pools = self.dex_pools.read().await;
        let synced = self.tick_synced_pools.lock().await;

        let tick_dependent_dexes = [
            DexType::Orca,
            DexType::RaydiumClmm,
            DexType::MeteoraDlmm,
        ];

        for dex_type in tick_dependent_dexes {
            if let Some(pools_of_type) = dex_pools.get(&dex_type) {
                for pool_address in pools_of_type {
                    if !synced.contains(pool_address) {
                        // Found a pool that requires ticks but is not yet synced
                        return false;
                    }
                }
            }
        }

        true
    }
}
