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
use dashmap::{DashMap, DashSet};
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
// Note: DashMap/DashSet from dashmap crate used for PoolStorage, PairToPoolsMap,
// DexPoolsMap, tick_synced_pools. tokio Mutex/RwLock still used for other fields.
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

/// Type alias for pool storage — DashMap for lock-free per-shard concurrent access
type PoolStorage = Arc<DashMap<Pubkey, PoolState>>;
/// Type alias for token pair to pool addresses mapping
type PairToPoolsMap = Arc<DashMap<(Pubkey, Pubkey), HashSet<Pubkey>>>;
/// Type alias for DEX to pool addresses mapping
type DexPoolsMap = Arc<DashMap<DexType, HashSet<Pubkey>>>;
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
    tick_synced_pools: Arc<DashSet<Pubkey>>,
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

        let mut manager = Self {
            grpc_service,
            pools: Arc::new(DashMap::new()),
            pair_to_pools: Arc::new(DashMap::new()),
            dex_pools: Arc::new(DashMap::new()),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            rpc_client,
            arbitrage_pool_tx,
            pending_updates: Arc::new(Mutex::new(HashMap::new())),
            pending_updates_account_event: Arc::new(Mutex::new(HashMap::new())),
            pending_pools_to_fetch_tick_arrays: Arc::new(Mutex::new(HashSet::new())),
            tick_synced_pools: Arc::new(DashSet::new()),
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
                    pools_clone.iter().map(|entry| entry.value().clone()).collect()
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
        let (token_a, token_b) = pool.get_tokens();
        let pool_address = pool.address();

        self.pools.insert(pool_address, pool);

        // Insert both directions
        self.pair_to_pools
            .entry((token_a, token_b))
            .or_insert_with(HashSet::new)
            .insert(pool_address);

        if token_a != token_b {
            self.pair_to_pools
                .entry((token_b, token_a))
                .or_insert_with(HashSet::new)
                .insert(pool_address);
        }

        self.tick_synced_pools.insert(pool_address);
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
            pools: Arc::new(DashMap::new()),
            pair_to_pools: Arc::new(DashMap::new()),
            dex_pools: Arc::new(DashMap::new()),
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
            tick_synced_pools: Arc::new(DashSet::new()),
            arbitrage_monitored_tokens: Arc::new(RwLock::new(HashSet::new())),
            startup_time: SystemTime::now(),
            config,
            dbc_configs: Arc::new(RwLock::new(HashMap::new())),
            pumpfun_discovery,
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

                        // Insert directly into DashMaps
                        {
                            for pool in discovered_pools {
                                let pool_address = pool.address();

                                // Only add if not exists
                                #[allow(clippy::map_entry)]
                                if !pools_cache.contains_key(&pool_address) {
                                    let (token_a, token_b) = pool.get_tokens();

                                    pools_cache.insert(pool_address, pool.clone());

                                    // Update pair map
                                    pair_to_pools_cache
                                        .entry((token_a, token_b))
                                        .or_insert_with(HashSet::new)
                                        .insert(pool_address);

                                    if token_a != token_b {
                                        pair_to_pools_cache
                                            .entry((token_b, token_a))
                                            .or_insert_with(HashSet::new)
                                            .insert(pool_address);
                                    }

                                    // Update dex map
                                    dex_pools_cache
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

        log::info!("🔹 Starting pool discovery task...");
        self.start_pool_discovery_task();

        log::info!("🔹 Starting tick array fetcher/flusher...");
        self.start_tick_array_fetcher_flusher();

        let grpc_service = self.grpc_service.clone();
        let pool_tx = self.get_pool_update_sender();
        let chain_tx = self.get_chain_state_update_sender();

        log::info!("🔹 Spawning gRPC subscription task...");
        tokio::spawn(async move {
            loop {
                log::info!("📡 gRPC subscription task executing...");

                // Clone senders for this attempt (since subscribe takes ownership)
                let pool_tx_clone = pool_tx.clone();
                let chain_tx_clone = chain_tx.clone();

                match grpc_service
                    .subscribe_pool_updates(pool_tx_clone, chain_tx_clone)
                    .await
                {
                    Ok(_) => {
                        log::warn!("⚠️ gRPC subscription ended unexpectedly (connection closed?). Restarting in 5s...");
                    }
                    Err(e) => {
                        log::error!("❌ gRPC subscription failed: {}. Restarting in 5s...", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
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
                            let tick_synced = &tick_synced_pools_c;
                            if tick_synced.contains(&pool_id) {
                                return;
                            }
                        }

                        if let Some(pool_ref) = pools_c.get(&pool_id) {
                            let pool_state = pool_ref.value().clone();

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
                                                    tick_synced_pools_c.insert(pool_id);
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
                                                    tick_synced_pools_c.insert(pool_id);
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
                                                    tick_synced_pools_c.insert(pool_id);
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
                                                    tick_synced_pools_c.insert(pool_id);
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
                                        tick_synced_pools_c.insert(pool_id);
                                    }
                                }
                            }
                        } else {
                            // pool not found, mark as synced to avoid repeated attempts
                            tick_synced_pools_c.insert(pool_id);
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

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(100));

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
        tick_synced_pools: Arc<DashSet<Pubkey>>,
        rpc_client: Arc<RpcClient>,
        sol_price: f64,
        arbitrage_pool_tx: &broadcast::Sender<ArbitragePoolUpdate>,
        arbitrage_monitored_tokens: &HashSet<Pubkey>,
        dbc_configs: Arc<RwLock<HashMap<Pubkey, crate::pool_data_types::dbc::PoolConfig>>>,
    ) {
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
        let pool_exists = pools.contains_key(&pool_address);

        if pool_exists {
            // Update pool in-place via DashMap RefMut
            if let Some(mut pool_ref) = pools.get_mut(&pool_address) {
                pool_with_ticks = update_pool_state_by_event(update, pool_ref.value_mut(), sol_price);
                pool_dex_type = Some(pool_ref.value().dex());
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
            if !tick_synced_pools.contains(&pool_address) {
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
            // Clone needed data from DashMap guard immediately to avoid holding it across .await
            let pool_data = pools.get(&pool_address).map(|pool_ref| {
                let (token_a, token_b) = pool_ref.value().get_tokens();
                let dex = pool_ref.value().dex();
                let calculate_fn_data = pool_ref.value().clone();
                (token_a, token_b, dex, calculate_fn_data)
            });
            // DashMap guard is dropped here

            if let Some((token_a, token_b, dex, pool_state)) = pool_data {
                // Check if this pool involves any monitored tokens
                if arbitrage_monitored_tokens.contains(&token_a)
                    || arbitrage_monitored_tokens.contains(&token_b)
                {
                    // Safe to .await now — no DashMap guards held
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
                        pool_state.calculate_token_prices(sol_price, decimals_a, decimals_b);

                    // Calculate prices in both directions
                    let (forward_price, reverse_price) =
                        if arbitrage_monitored_tokens.contains(&token_a) {
                            (price_b, price_a)
                        } else {
                            (price_b, price_a)
                        };

                    let broadcast_event = ArbitragePoolUpdate {
                        pool_address,
                        token_a,
                        token_b,
                        dex,
                        forward_price,
                        reverse_price,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };

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
        match pools.entry(pool_address) {
            dashmap::mapref::entry::Entry::Vacant(v) => {
                v.insert(pool_state);
            }
            dashmap::mapref::entry::Entry::Occupied(_) => {
                log::warn!(
                    "Pool {:?} was inserted concurrently, skipping insert",
                    pool_address
                );
                return;
            }
        }

        // Update mappings
        pair_to_pools
            .entry((token_a, token_b))
            .or_insert_with(HashSet::new)
            .insert(pool_address);
        if (token_a, token_b) != (token_b, token_a) {
            pair_to_pools
                .entry((token_b, token_a))
                .or_insert_with(HashSet::new)
                .insert(pool_address);
        }

        dex_pools
            .entry(dex)
            .or_insert_with(HashSet::new)
            .insert(pool_address);

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

    pub fn is_pool_tick_synced(&self, pool_address: &Pubkey) -> bool {
        self.tick_synced_pools.contains(pool_address)
    }

    /// Get pool state by address
    pub fn get_pool(&self, pool_address: &Pubkey) -> Option<PoolState> {
        self.pools.get(pool_address).map(|r| r.value().clone())
    }

    /// Get all pools for a token pair (excluding stale pools)
    pub fn get_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolState> {
        // Step 1: Get pool addresses (quick map read)
        let key = if token_a < token_b {
            (*token_a, *token_b)
        } else {
            (*token_b, *token_a)
        };
        let pool_addresses = self
            .pair_to_pools
            .get(&key)
            .map(|r| r.value().clone())
            .unwrap_or_default();

        // Step 2: Read pools directly via DashMap
        let mut results = Vec::new();
        for addr in &pool_addresses {
            if let Some(pool_ref) = self.pools.get(addr) {
                let pool_state = pool_ref.value().clone();
                if !self.is_pool_stale(&pool_state) {
                    results.push(pool_state);
                }
            }
        }
        results
    }

    pub fn get_pool_states_by_addresses(
        &self,
        pool_addresses: &HashSet<Pubkey>,
    ) -> HashMap<Pubkey, PoolState> {
        let mut results = HashMap::new();
        for addr in pool_addresses {
            if let Some(pool_ref) = self.pools.get(addr) {
                let pool_state = pool_ref.value().clone();
                if !self.is_pool_stale(&pool_state) {
                    results.insert(pool_state.address(), pool_state);
                }
            }
        }
        results
    }

    pub fn get_pool_state_by_address(&self, pool_address: &Pubkey) -> Option<PoolState> {
        if let Some(pool_ref) = self.pools.get(pool_address) {
            let pool_state = pool_ref.value().clone();
            if !self.is_pool_stale(&pool_state) {
                Some(pool_state)
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

    pub fn get_pool_addresses_for_pair(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
    ) -> HashSet<Pubkey> {
        let key = if token_a < token_b {
            (*token_a, *token_b)
        } else {
            (*token_b, *token_a)
        };
        self.pair_to_pools
            .get(&key)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    pub fn get_pool_count_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> usize {
        let key = if token_a < token_b {
            (*token_a, *token_b)
        } else {
            (*token_b, *token_a)
        };
        self.pair_to_pools
            .get(&key)
            .map(|r| r.value().len())
            .unwrap_or(0)
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
    pub fn get_pools_for_dex(&self, dex: DexType) -> Vec<PoolState> {
        // Step 1: Get pool addresses for this DEX
        let pool_addresses = self
            .dex_pools
            .get(&dex)
            .map(|r| r.value().clone())
            .unwrap_or_default();

        // Step 2: Read all pools and filter stale ones
        let mut results = Vec::new();
        for addr in &pool_addresses {
            if let Some(pool_ref) = self.pools.get(addr) {
                let pool_state = pool_ref.value().clone();
                if !self.is_pool_stale(&pool_state) {
                    results.push(pool_state);
                }
            }
        }
        results
    }

    /// Remove a pool from the manager
    pub fn remove_pool(&self, pool_address: &Pubkey) {
        self.pools.remove(pool_address);
        // Note: We don't remove from other mappings for performance reasons
    }

    /// Get all cached tokens
    pub async fn get_all_tokens(&self) -> Vec<Token> {
        let token_cache = self.token_cache.read().await;
        token_cache.values().cloned().collect()
    }

    /// Get pool statistics
    pub async fn get_stats(&self) -> PoolManagerStats {
        let token_cache = self.token_cache.read().await;

        PoolManagerStats {
            total_pools: self.pools.len(),
            total_pairs: self.pair_to_pools.len(),
            total_tokens: token_cache.len(),
            pools_by_dex: self.dex_pools
                .iter()
                .map(|entry| (*entry.key(), entry.value().len()))
                .collect(),
        }
    }

    /// Get all pools containing a specific token
    pub fn get_pools_for_token(&self, token_address: &Pubkey) -> Vec<PoolState> {
        let mut results = Vec::new();
        for entry in self.pools.iter() {
            let pool_state = entry.value().clone();
            let (token_a, token_b) = pool_state.get_tokens();
            if (&token_a == token_address || &token_b == token_address)
                && !self.is_pool_stale(&pool_state)
            {
                results.push(pool_state);
            }
        }
        results
    }

    /// Load data from Postgres into in-memory structures
    async fn load_from_db(&mut self) {
        log::info!("Loading pool state from Postgres...");

        // 1. Load Pools
        match self.db.load_pools().await {
            Ok(pools) => {
                for pool_state in pools {
                    self.pools.insert(pool_state.address(), pool_state);
                }
                log::info!("Loaded {} pools from Postgres", self.pools.len());
            }
            Err(e) => {
                log::error!("Failed to load pools from Postgres: {}", e);
            }
        }

        // 2. Load Tokens
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
        let mut pair_to_pools_map: HashMap<(Pubkey, Pubkey), HashSet<Pubkey>> = HashMap::new();
        let mut dex_pools_map: HashMap<DexType, HashSet<Pubkey>> = HashMap::new();
        let mut raydium_clmm_amm_configs_set: HashSet<Pubkey> = HashSet::new();
        let mut raydium_cpmm_amm_configs_set: HashSet<Pubkey> = HashSet::new();
        // Collect pools that need tick/bin array fetching — do NOT .await inside DashMap iter
        let mut pools_needing_tick_fetch: Vec<PoolForTickFetching> = Vec::new();

        log::info!("Rebuilding mappings from {} pools...", self.pools.len());
        let _tick_fetcher = TickArrayFetcher::new(
            self.rpc_client.clone(),
            RaydiumClmmPoolState::get_program_id(),
        );

        for entry in self.pools.iter() {
            let pool_address = entry.key();
            let pool_state = entry.value();

            match pool_state {
                PoolState::Pumpfun(_) => {
                    if !self.config.enable_pumpfun {
                        continue;
                    }
                }
                PoolState::PumpSwap(_) => {
                    if !self.config.enable_pumpfun_swap {
                        continue;
                    }
                }
                PoolState::RaydiumAmmV4(_) => {
                    if !self.config.enable_raydium_amm_v4 {
                        continue;
                    }
                }
                PoolState::RaydiumCpmm(cpmm_pool_state) => {
                    if !self.config.enable_raydium_cpmm {
                        continue;
                    }
                    raydium_cpmm_amm_configs_set.insert(cpmm_pool_state.amm_config);
                }
                PoolState::Bonk(_) => {
                    if !self.config.enable_bonk {
                        continue;
                    }
                }
                PoolState::RadyiumClmm(clmm_pool_state) => {
                    if !self.config.enable_raydium_clmm {
                        continue;
                    }
                    pools_needing_tick_fetch.push(PoolForTickFetching {
                        address: *pool_address,
                        dex_type: DexType::RaydiumClmm,
                    });
                    raydium_clmm_amm_configs_set.insert(clmm_pool_state.amm_config);
                }
                PoolState::MeteoraDbc(_) => {
                    if !self.config.enable_meteora_dbc {
                        continue;
                    }
                }
                PoolState::MeteoraDammV2(_) => {
                    if !self.config.enable_meteora_dammv2 {
                        continue;
                    }
                }
                PoolState::MeteoraDlmm(_) => {
                    if !self.config.enable_meteora_dlmm {
                        continue;
                    }
                    pools_needing_tick_fetch.push(PoolForTickFetching {
                        address: *pool_address,
                        dex_type: DexType::MeteoraDlmm,
                    });
                }
                PoolState::OrcaWhirlpool(_) => {
                    if !self.config.enable_orca_whirlpools {
                        continue;
                    }
                    pools_needing_tick_fetch.push(PoolForTickFetching {
                        address: *pool_address,
                        dex_type: DexType::Orca,
                    });
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
        // DashMap iter guards are now dropped

        // Now safe to .await — insert pending tick fetches
        if !pools_needing_tick_fetch.is_empty() {
            let mut pending = self.pending_pools_to_fetch_tick_arrays.lock().await;
            for pool in pools_needing_tick_fetch {
                pending.insert(pool);
            }
            log::info!("Queued {} pools for tick/bin array fetching", pending.len());
        }

        // Bulk-insert into DashMaps
        self.pair_to_pools.clear();
        for (pair, addrs) in pair_to_pools_map {
            self.pair_to_pools.insert(pair, addrs);
        }
        log::info!("Rebuilt {} pair mappings", self.pair_to_pools.len());

        self.dex_pools.clear();
        for (dex, addrs) in dex_pools_map {
            self.dex_pools.insert(dex, addrs);
        }
        log::info!("Rebuilt {} DEX mappings", self.dex_pools.len());

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
        let pools: Vec<PoolState> = self.pools.iter().map(|entry| entry.value().clone()).collect();

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
        let mut pool_count: usize = 0;

        let pool_entries: Vec<PoolState> = pools.iter().map(|entry| entry.value().clone()).collect();

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
        self.get_pool_addresses_for_pair(token_a, token_b)
    }

    async fn get_pool_state_by_address(&self, pool_address: &Pubkey) -> Option<PoolState> {
        self.get_pool_state_by_address(pool_address)
    }

    async fn is_pool_tick_synced(&self, pool_address: &Pubkey) -> bool {
        self.is_pool_tick_synced(pool_address)
    }

    async fn get_token(&self, token_address: &Pubkey) -> Option<Token> {
        self.get_token(token_address).await
    }

    fn get_sol_price(&self) -> f64 {
        self.get_sol_price()
    }

    async fn get_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolState> {
        self.get_pools_for_pair(token_a, token_b)
    }

    async fn get_pools_for_token(&self, token_address: &Pubkey) -> Vec<PoolState> {
        self.get_pools_for_token(token_address)
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
}
