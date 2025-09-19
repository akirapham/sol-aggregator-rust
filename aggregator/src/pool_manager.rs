use crate::config::ConfigLoader;
use crate::fetchers::fetchers::fetch_token;
use crate::grpc::{BatchProcessor, GrpcService};
use crate::pool_data_types::{DexType, PoolState};
use crate::types::PoolUpdateEvent;
use crate::types::Token;
use crate::utils::{
    pool_update_event_to_pool_state, update_pool_state_by_event, BinancePriceService,
};
use bincode::config::Configuration;
use futures::stream::{self, StreamExt};
use rocksdb::{Options, DB};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::core::event_parser::{
    PubkeyData, SimplifiedTokenBalance,
};
use solana_streamer_sdk::streaming::event_parser::UnifiedEvent;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::interval;
/// In-memory pool state manager with real-time updates
pub struct PoolStateManager {
    grpc_service: Arc<GrpcService>,
    /// Pool states indexed by pool address
    pools: Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
    /// Pool addresses indexed by token pair
    pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), HashSet<Pubkey>>>>,
    /// DEX-specific pool addresses
    dex_pools: Arc<RwLock<HashMap<DexType, HashSet<Pubkey>>>>,
    /// Token metadata cache
    token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,

    pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
    rpc_client: Arc<RpcClient>,

    /// Coalescing buffer: latest update per pool address
    pending_updates: Arc<Mutex<HashMap<Pubkey, PoolUpdateEvent>>>,
    /// Coalescing buffer: latest update per pool address
    pending_updates_account_event: Arc<Mutex<HashMap<Pubkey, PoolUpdateEvent>>>,
    /// RocksDB instance for persistence
    db: Arc<DB>,
    price_service: Arc<BinancePriceService>,
}

// Serializable wrappers for RocksDB (serialize inner data, not Mutex/Arc)
#[derive(Serialize, Deserialize)]
struct SerializablePools(HashMap<Pubkey, PoolState>);

#[derive(Serialize, Deserialize)]
struct SerializablePairToPools(HashMap<(Pubkey, Pubkey), HashSet<Pubkey>>);

#[derive(Serialize, Deserialize)]
struct SerializableDexPools(HashMap<DexType, HashSet<Pubkey>>);

#[derive(Serialize, Deserialize)]
struct SerializableTokenCache(HashMap<Pubkey, Token>);

