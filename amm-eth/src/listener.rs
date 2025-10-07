use crate::price_store::PriceStore;
use crate::types::{DexVersion, EthConfig, TokenPrice};
use anyhow::{Context, Result};
use dashmap::DashMap;
use ethers::abi::RawLog;
use ethers::prelude::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// Uniswap V2 Pair ABI - Sync event (emitted after every swap with updated reserves)
abigen!(
    UniswapV2Pair,
    r#"[
        event Sync(uint112 reserve0, uint112 reserve1)
        function token0() external view returns (address)
        function token1() external view returns (address)
    ]"#
);

// Uniswap V3 Pool ABI - Swap event with sqrtPriceX96
abigen!(
    UniswapV3Pool,
    r#"[
        event Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
        function token0() external view returns (address)
        function token1() external view returns (address)
    ]"#
);

// Multicall3 contract for batching calls
abigen!(
    Multicall3,
    r#"[
        struct Call { address target; bytes callData; }
        struct Result { bool success; bytes returnData; }
        function aggregate(Call[] calldata calls) external returns (uint256 blockNumber, bytes[] memory returnData)
    ]"#
);

// Token pair cache entry
#[derive(Debug, Clone)]
struct TokenPair {
    token0: Address,
    token1: Address,
}

/// Ethereum WebSocket client for listening to Uniswap swap events
pub struct EthSwapListener {
    config: EthConfig,
    price_store: PriceStore,
    provider: Arc<Provider<Ws>>,
    // Cache for token pairs: pool_address -> (token0, token1)
    token_pair_cache: Arc<DashMap<Address, TokenPair>>,
}

impl EthSwapListener {
    /// Create a new Ethereum swap listener
    pub async fn new(config: EthConfig, price_store: PriceStore) -> Result<Self> {
        info!("Connecting to Ethereum WebSocket: {}", config.websocket_url);

        let ws = Ws::connect(&config.websocket_url)
            .await
            .context("Failed to connect to Ethereum WebSocket")?;

        let provider = Arc::new(Provider::new(ws));

        info!("Connected to Ethereum network");

        Ok(Self {
            config,
            price_store,
            provider,
            token_pair_cache: Arc::new(DashMap::new()),
        })
    }

