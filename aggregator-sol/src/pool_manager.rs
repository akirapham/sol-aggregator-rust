use crate::config::ConfigLoader;
use crate::fetchers::fetchers::{fetch_account_data, fetch_token};
use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
use crate::grpc::{BatchProcessor, GrpcService};
use crate::pool_data_types::{
    DexType, GetAmmConfig, PoolState, PoolUpdateEventType, RaydiumClmmAmmConfig,
    RaydiumClmmPoolState, RaydiumClmmPoolUpdate, RaydiumCpmmAmmConfig, WhirlpoolPoolState,
    WhirlpoolPoolUpdate,
};
use crate::types::Token;
use crate::types::{AggregatorConfig, ChainStateUpdate, PoolUpdateEvent};
use crate::utils::{pool_update_event_to_pool_state, update_pool_state_by_event};
use anyhow::Result;
use async_trait::async_trait;
use binance_price_stream::BinancePriceStream;
use bincode::config::Configuration;
use borsh::BorshDeserialize;
use futures::stream::{self, StreamExt};
use rocksdb::{Options, DB};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::common::high_performance_clock::get_high_perf_clock;
use solana_streamer_sdk::streaming::event_parser::core::event_parser::{
    PubkeyData, SimplifiedTokenBalance,
};
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::PoolConfig;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools;
use solana_streamer_sdk::streaming::event_parser::UnifiedEvent;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
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
type PendingPoolsForTickFetching = Arc<Mutex<HashSet<PoolForTickFetching>>>;

/// In-memory pool state manager with real-time updates
pub struct PoolStateManager {
    grpc_service: Arc<GrpcService>,
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
    /// RocksDB instance for persistence
    db: Arc<DB>,
    price_service: Arc<BinancePriceStream>,
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
}

// Serializable wrappers for RocksDB (serialize inner data, not Mutex/Arc)
#[derive(Debug, Serialize, Deserialize)]
struct SerializablePools(HashMap<Pubkey, PoolState>);

#[derive(Debug, Serialize, Deserialize)]
struct SerializableTokenCache(HashMap<Pubkey, Token>);

#[allow(dead_code)]
impl PoolStateManager {
    pub async fn new(
        grpc_service: Arc<GrpcService>,
        price_service: Arc<BinancePriceStream>,
    ) -> Self {
        // Load configuration from environment
        let config = ConfigLoader::load().expect("Failed to load configuration");

        // Initialize RocksDB
        let db_path = "./rocksdb_data"; // Customize path as needed
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = Arc::new(DB::open(&opts, Path::new(db_path)).expect("Failed to open RocksDB"));

        let (pool_update_tx, pool_update_rx) = mpsc::unbounded_channel::<Vec<PoolUpdateEvent>>();
        let (chain_state_update_tx, chain_state_update_rx) =
            mpsc::unbounded_channel::<ChainStateUpdate>();
        let (arbitrage_pool_tx, _arbitrage_pool_rx) =
            broadcast::channel::<ArbitragePoolUpdate>(1000);

        let mut instance = Self {
            grpc_service,
            pools: Arc::new(RwLock::new(HashMap::new())),
            pair_to_pools: Arc::new(RwLock::new(HashMap::new())),
            dex_pools: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            rpc_client: Arc::new(RpcClient::new_with_commitment(
                config.rpc_url.clone(),
                CommitmentConfig::processed(),
            )),
            arbitrage_pool_tx,
            pending_updates: Arc::new(Mutex::new(HashMap::new())),
            pending_updates_account_event: Arc::new(Mutex::new(HashMap::new())),
            db: db.clone(),
            price_service,
            chain_state: Arc::new(Mutex::new(ChainStateUpdate::default())),
            chain_state_update_tx,
            raydium_clmm_amm_config_cache: Arc::new(RwLock::new(HashMap::new())),
            raydium_cpmm_amm_config_cache: Arc::new(RwLock::new(HashMap::new())),
            pending_pools_to_fetch_tick_arrays: Arc::new(Mutex::new(HashSet::new())),
            tick_synced_pools: Arc::new(Mutex::new(HashSet::new())),
            arbitrage_monitored_tokens: Arc::new(RwLock::new(HashSet::new())),
            startup_time: SystemTime::now(),
            config,
        };

        // Load data from RocksDB on startup
        instance.load_from_db().await;

        instance
            .start_pool_update_event_processing(pool_update_rx)
            .await;

        instance
            .start_chain_state_update_event_processing(chain_state_update_rx)
            .await;

        // start periodic flusher that applies coalesced updates
        instance.start_event_update_flusher();

        // Start periodic save to RocksDB (every 15 minutes)
        instance.start_periodic_save_to_db();

        instance.start_tick_array_fetcher_flusher();

        instance
    }