impl PoolStateManager {
    pub async fn new(
        grpc_service: Arc<GrpcService>,
        price_service: Arc<BinancePriceService>,
    ) -> Self {
        // Initialize RocksDB
        let db_path = "./rocksdb_data"; // Customize path as needed
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = Arc::new(DB::open(&opts, Path::new(db_path)).expect("Failed to open RocksDB"));

        let (pool_update_tx, pool_update_rx) = mpsc::unbounded_channel::<Vec<PoolUpdateEvent>>();
        let mut instance = Self {
            grpc_service: grpc_service,
            pools: Arc::new(RwLock::new(HashMap::new())),
            pair_to_pools: Arc::new(RwLock::new(HashMap::new())),
            dex_pools: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            rpc_client: Arc::new(RpcClient::new_with_commitment(
                ConfigLoader::load().unwrap().rpc_url.clone(),
                CommitmentConfig::processed(),
            )),
            pending_updates: Arc::new(Mutex::new(HashMap::new())),
            pending_updates_account_event: Arc::new(Mutex::new(HashMap::new())),
            db: db.clone(),
            price_service,
        };

        // Load data from RocksDB on startup
        instance.load_from_db().await;

        instance
            .start_pool_update_event_processing(pool_update_rx)
            .await;

        // start periodic flusher that applies coalesced updates
        {
            let pending = Arc::clone(&instance.pending_updates);
            let pending_account = Arc::clone(&instance.pending_updates_account_event);
            let pools = Arc::clone(&instance.pools);
            let pair_to_pools = Arc::clone(&instance.pair_to_pools);
            let dex_pools = Arc::clone(&instance.dex_pools);
            let token_cache = Arc::clone(&instance.token_cache);
            let rpc_client = instance.rpc_client.clone();
            let price_service = Arc::clone(&instance.price_service);

            tokio::spawn(async move {
                let mut ticker = interval(Duration::from_millis(400));
                loop {
                    ticker.tick().await;

                    // read sol price
                    let sol_price = price_service.get_sol_price().await.unwrap_or_default();

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
                    let drain_ns = drain_start.elapsed();

                    // instrumentation: how many updates we drained
                    let count_account = draineds_account_event.len();
                    let count_normal = draineds.len();
                    let total_count = count_account + count_normal;

                    // bounded concurrency (hybrid)
                    let concurrency_limit = 64usize;

                    // Process account events and normal events in parallel using join!
                    let apply_start = std::time::Instant::now();

                    let (apply_account_result, apply_normal_result) = tokio::join!(
                        // Account events processing
                        async {
                            let start = std::time::Instant::now();
                            stream::iter(draineds_account_event.into_iter().map(|update| {
                                let pools_c = Arc::clone(&pools);
                                let pair_to_pools_c = Arc::clone(&pair_to_pools);
                                let dex_pools_c = Arc::clone(&dex_pools);
                                let token_cache_c = Arc::clone(&token_cache);
                                let rpc_client_c = Arc::clone(&rpc_client);
                                async move {
                                    Self::apply_pool_update(
                                        &update,
                                        pools_c,
                                        pair_to_pools_c,
                                        dex_pools_c,
                                        token_cache_c,
                                        rpc_client_c,
                                        sol_price,
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
                                async move {
                                    Self::apply_pool_update(
                                        &update,
                                        pools_c,
                                        pair_to_pools_c,
                                        dex_pools_c,
                                        token_cache_c,
                                        rpc_client_c,
                                        sol_price,
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

                    let apply_account_ns = apply_account_result;
                    let apply_normal_ns = apply_normal_result;
                    let total_apply_ns = apply_start.elapsed();

                    // log summary / throughput
                    if total_count > 0 {
                        let avg_per_update_ms =
                            (total_apply_ns.as_millis() as f64) / (total_count as f64);
                        log::info!(
                            "Flusher apply (parallel): total_event_count {:?}, handle time {:?}, avg {:.3} ms/update, concurrency={}",
                            total_count,
                            total_apply_ns,
                            avg_per_update_ms,
                            concurrency_limit
                        );
                    } else {
                        log::info!("Flusher apply: nothing to apply");
                    }
                }
            });
        }

        // Start periodic save to RocksDB (every 15 minutes)
        {
            let db_clone = Arc::clone(&db);
            let pools_clone = Arc::clone(&instance.pools);
            let token_cache_clone = Arc::clone(&instance.token_cache);

            tokio::spawn(async move {
                let mut save_ticker = interval(Duration::from_secs(15 * 60)); // 15 minutes
                loop {
                    save_ticker.tick().await;
                    // measure time to save
                    let save_start = std::time::Instant::now();
                    if let Err(e) =
                        Self::save_to_db(&db_clone, &pools_clone, &token_cache_clone).await
                    {
                        log::error!("Failed to save to RocksDB: {:?}", e);
                    } else {
                        let save_ns = save_start.elapsed();
                        log::info!("Saved pool state to RocksDB in {:?}", save_ns);
                    }
                }
            });
        }

        instance
    }

    pub fn get_pool_update_sender(&self) -> mpsc::UnboundedSender<Vec<PoolUpdateEvent>> {
        self.pool_update_tx.clone()
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

    pub fn start_batch_event_processing(
        mut batch_rx: mpsc::UnboundedReceiver<
            Vec<(
                Vec<Box<dyn UnifiedEvent>>,
                Vec<PubkeyData>,
                Vec<u64>,
                HashMap<String, SimplifiedTokenBalance>,
            )>,
        >,
        pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
    ) {
        // run in its own task
        tokio::spawn(async move {
            log::info!("Starting batch event processing loop...");

            while let Some(batch) = batch_rx.recv().await {
                log::debug!("Received batch of {} events for processing", batch.len());

                // Process the batch using the existing method
                BatchProcessor::process_batch(batch, pool_update_tx.clone()).await;
            }

            log::info!("Batch event processing loop ended - no more batches to process");
        });
    }

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
                            buf.insert(update.address(), update.clone());
                        }
                    }
                }

                {
                    let mut buf = pending_account.lock().await;
                    for update in updates.iter() {
                        // use the event address as key; clone the event for the buffer
                        if update.is_account_state_update() {
                            buf.insert(update.address(), update.clone());
                        }
                    }
                }
            }

            log::info!("Pool update processing loop ended");
        });
    }

    async fn apply_pool_update(
        update: &PoolUpdateEvent,
        pools: Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
        pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), HashSet<Pubkey>>>>,
        dex_pools: Arc<RwLock<HashMap<DexType, HashSet<Pubkey>>>>,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        rpc_client: Arc<RpcClient>,
        sol_price: f64,
    ) {
        let pool_address = update.address();
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
                update_pool_state_by_event(update, &mut pool_guard, sol_price);
            }
        } else {
            // Insert new pool
            let pool_state = pool_update_event_to_pool_state(update, sol_price);
            if let Some(pool_state) = pool_state {
                let (token0, token1) = pool_state.get_tokens();
                if token0 == Pubkey::default() || token1 == Pubkey::default() {
                    return;
                }
                Self::insert_new_pool(
                    pool_state,
                    pools,
                    pair_to_pools,
                    dex_pools,
                    token_cache,
                    rpc_client,
                )
                .await;
            }
        }
    }