    /// Start listening to swap events
    pub async fn start(&self) -> Result<()> {
        info!("Starting Ethereum swap event listener");

        // Subscribe to Uniswap V2 Sync events
        let v2_handle = {
            let provider = self.provider.clone();
            let price_store = self.price_store.clone();
            let config = self.config.clone();
            let token_cache = self.token_pair_cache.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::listen_v2_sync(provider, price_store, config, token_cache).await {
                    error!("Uniswap V2 listener error: {}", e);
                }
            })
        };

        // Subscribe to Uniswap V3 swap events
        let v3_handle = {
            let provider = self.provider.clone();
            let price_store = self.price_store.clone();
            let config = self.config.clone();
            let token_cache = self.token_pair_cache.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::listen_v3_swaps(provider, price_store, config, token_cache).await {
                    error!("Uniswap V3 listener error: {}", e);
                }
            })
        };

        // Wait for both listeners
        let _ = tokio::try_join!(v2_handle, v3_handle)?;

        Ok(())
    }

    /// Listen to Uniswap V2 Sync events (emitted after swaps with updated reserves)
    async fn listen_v2_sync(
        provider: Arc<Provider<Ws>>,
        price_store: PriceStore,
        config: EthConfig,
        token_cache: Arc<DashMap<Address, TokenPair>>,
    ) -> Result<()> {
        info!("Starting Uniswap V2 Sync event listener");

        // Create a filter for V2 Sync events
        let sync_filter = Filter::new()
            .event("Sync(uint112,uint112)");

        let mut stream = provider
            .subscribe_logs(&sync_filter)
            .await
            .context("Failed to subscribe to V2 Sync events")?;

        info!("Subscribed to Uniswap V2 Sync events");

        while let Some(log) = stream.next().await {
            if let Err(e) = Self::process_v2_sync_log(log, &provider, &price_store, &config, &token_cache).await {
                error!("Error processing V2 sync: {}", e);
            }
        }

        warn!("Uniswap V2 sync stream ended");
        Ok(())
    }

    /// Process a Uniswap V2 Sync event (has updated reserves)
    async fn process_v2_sync_log(
        log: Log,
        provider: &Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        token_cache: &Arc<DashMap<Address, TokenPair>>,
    ) -> Result<()> {
        let pool_address = log.address;

        // Try to parse as V2 Sync event
        let sync_event: uniswap_v2_pair::SyncFilter = match <uniswap_v2_pair::SyncFilter as ethers::contract::EthEvent>::decode_log(&RawLog {
            topics: log.topics.clone(),
            data: log.data.to_vec(),
        }) {
            Ok(event) => event,
            Err(_) => return Ok(()), // Skip if not a sync event
        };

        log::info!(
            "Processing V2 Sync event for pool: {:?}",
            pool_address
        );

        // Get token addresses from cache or fetch via contract
        let token_pair = Self::get_or_fetch_token_pair(pool_address, provider, token_cache).await?;

        let reserve0 = sync_event.reserve_0;
        let reserve1 = sync_event.reserve_1;

        // Skip if reserves are zero
        if reserve0 == 0 || reserve1 == 0 {
            return Ok(());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        debug!(
            "V2 Sync: Pool {:?} - Reserve0: {}, Reserve1: {}",
            pool_address, reserve0, reserve1
        );

        // Calculate price of token0 in terms of token1
        let price_token0_in_token1 = (reserve1 as f64) / (reserve0 as f64);

        // Calculate price of token1 in terms of token0
        let price_token1_in_token0 = (reserve0 as f64) / (reserve1 as f64);

        // Update prices for both tokens
        Self::update_token_price(
            token_pair.token0,
            token_pair.token1,
            price_token0_in_token1,
            pool_address,
            timestamp,
            DexVersion::UniswapV2,
            price_store,
            config,
        );

        Self::update_token_price(
            token_pair.token1,
            token_pair.token0,
            price_token1_in_token0,
            pool_address,
            timestamp,
            DexVersion::UniswapV2,
            price_store,
            config,
        );

        Ok(())
    }

    /// Listen to Uniswap V3 swap events
    async fn listen_v3_swaps(
        provider: Arc<Provider<Ws>>,
        price_store: PriceStore,
        config: EthConfig,
        token_cache: Arc<DashMap<Address, TokenPair>>,
    ) -> Result<()> {
        info!("Starting Uniswap V3 swap listener");

        // Create a filter for V3 Swap events
        let swap_filter = Filter::new()
            .event("Swap(address,address,int256,int256,uint160,uint128,int24)");

        let mut stream = provider
            .subscribe_logs(&swap_filter)
            .await
            .context("Failed to subscribe to V3 swap events")?;

        info!("Subscribed to Uniswap V3 swap events");

        while let Some(log) = stream.next().await {
            if let Err(e) = Self::process_v3_swap_log(log, &provider, &price_store, &config, &token_cache).await {
                error!("Error processing V3 swap: {}", e);
            }
        }

        warn!("Uniswap V3 swap stream ended");
        Ok(())
    }

    /// Process a Uniswap V3 swap log (use sqrtPriceX96 to compute price)
    async fn process_v3_swap_log(
        log: Log,
        provider: &Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        token_cache: &Arc<DashMap<Address, TokenPair>>,
    ) -> Result<()> {
        let pool_address = log.address;

        // Try to parse as V3 Swap event
        let swap_event: uniswap_v3_pool::SwapFilter = match <uniswap_v3_pool::SwapFilter as ethers::contract::EthEvent>::decode_log(&RawLog {
            topics: log.topics.clone(),
            data: log.data.to_vec(),
        }) {
            Ok(event) => event,
            Err(_) => return Ok(()), // Skip if not a swap event
        };

        // Get token addresses from cache or fetch via contract
        let token_pair = Self::get_or_fetch_token_pair(pool_address, provider, token_cache).await?;

        // Extract sqrtPriceX96 from the event
        let sqrt_price_x96 = swap_event.sqrt_price_x96;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        debug!(
            "V3 Swap: Pool {:?} - sqrtPriceX96: {}",
            pool_address, sqrt_price_x96
        );

        // Calculate price from sqrtPriceX96
        // price = (sqrtPriceX96 / 2^96)^2
        // Convert U256 to f64 for calculation
        let sqrt_price = sqrt_price_x96.as_u128() as f64 / (1u128 << 96) as f64;
        let price_token0_in_token1 = sqrt_price * sqrt_price;
        let price_token1_in_token0 = if price_token0_in_token1 > 0.0 {
            1.0 / price_token0_in_token1
        } else {
            0.0
        };

        // Update prices for both tokens
        Self::update_token_price(
            token_pair.token0,
            token_pair.token1,
            price_token0_in_token1,
            pool_address,
            timestamp,
            DexVersion::UniswapV3,
            price_store,
            config,
        );

        Self::update_token_price(
            token_pair.token1,
            token_pair.token0,
            price_token1_in_token0,
            pool_address,
            timestamp,
            DexVersion::UniswapV3,
            price_store,
            config,
        );

        Ok(())
    }

    /// Get or fetch token pair from cache
    async fn get_or_fetch_token_pair(
        pool_address: Address,
        provider: &Arc<Provider<Ws>>,
        token_cache: &Arc<DashMap<Address, TokenPair>>,
    ) -> Result<TokenPair> {
        // Check cache first
        if let Some(pair) = token_cache.get(&pool_address) {
            return Ok(pair.clone());
        }

        // Not in cache, fetch from contract
        let pair_contract = UniswapV2Pair::new(pool_address, provider.clone());

        // Fetch token addresses sequentially to avoid lifetime issues
        let token0 = pair_contract.token_0().call().await?;
        let token1 = pair_contract.token_1().call().await?;

        let token_pair = TokenPair { token0, token1 };

        // Store in cache
        token_cache.insert(pool_address, token_pair.clone());

        debug!(
            "Cached token pair for pool {:?}: token0={:?}, token1={:?}",
            pool_address, token0, token1
        );

        Ok(token_pair)
    }

    /// Update token price in the price store
    fn update_token_price(
        token_address: Address,
        paired_with: Address,
        price_in_paired_token: f64,
        pool_address: Address,
        timestamp: u64,
        dex_version: DexVersion,
        price_store: &PriceStore,
        config: &EthConfig,
    ) {
        let mut price_in_eth = 0.0;
        let mut price_in_usd = None;

        // If paired with WETH, this IS the ETH price
        if paired_with == config.weth_address {
            price_in_eth = price_in_paired_token;

            // Calculate USD price if ETH/USD rate is available
            if let Ok(eth_price) = config.eth_price_usd.read() {
                if let Some(eth_usd) = *eth_price {
                    price_in_usd = Some(price_in_eth * eth_usd);
                }
            }
        }

        let token_price = TokenPrice {
            token_address,
            price_in_eth,
            price_in_usd,
            last_updated: timestamp,
            pool_address,
            dex_version,
        };

        price_store.update_price(token_address, token_price);
    }

    /// Get the current price store
    pub fn get_price_store(&self) -> &PriceStore {
        &self.price_store
    }
}