    pub fn get_pool_update_sender(&self) -> mpsc::UnboundedSender<Vec<PoolUpdateEvent>> {
        self.pool_update_tx.clone()
    }

    pub fn get_chain_state_update_sender(&self) -> mpsc::UnboundedSender<ChainStateUpdate> {
        self.chain_state_update_tx.clone()
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
            let db = Arc::clone(&self.db);
            tokio::spawn(async move {
                if let Err(e) = Self::save_arbitrage_tokens_to_db(&db, &tokens) {
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
        let db = Arc::clone(&self.db);
        let tokens = self.get_arbitrage_monitored_tokens().await;
        tokio::spawn(async move {
            if let Err(e) = Self::save_arbitrage_tokens_to_db(&db, &tokens) {
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
        let db = Arc::clone(&self.db);
        let tokens = self.get_arbitrage_monitored_tokens().await;
        tokio::spawn(async move {
            if let Err(e) = Self::save_arbitrage_tokens_to_db(&db, &tokens) {
                log::error!("Failed to save arbitrage tokens to DB: {}", e);
            }
        });

        Ok(())
    }

    /// Save arbitrage monitored token addresses to DB
    fn save_arbitrage_tokens_to_db(db: &Arc<DB>, tokens: &HashSet<Pubkey>) -> Result<(), String> {
        const ARBITRAGE_TOKEN_ADDRS_KEY: &[u8] = b"arbitrage_monitored_token_addresses";

        let addrs: Vec<String> = tokens.iter().map(|p| p.to_string()).collect();
        let json = serde_json::to_string(&addrs)
            .map_err(|e| format!("Failed to serialize token addresses: {}", e))?;

        db.put(ARBITRAGE_TOKEN_ADDRS_KEY, json.as_bytes())
            .map_err(|e| format!("Failed to save token addresses to DB: {}", e))?;

        Ok(())
    }

    /// Load arbitrage monitored token addresses from DB
    pub fn load_arbitrage_tokens_from_db(db: &Arc<DB>) -> Result<HashSet<Pubkey>, String> {
        const ARBITRAGE_TOKEN_ADDRS_KEY: &[u8] = b"arbitrage_monitored_token_addresses";

        match db.get(ARBITRAGE_TOKEN_ADDRS_KEY) {
            Ok(Some(bytes)) => {
                let json = String::from_utf8(bytes.to_vec())
                    .map_err(|e| format!("Invalid UTF-8 in DB: {}", e))?;

                let addrs: Vec<String> = serde_json::from_str(&json)
                    .map_err(|e| format!("Failed to deserialize token addresses: {}", e))?;

                addrs
                    .iter()
                    .map(|s| {
                        Pubkey::from_str(s).map_err(|e| format!("Invalid pubkey in DB: {}", e))
                    })
                    .collect()
            }
            Ok(None) => Ok(HashSet::new()), // No tokens saved yet
            Err(e) => Err(format!("Failed to load token addresses from DB: {}", e)),
        }
    }

    pub async fn start(&self) {
        let grpc_service = self.grpc_service.clone();

        tokio::spawn(async move {
            let _ = grpc_service.start().await;
        });
    }

    pub async fn stop(&self) {
        self.grpc_service.stop().await;
    }

    pub fn start_periodic_save_to_db(&self) {
        let db_clone = Arc::clone(&self.db);
        let pools_clone = Arc::clone(&self.pools);
        let token_cache_clone = Arc::clone(&self.token_cache);

        tokio::spawn(async move {
            let mut save_ticker = interval(Duration::from_secs(6 * 60 * 60)); // 6h
            loop {
                save_ticker.tick().await;
                // measure time to save
                let save_start = std::time::Instant::now();
                if let Err(e) = Self::save_to_db(&db_clone, &pools_clone, &token_cache_clone).await
                {
                    log::error!("Failed to save to RocksDB: {:?}", e);
                } else {
                    let save_ns = save_start.elapsed();
                    log::info!("Saved pool state to RocksDB in {:?}", save_ns);
                }
            }
        });
    }

    // read set of pools with ticks, sync the pool with ticks, mark it as synced with ticks
    pub fn start_tick_array_fetcher_flusher(&self) {
        let pending_pools_to_fetch_tick_arrays =
            Arc::clone(&self.pending_pools_to_fetch_tick_arrays);
        let pools = Arc::clone(&self.pools);
        let rpc_client: Arc<RpcClient> = self.rpc_client.clone();
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
                    async move {
                        let pool_id = pool_for_fetch.address;
                        let dex_type = pool_for_fetch.dex_type;

                        // fetch tick arrays for this pool
                        // check if pool_id already synced
                        {
                            let tick_synced = tick_synced_pools_c.lock().await;
                            if tick_synced.contains(&pool_id) {
                                // already synced, skip
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
                let sol_price = price_service.get_price("SOLUSDT").unwrap_or_default().price;
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
                        continue;
                    }
                    let mut v = Vec::with_capacity(buf.len());
                    for (_k, v_event) in buf.drain() {
                        v.push(v_event);
                    }
                    v
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
    ) {
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
            let (pool_state, is_pool_with_ticks) =
                pool_update_event_to_pool_state(update, sol_price);
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
    /// Returns true if the pool's last update was before the application started
    fn is_pool_stale(&self, pool: &PoolState) -> bool {
        let pool_last_update = SystemTime::UNIX_EPOCH + Duration::from_micros(pool.last_updated());
        pool_last_update < self.startup_time
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
            let key = (*token_a, *token_b);
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

        // Step 3: Read pools concurrently and filter out stale ones
        let mut results = Vec::new();
        for mutex in pool_mutexes {
            let pool_guard = mutex.lock().await; // Only locks this specific pool
            let pool_state = (*pool_guard).clone();
            // Exclude stale pools
            if !self.is_pool_stale(&pool_state) {
                results.push(pool_state);
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

        let mut results = HashMap::new();
        for mutex in pool_mutexes {
            let pool_guard = mutex.lock().await; // Only locks this specific pool
            let pool_cloned = (*pool_guard).clone();
            // Exclude stale pools
            if !self.is_pool_stale(&pool_cloned) {
                results.insert(pool_cloned.address(), pool_cloned);
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

    /// Get access to the underlying RocksDB instance
    pub fn get_db(&self) -> Arc<DB> {
        Arc::clone(&self.db)
    }

    pub async fn get_pool_addresses_for_pair(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
    ) -> HashSet<Pubkey> {
        let pair_to_pools = self.pair_to_pools.read().await;
        let key = (*token_a, *token_b);
        pair_to_pools.get(&key).cloned().unwrap_or_default()
    }

    pub async fn get_pool_count_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> usize {
        let pair_to_pools = self.pair_to_pools.read().await;
        let key = (*token_a, *token_b);
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

    /// Load data from RocksDB into in-memory structures
    async fn load_from_db(&mut self) {
        log::info!("Loading pool state from RocksDB...");
        // Load pools
        if let Ok(Some(data)) = self.db.get(b"pools") {
            if let Ok(serialized) = bincode::serde::decode_from_slice::<
                SerializablePools,
                Configuration,
            >(&data, bincode::config::standard())
            {
                let mut pools_write = self.pools.write().await;
                for (key, state) in serialized.0 .0 {
                    pools_write.insert(key, Arc::new(Mutex::new(state)));
                }
                log::info!("Loaded {} pools from RocksDB", pools_write.len());
            } else {
                log::error!("Failed to deserialize pools from RocksDB");
            }
        }

        // Load token_cache
        if let Ok(Some(data)) = self.db.get(b"token_cache") {
            if let Ok((serialized, _)) = bincode::serde::decode_from_slice::<
                SerializableTokenCache,
                Configuration,
            >(&data, bincode::config::standard())
            {
                let mut token_write = self.token_cache.write().await;
                *token_write = serialized.0;
                log::info!("Loaded {} tokens from RocksDB", token_write.len());
            }
        }

        // Load arbitrage monitored tokens from DB
        match Self::load_arbitrage_tokens_from_db(&self.db) {
            Ok(tokens) => {
                if !tokens.is_empty() {
                    let mut monitored = self.arbitrage_monitored_tokens.write().await;
                    *monitored = tokens.clone();
                    log::info!(
                        "Loaded {} arbitrage monitored tokens from RocksDB",
                        tokens.len()
                    );
                }
            }
            Err(e) => {
                log::warn!("Failed to load arbitrage tokens from DB: {}", e);
            }
        }

        // Rebuild pair_to_pools and dex_pools mappings from loaded pools
        self.rebuild_mappings_from_pools().await;
    }

    /// Rebuild pair_to_pools and dex_pools mappings from existing pools
    async fn rebuild_mappings_from_pools(&self) {
        let pools_read = self.pools.read().await;
        let mut pair_to_pools_map: HashMap<(Pubkey, Pubkey), HashSet<Pubkey>> = HashMap::new();
        let mut dex_pools_map: HashMap<DexType, HashSet<Pubkey>> = HashMap::new();
        let mut raydium_clmm_amm_configs_set: HashSet<Pubkey> = HashSet::new();
        let mut raydium_cpmm_amm_configs_set: HashSet<Pubkey> = HashSet::new();

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

                    // add pool to pending pools to fetch tick arrays
                    let mut pending = self.pending_pools_to_fetch_tick_arrays.lock().await;
                    pending.insert(PoolForTickFetching {
                        address: *pool_address,
                        dex_type: DexType::RaydiumClmm,
                    });
                    drop(pending); // release lock early

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
                PoolState::OrcaWhirlpool(_) => {
                    if !self.config.enable_orca_whirlpools {
                        log::debug!(
                            "Skipping OrcaWhirlpool pool {} - disabled in configuration",
                            pool_address
                        );
                        continue;
                    }

                    // add pool to pending pools to fetch tick arrays
                    let mut pending = self.pending_pools_to_fetch_tick_arrays.lock().await;
                    pending.insert(PoolForTickFetching {
                        address: *pool_address,
                        dex_type: DexType::Orca,
                    });
                    drop(pending); // release lock early
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

        // fetching amm configs from on-chain
        {
            // fetch clmm configs
            for amm_config in raydium_clmm_amm_configs_set.iter() {
                if let Err(e) = self.get_raydium_clmm_amm_config(amm_config).await {
                    log::error!(
                        "Failed to fetch Raydium CLMM AMM config {:?}: {:?}",
                        amm_config,
                        e
                    );
                }
            }

            // fetch cpmm configs
            for amm_config in raydium_cpmm_amm_configs_set.iter() {
                if let Err(e) = self.get_raydium_cpmm_amm_config(amm_config).await {
                    log::error!(
                        "Failed to fetch Raydium CPMM AMM config {:?}: {:?}",
                        amm_config,
                        e
                    );
                }
            }

            log::info!(
                "Loaded {} Raydium CLMM AMM configs from on-chain",
                raydium_clmm_amm_configs_set.len()
            );
            log::info!(
                "Loaded {} Raydium CPMM AMM configs from on-chain",
                raydium_cpmm_amm_configs_set.len()
            );
        }
    }

    /// Save in-memory data to RocksDB
    pub async fn save_pools(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("Saving pools to database...");
        Self::save_to_db(&self.db, &self.pools, &self.token_cache).await
    }

    async fn save_to_db(
        db: &Arc<DB>,
        pools: &PoolStorage,
        token_cache: &Arc<RwLock<HashMap<Pubkey, Token>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Clone the pools Arc for the blocking task
        let pools_clone = Arc::clone(pools);
        // Serialize pools in a blocking task
        let pools_data = tokio::task::spawn_blocking(move || {
            let pools_read = pools_clone.blocking_read();
            let mut pools_data: HashMap<Pubkey, PoolState> = HashMap::new();

            for (k, v) in pools_read.iter() {
                let guard = v.blocking_lock();
                pools_data.insert(*k, (*guard).clone());
            }
            pools_data
        })
        .await?;
        let pool_count = pools_data.len();
        let serialized_pools = bincode::serde::encode_to_vec(
            SerializablePools(pools_data),
            bincode::config::standard(),
        )?;
        db.put(b"pools", serialized_pools)?;
        log::info!("Saved {} pools to RocksDB", pool_count);

        // Serialize token_cache
        let token_read = token_cache.read().await;
        let token_count = token_read.len();
        let serialized_token = bincode::serde::encode_to_vec(
            SerializableTokenCache((*token_read).clone()),
            bincode::config::standard(),
        )?; // Changed
        db.put(b"token_cache", serialized_token)?;
        log::info!("Saved {} tokens to RocksDB", token_count);

        // log saved data size in bytes
        let total_size = db.property_int_value("rocksdb.estimate-live-data-size")?;
        log::info!("RocksDB live data size: {} bytes", total_size.unwrap_or(0));

        Ok(())
    }

    pub fn get_sol_price(&self) -> f64 {
        self.price_service
            .get_price("SOLUSDT")
            .unwrap_or_default()
            .price
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolManagerStats {
    pub total_pools: usize,
    pub total_pairs: usize,
    pub total_tokens: usize,
    pub pools_by_dex: HashMap<DexType, usize>,
}
