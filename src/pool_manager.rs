use solana_client::nonblocking::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::UnifiedEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::config::ConfigLoader;
use crate::error::{DexAggregatorError, Result};
use crate::fetchers::fetchers::fetch_token;
use crate::grpc::{create_grpc_service, BatchProcessor, GrpcService};
use crate::types::{DexType, Token};
use crate::utils::pool_update_event_to_pool_state;
use crate::{AggregatorConfig, PoolState, PoolUpdateEvent};

/// In-memory pool state manager with real-time updates
pub struct PoolStateManager {
    grpc_service: Arc<GrpcService>,
    /// Pool states indexed by pool address
    pools: Arc<RwLock<HashMap<Pubkey, PoolState>>>,
    /// Pool addresses indexed by token pair
    pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), Vec<Pubkey>>>>,
    /// DEX-specific pool addresses
    dex_pools: Arc<RwLock<HashMap<DexType, Vec<Pubkey>>>>,
    /// Token metadata cache
    token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,

    pool_update_tx: mpsc::UnboundedSender<PoolUpdateEvent>,
    pool_update_rx: mpsc::UnboundedReceiver<PoolUpdateEvent>,
    rpc_client: Arc<RpcClient>,
}

impl PoolStateManager {
    pub async fn new(grpc_service: Arc<GrpcService>) -> Self {
        let (pool_update_tx, pool_update_rx) = mpsc::unbounded_channel::<PoolUpdateEvent>();
        Self {
            grpc_service: grpc_service,
            pools: Arc::new(RwLock::new(HashMap::new())),
            pair_to_pools: Arc::new(RwLock::new(HashMap::new())),
            dex_pools: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_update_tx,
            pool_update_rx,
            rpc_client: Arc::new(RpcClient::new_with_commitment(ConfigLoader::load().unwrap().rpc_url.clone(), CommitmentConfig::processed())),
        }
    }

    pub fn get_pool_update_sender(&self) -> mpsc::UnboundedSender<PoolUpdateEvent> {
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
        mut batch_rx: mpsc::UnboundedReceiver<Vec<Box<dyn UnifiedEvent>>>,
        pool_update_tx: mpsc::UnboundedSender<PoolUpdateEvent>,
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
        while let Some(update) = self.pool_update_rx.recv().await {
            // Process pool updates concurrently
            let pools_clone = Arc::clone(&pools);
            let pair_to_pools_clone = Arc::clone(&pair_to_pools);
            let dex_pools_clone = Arc::clone(&dex_pools);
            let token_cache_clone = Arc::clone(&token_cache);
            let rpc_client_clone = rpc_client.clone();
            tokio::spawn(async move {
                Self::apply_pool_update(
                    &update,
                    pools_clone,
                    pair_to_pools_clone,
                    dex_pools_clone,
                    token_cache_clone,
                    rpc_client_clone,
                )
                .await;
            });
        }

        log::info!("Pool update processing loop ended");
    }

    async fn apply_pool_update(
        update: &PoolUpdateEvent,
        pools: Arc<RwLock<HashMap<Pubkey, PoolState>>>,
        pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), Vec<Pubkey>>>>,
        dex_pools: Arc<RwLock<HashMap<DexType, Vec<Pubkey>>>>,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        rpc_client: Arc<RpcClient>,
    ) {
        let pool_state = pool_update_event_to_pool_state(update);
        // check if pool exists
        let pool_exists = {
            let pools_read = pools.read().await;
            pools_read.contains_key(&pool_state.address)
        };
        if pool_exists {
            // Update existing pool
            let mut pools_write = pools.write().await;
            if let Some(pool) = pools_write.get_mut(&pool_state.address) {
                pool.reserve_a = pool_state.reserve_a;
                pool.reserve_b = pool_state.reserve_b;
                pool.last_updated = pool_state.last_updated;
                pool.liquidity = pool_state.liquidity;
                pool.liquidity_usd = pool_state.liquidity_usd;
                pool.sqrt_price = pool_state.sqrt_price;
                pool.tick_current = pool_state.tick_current;
                pool.amp_factor = pool_state.amp_factor;
                pool.tick_spacing = pool_state.tick_spacing;
            }
        } else {
            // Insert new pool
            Self::insert_new_pool(pool_state, pools, pair_to_pools, dex_pools, token_cache, rpc_client).await;
        }
    }

    async fn insert_new_pool(
        pool_state: PoolState,
        pools: Arc<RwLock<HashMap<Pubkey, PoolState>>>,
        pair_to_pools: Arc<RwLock<HashMap<(Pubkey, Pubkey), Vec<Pubkey>>>>,
        dex_pools: Arc<RwLock<HashMap<DexType, Vec<Pubkey>>>>,
        token_cache: Arc<RwLock<HashMap<Pubkey, Token>>>,
        rpc_client: Arc<RpcClient>,
    ) {
        let pool_address = pool_state.address;
        let dex = pool_state.dex;
        let token_a = pool_state.token_a;
        let token_b = pool_state.token_b;

        // Insert pool
        {
            let mut pools_write = pools.write().await;
            pools_write.insert(pool_address, pool_state.clone());
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
        pools.get(pool_address).cloned()
    }

    /// Get all pools for a token pair
    pub async fn get_pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolState> {
        let pair_to_pools = self.pair_to_pools.read().await;
        let pools = self.pools.read().await;

        let key = (*token_a, *token_b);
        if let Some(pool_addresses) = pair_to_pools.get(&key) {
            pool_addresses
                .iter()
                .filter_map(|addr| pools.get(addr).cloned())
                .collect()
        } else {
            Vec::new()
        }
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
        let dex_pools = self.dex_pools.read().await;
        let pools = self.pools.read().await;

        if let Some(pool_addresses) = dex_pools.get(&dex) {
            pool_addresses
                .iter()
                .filter_map(|addr| pools.get(addr).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get best pools for a token pair sorted by liquidity
    pub async fn get_best_pools_for_pair(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
        limit: usize,
    ) -> Vec<PoolState> {
        let mut pools = self.get_pools_for_pair(token_a, token_b).await;

        // Sort by liquidity (reserve_a + reserve_b as proxy)
        pools.sort_by(|a, b| {
            let liquidity_a = a.reserve_a + a.reserve_b;
            let liquidity_b = b.reserve_a + b.reserve_b;
            liquidity_b.cmp(&liquidity_a)
        });

        pools.into_iter().take(limit).collect()
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

    /// Clean up old or inactive pools
    pub async fn cleanup_stale_pools(&self, max_age_seconds: u64) {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut pools = self.pools.write().await;
        pools.retain(|_, pool| current_time - pool.last_updated < max_age_seconds);
    }
}

#[derive(Debug, Clone)]
pub struct PoolManagerStats {
    pub total_pools: usize,
    pub total_pairs: usize,
    pub total_tokens: usize,
    pub pools_by_dex: HashMap<DexType, usize>,
}