    async fn insert_new_pool(
        pool_state: PoolState,
        pools: Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
        pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), HashSet<Pubkey>>>>,
        dex_pools: Arc<RwLock<HashMap<DexType, HashSet<Pubkey>>>>,
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

    /// Get all pools for a token pair
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

        // Step 3: Read pools concurrently (no map lock held)
        let mut results = Vec::new();
        for mutex in pool_mutexes {
            let pool_guard = mutex.lock().await; // Only locks this specific pool
            results.push((*pool_guard).clone());
        }
        results
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

    /// Get pools for a specific DEX
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

        // Step 3: Read all pools concurrently
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
                results.push(pool);
            }
        }
        results
    }

    /// Get best pools for a token pair sorted by liquidity
    // pub async fn get_best_pools_for_pair(
    //     &self,
    //     token_a: &Pubkey,
    //     token_b: &Pubkey,
    //     limit: usize,
    // ) -> Vec<PoolState> {
    //     let mut pools = self.get_pools_for_pair(token_a, token_b).await;

    //     // Sort by liquidity (reserve_a + reserve_b as proxy)
    //     pools.sort_by(|a, b| {
    //         let liquidity_a = a.reserve_a + a.reserve_b;
    //         let liquidity_b = b.reserve_a + b.reserve_b;
    //         liquidity_b.cmp(&liquidity_a)
    //     });

    //     pools.into_iter().take(limit).collect()
    // }

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

        // Rebuild pair_to_pools and dex_pools mappings from loaded pools
        self.rebuild_mappings_from_pools().await;
    }

    /// Rebuild pair_to_pools and dex_pools mappings from existing pools
    async fn rebuild_mappings_from_pools(&self) {
        let pools_read = self.pools.read().await;
        let mut pair_to_pools_map: HashMap<(Pubkey, Pubkey), HashSet<Pubkey>> = HashMap::new();
        let mut dex_pools_map: HashMap<DexType, HashSet<Pubkey>> = HashMap::new();

        log::info!("Rebuilding mappings from {} pools...", pools_read.len());

        for (pool_address, pool_mutex) in pools_read.iter() {
            // Get pool state (we know these exist since we just loaded them)
            let pool_guard = pool_mutex.lock().await; // Safe since we're loading on startup
            let pool_state = &*pool_guard;

            let (token_a, token_b) = pool_state.get_tokens();
            let dex_type = pool_state.dex();

            // Skip pools with invalid tokens
            if token_a == Pubkey::default() || token_b == Pubkey::default() {
                continue;
            }

            // Add to pair_to_pools mapping (both directions)
            pair_to_pools_map
                .entry((token_a, token_b))
                .or_insert_with(HashSet::new)
                .insert(*pool_address);

            if (token_a, token_b) != (token_b, token_a) {
                pair_to_pools_map
                    .entry((token_b, token_a))
                    .or_insert_with(HashSet::new)
                    .insert(*pool_address);
            }

            // Add to dex_pools mapping
            dex_pools_map
                .entry(dex_type)
                .or_insert_with(HashSet::new)
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
    }

    /// Save in-memory data to RocksDB
    async fn save_to_db(
        db: &Arc<DB>,
        pools: &Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
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
            &SerializablePools(pools_data),
            bincode::config::standard(),
        )?;
        db.put(b"pools", serialized_pools)?;
        log::info!("Saved {} pools to RocksDB", pool_count);

        // Serialize token_cache
        let token_read = token_cache.read().await;
        let token_count = token_read.len();
        let serialized_token = bincode::serde::encode_to_vec(
            &SerializableTokenCache((*token_read).clone()),
            bincode::config::standard(),
        )?; // Changed
        db.put(b"token_cache", serialized_token)?;
        log::info!("Saved {} tokens to RocksDB", token_count);

        Ok(())
    }

    pub async fn get_sol_price(&self) -> f64 {
        self.price_service.get_sol_price().await.unwrap_or_default()
    }

    // Clean up old or inactive pools
    // pub async fn cleanup_stale_pools(&self, max_age_seconds: u64) {
    //     let current_time = std::time::SystemTime::now()
    //         .duration_since(std::time::UNIX_EPOCH)
    //         .unwrap()
    //         .as_secs();

    //     let mut pools = self.pools.write().await;
    //     pools.retain(|_, pool| current_time - pool.last_updated() < max_age_seconds);
    // }
}

#[derive(Debug, Clone)]
pub struct PoolManagerStats {
    pub total_pools: usize,
    pub total_pairs: usize,
    pub total_tokens: usize,
    pub pools_by_dex: HashMap<DexType, usize>,
}
