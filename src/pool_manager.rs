use solana_client::nonblocking::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::core::event_parser::{
    PubkeyData, SimplifiedTokenBalance,
};
use solana_streamer_sdk::streaming::event_parser::UnifiedEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::ConfigLoader;
use crate::fetchers::fetchers::fetch_token;
use crate::grpc::{BatchProcessor, GrpcService};
use crate::pool_data_types::{DexType, PoolState};
use crate::types::Token;
use crate::utils::pool_update_event_to_pool_state;
use crate::PoolUpdateEvent;

/// In-memory pool state manager with real-time updates
pub struct PoolStateManager {
    grpc_service: Arc<GrpcService>,
    /// Pool states indexed by pool address
    pools: Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
    /// Pool addresses indexed by token pair
    pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), Vec<Pubkey>>>>,
    /// DEX-specific pool addresses
    dex_pools: Arc<RwLock<HashMap<DexType, Vec<Pubkey>>>>,
    /// Token metadata cache
    token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,

    pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
    pool_update_rx: mpsc::UnboundedReceiver<Vec<PoolUpdateEvent>>,
    rpc_client: Arc<RpcClient>,
}

impl PoolStateManager {
    pub async fn new(grpc_service: Arc<GrpcService>) -> Self {
        let (pool_update_tx, pool_update_rx) = mpsc::unbounded_channel::<Vec<PoolUpdateEvent>>();
        Self {
            grpc_service: grpc_service,
            pools: Arc::new(RwLock::new(HashMap::new())),
            pair_to_pools: Arc::new(RwLock::new(HashMap::new())),
            dex_pools: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            pool_update_rx,
            rpc_client: Arc::new(RpcClient::new_with_commitment(
                ConfigLoader::load().unwrap().rpc_url.clone(),
                CommitmentConfig::processed(),
            )),
        }
    }

    pub fn get_pool_update_sender(&self) -> mpsc::UnboundedSender<Vec<PoolUpdateEvent>> {
        self.pool_update_tx.clone()
    }

    pub async fn start(&mut self) {
        let grpc_service = self.grpc_service.clone();

        tokio::spawn(async move {
            let _ = grpc_service.start().await;
        });

        log::info!("Starting event handling loop...");

        self.start_pool_update_event_processing().await;
        log::info!("Event handling loop ended - no more batches to process");
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

    async fn start_pool_update_event_processing(&mut self) {
        log::info!("Starting pool update event processing loop...");

        let pools = Arc::clone(&self.pools);
        let pair_to_pools = Arc::clone(&self.pair_to_pools);
        let dex_pools = Arc::clone(&self.dex_pools);
        let token_cache = Arc::clone(&self.token_cache);
        let rpc_client = self.rpc_client.clone();
        while let Some(updates) = self.pool_update_rx.recv().await {
            // Process pool updates concurrently
            let pools_clone = Arc::clone(&pools);
            let pair_to_pools_clone = Arc::clone(&pair_to_pools);
            let dex_pools_clone = Arc::clone(&dex_pools);
            let token_cache_clone = Arc::clone(&token_cache);
            let rpc_client_clone = rpc_client.clone();
            tokio::spawn(async move {
                for update in updates.iter() {
                    Self::apply_pool_update(
                        update,
                        Arc::clone(&pools_clone),
                        Arc::clone(&pair_to_pools_clone),
                        Arc::clone(&dex_pools_clone),
                        Arc::clone(&token_cache_clone),
                        Arc::clone(&rpc_client_clone),
                    )
                    .await;
                }
            });
        }

        log::info!("Pool update processing loop ended");
    }

    async fn apply_pool_update(
        update: &PoolUpdateEvent,
        pools: Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
        pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), Vec<Pubkey>>>>,
        dex_pools: Arc<RwLock<HashMap<DexType, Vec<Pubkey>>>>,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        rpc_client: Arc<RpcClient>,
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
                let pool_state = pool_update_event_to_pool_state(update, Some(pool_guard.clone()));
                // if pool_guard.dex() == DexType::Raydium {
                log::info!(
                    "Updating existing pool: {}, dex {}, reserves: {:?}",
                    pool_address,
                    pool_guard.dex(),
                    pool_state.get_reserves()
                );
                // }
                *pool_guard = pool_state;
            }
        } else {
            // Insert new pool
            Self::insert_new_pool(
                pool_update_event_to_pool_state(update, None),
                pools,
                pair_to_pools,
                dex_pools,
                token_cache,
                rpc_client,
            )
            .await;
        }
    }

    async fn insert_new_pool(
        pool_state: PoolState,
        pools: Arc<RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>>,
        pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), Vec<Pubkey>>>>,
        dex_pools: Arc<RwLock<HashMap<DexType, Vec<Pubkey>>>>,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        rpc_client: Arc<RpcClient>,
    ) {
        let pool_address = pool_state.address();
        let dex = pool_state.dex();
        let (token_a, token_b) = pool_state.get_tokens();

        // Insert pool
        {
            let mut pools_write = pools.write().await;
            pools_write.insert(pool_address, Arc::new(Mutex::new(pool_state.clone())));
        }

        // Update mappings
        {
            let mut pair_to_pools_write = pair_to_pools.write().await;
            pair_to_pools_write
                .entry((token_a, token_b))
                .or_insert_with(Vec::new)
                .push(pool_address);
            if (token_a, token_b) != (token_b, token_a) {
                pair_to_pools_write
                    .entry((token_b, token_a))
                    .or_insert_with(Vec::new)
                    .push(pool_address);
            }
        }

        {
            let mut dex_pools_write = dex_pools.write().await;
            dex_pools_write
                .entry(dex)
                .or_insert_with(Vec::new)
                .push(pool_address);
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

// impl TokenProviderInterface for PoolStateManager {
//     async fn get_token_info(&self, mint: &Pubkey) -> Result<Option<Token>> {
//         let token = self.get_token(mint).await;
//         Ok(token)
//     }
// }

#[derive(Debug, Clone)]
pub struct PoolManagerStats {
    pub total_pools: usize,
    pub total_pairs: usize,
    pub total_tokens: usize,
    pub pools_by_dex: HashMap<DexType, usize>,
}
