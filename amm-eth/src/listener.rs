use crate::price_store::PriceStore;
use crate::types::EthConfig;
use anyhow::{Context, Result};
use dashmap::DashMap;
use eth_dex_quote::{DexVersion, TokenPrice};
use ethers::abi::RawLog;
use ethers::prelude::*;
use log::{debug, error, info, warn};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

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

// Uniswap V4 PoolManager ABI - Swap event with PoolId and Initialize event
// V4 uses a singleton PoolManager contract for all pools
abigen!(
    UniswapV4PoolManager,
    r#"[
        event Swap(bytes32 indexed id, address indexed sender, int128 amount0, int128 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick, uint24 fee)
        event Initialize(bytes32 indexed id, address indexed currency0, address indexed currency1, uint24 fee, int24 tickSpacing, address hooks)
        function getPool(bytes32 id) external view returns (tuple(uint160 sqrtPriceX96, int24 tick, uint24 protocolFee, uint24 lpFee))
    ]"#
);

// Uniswap V4 StateView contract for querying pool information
// This contract provides helper functions to extract pool details from pool IDs
abigen!(
    UniswapV4StateView,
    r#"[
        function getPoolKey(bytes32 id) external pure returns (tuple(address currency0, address currency1, uint24 fee, int24 tickSpacing, address hooks))
        function getPoolState(bytes32 id) external view returns (tuple(uint160 sqrtPriceX96, int24 tick, uint24 protocolFee, uint24 lpFee))
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

// ERC20 Token ABI for decimals
abigen!(
    ERC20,
    r#"[
        function decimals() external view returns (uint8)
        function symbol() external view returns (string)
        function name() external view returns (string)
    ]"#
);

/// Ethereum WebSocket client for listening to Uniswap swap events
pub struct EthSwapListener {
    config: EthConfig,
    price_store: PriceStore,
    // Cache for token pairs: pool_address -> (token0, token1, decimals0, decimals1)
    token_pair_cache: Arc<DashMap<Address, (Address, Address, u8, u8)>>,
    // Cache for V4 pool IDs: pool_id (bytes32) -> (token0, token1, decimals0, decimals1, pool_address)
    v4_pool_id_cache: Arc<DashMap<[u8; 32], (Address, Address, u8, u8, Address)>>,
    // Cache for token decimals: token_address -> decimals
    token_decimal_cache: Arc<DashMap<Address, u8>>,
    // Semaphore to limit concurrent event processing
    processing_semaphore: Arc<Semaphore>,
}

impl EthSwapListener {
    /// Create a new Ethereum swap listener
    pub async fn new(config: EthConfig, price_store: PriceStore) -> Result<Self> {
        info!("Initializing Ethereum swap listener");

        // Limit concurrent event processing to 50 tasks
        let processing_semaphore = Arc::new(Semaphore::new(50));
        info!("Event processing concurrency limit: 50");

        Ok(Self {
            config,
            price_store,
            token_pair_cache: Arc::new(DashMap::new()),
            v4_pool_id_cache: Arc::new(DashMap::new()),
            token_decimal_cache: Arc::new(DashMap::new()),
            processing_semaphore,
        })
    }

    /// Get reference to the token pair cache for persistence
    pub fn get_token_pair_cache(&self) -> Arc<DashMap<Address, (Address, Address, u8, u8)>> {
        self.token_pair_cache.clone()
    }

    /// Get reference to the token decimal cache for persistence
    pub fn get_token_decimal_cache(&self) -> Arc<DashMap<Address, u8>> {
        self.token_decimal_cache.clone()
    }

    /// Start listening to swap events
    pub async fn start(&self) -> Result<()> {
        info!("Starting Ethereum swap event listener (V2, V3, V4)");

        // Subscribe to Uniswap V2 Sync events
        let v2_handle = {
            let websocket_url = self.config.websocket_url.clone();
            let price_store = self.price_store.clone();
            let config = self.config.clone();
            let token_cache = self.token_pair_cache.clone();
            let semaphore = self.processing_semaphore.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    Self::listen_v2_sync(websocket_url, price_store, config, token_cache, semaphore)
                        .await
                {
                    error!("Uniswap V2 listener error: {}", e);
                }
            })
        };

        // Subscribe to Uniswap V3 swap events
        let v3_handle = {
            let websocket_url = self.config.websocket_url.clone();
            let price_store = self.price_store.clone();
            let config = self.config.clone();
            let token_cache = self.token_pair_cache.clone();
            let semaphore = self.processing_semaphore.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::listen_v3_swaps(
                    websocket_url,
                    price_store,
                    config,
                    token_cache,
                    semaphore,
                )
                .await
                {
                    error!("Uniswap V3 listener error: {}", e);
                }
            })
        };

        // Subscribe to Uniswap V4 swap events
        let v4_handle = {
            let websocket_url = self.config.websocket_url.clone();
            let price_store = self.price_store.clone();
            let config = self.config.clone();
            let v4_cache = self.v4_pool_id_cache.clone();
            let semaphore = self.processing_semaphore.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    Self::listen_v4_swaps(websocket_url, price_store, config, v4_cache, semaphore)
                        .await
                {
                    error!("Uniswap V4 listener error: {}", e);
                }
            })
        };

        // Wait for all three listeners
        let _ = tokio::try_join!(v2_handle, v3_handle, v4_handle)?;

        Ok(())
    }

    /// Listen to Uniswap V2 Sync events with auto-reconnection and ping
    async fn listen_v2_sync(
        websocket_url: String,
        price_store: PriceStore,
        config: EthConfig,
        token_cache: Arc<DashMap<Address, (Address, Address, u8, u8)>>,
        semaphore: Arc<Semaphore>,
    ) -> Result<()> {
        info!("Starting Uniswap V2 Sync event listener with auto-reconnection");

        let mut reconnect_delay = Duration::from_secs(5);
        let max_reconnect_delay = Duration::from_secs(300); // 5 minutes max

        loop {
            info!(
                "Connecting to Ethereum WebSocket for V2 events: {} (reconnect delay: {:?})",
                websocket_url, reconnect_delay
            );

            match Ws::connect(&websocket_url).await {
                Ok(ws) => {
                    let provider = Arc::new(Provider::new(ws));
                    info!("Connected to Ethereum network for V2 events");

                    match Self::listen_v2_sync_with_provider(
                        provider,
                        &price_store,
                        &config,
                        &token_cache,
                        &semaphore,
                    )
                    .await
                    {
                        Ok(_) => {
                            info!("V2 sync listener completed normally");
                            break;
                        }
                        Err(e) => {
                            error!("V2 sync listener error: {}", e);
                            // Reset reconnect delay on successful connection but error
                            reconnect_delay = Duration::from_secs(5);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect to Ethereum WebSocket for V2: {}", e);
                }
            }

            // Exponential backoff for reconnection
            warn!("Reconnecting V2 sync listener in {:?}...", reconnect_delay);
            tokio::time::sleep(reconnect_delay).await;

            // Increase delay for next attempt (exponential backoff)
            reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
        }

        Ok(())
    }

    /// Listen to V2 events with a connected provider
    async fn listen_v2_sync_with_provider(
        provider: Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        token_cache: &Arc<DashMap<Address, (Address, Address, u8, u8)>>,
        semaphore: &Arc<Semaphore>,
    ) -> Result<()> {
        // Create a filter for V2 Sync events
        let sync_filter = Filter::new().event("Sync(uint112,uint112)");

        let mut stream = provider
            .subscribe_logs(&sync_filter)
            .await
            .context("Failed to subscribe to V2 Sync events")?;

        info!("Subscribed to Uniswap V2 Sync events");

        // Start ping task to keep connection alive
        let ping_provider = provider.clone();
        let ping_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            let mut consecutive_failures = 0;
            const MAX_CONSECUTIVE_FAILURES: u32 = 3;

            loop {
                interval.tick().await;
                // Send a simple ping by requesting the latest block number
                // This keeps the connection alive and detects if it's broken
                match ping_provider.get_block_number().await {
                    Ok(block_num) => {
                        consecutive_failures = 0;
                        debug!("V2 WebSocket ping successful, block: {}", block_num);
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        error!(
                            "V2 Ping failed ({} consecutive), connection may be broken: {}",
                            consecutive_failures, e
                        );

                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            error!(
                                "V2 Too many consecutive ping failures, triggering reconnection"
                            );
                            break;
                        }
                    }
                }
            }
        });

        let result = async {
            let mut event_count = 0;
            let mut last_event_time = std::time::Instant::now();
            let mut timeout_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                tokio::select! {
                    // Handle incoming events
                    log = stream.next() => {
                        match log {
                            Some(log) => {
                                event_count += 1;
                                last_event_time = std::time::Instant::now();

                                // Acquire semaphore permit before spawning task
                                let permit = semaphore.clone().acquire_owned().await.unwrap();

                                // Spawn processing task to not block the stream
                                let provider_clone = provider.clone();
                                let price_store_clone = price_store.clone();
                                let config_clone = config.clone();
                                let token_cache_clone = token_cache.clone();

                                tokio::spawn(async move {
                                    // Permit will be dropped when task completes
                                    let _permit = permit;

                                    if let Err(e) = Self::process_v2_sync_log(
                                        log,
                                        &provider_clone,
                                        &price_store_clone,
                                        &config_clone,
                                        &token_cache_clone,
                                    )
                                    .await
                                    {
                                        error!("Error processing V2 sync: {}", e);
                                    }
                                });
                            }
                            None => {
                                // Stream ended
                                warn!("V2 Event stream ended (received None)");
                                break;
                            }
                        }
                    }

                    // Periodic timeout check
                    _ = timeout_check_interval.tick() => {
                        // Check if ping task has failed (connection issues)
                        if ping_handle.is_finished() {
                            match ping_handle.await {
                                Ok(_) => {
                                    warn!("V2 Ping task completed normally");
                                    return Err(anyhow::anyhow!("V2 Ping task ended unexpectedly"));
                                }
                                Err(e) => {
                                    error!("V2 Ping task panicked: {}", e);
                                    return Err(anyhow::anyhow!("V2 Ping task failed: {}", e));
                                }
                            }
                        }

                        // Check for event timeout (no events for 13 seconds - Ethereum block time + buffer)
                        if last_event_time.elapsed() > std::time::Duration::from_secs(20) && event_count > 0 {
                            warn!("V2 No events received for 20 seconds, connection may be stale");
                            return Err(anyhow::anyhow!("V2 Event timeout - no events received for 20 seconds"));
                        }
                    }
                }
            }

            Ok(())
        }
        .await;

        result
    }

    /// Process a Uniswap V2 Sync event (has updated reserves)
    async fn process_v2_sync_log(
        log: Log,
        _provider: &Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        token_cache: &Arc<DashMap<Address, (Address, Address, u8, u8)>>,
    ) -> Result<()> {
        let pool_address = log.address;

        // Try to parse as V2 Sync event
        let sync_event: uniswap_v2_pair::SyncFilter =
            match <uniswap_v2_pair::SyncFilter as ethers::contract::EthEvent>::decode_log(&RawLog {
                topics: log.topics.clone(),
                data: log.data.to_vec(),
            }) {
                Ok(event) => event,
                Err(_) => return Ok(()), // Skip if not a sync event
            };

        // Get token addresses and decimals from cache or fetch via contract
        let (token0, token1, decimals0, decimals1) =
            match Self::get_or_fetch_token_pair(pool_address, token_cache, config).await {
                Ok(pair) => pair,
                Err(e) => {
                    error!(
                        "Failed to get token pair for pool {:?}: {}",
                        pool_address, e
                    );
                    return Ok(());
                }
            };

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

        // Calculate price of token0 in terms of token1 (accounting for decimals)
        // Use U256 for precision, then convert to f64 at the end
        // price = (reserve1 * 10^decimals0) / (reserve0 * 10^decimals1)
        let reserve0_u256 = U256::from(reserve0);
        let reserve1_u256 = U256::from(reserve1);

        // Calculate with decimal adjustment using U256 for precision
        let decimals_diff_0_to_1 = decimals0 as i32 - decimals1 as i32;
        let price_token0_in_token1 = if decimals_diff_0_to_1 >= 0 {
            // Multiply reserve1 by 10^decimals_diff
            let multiplier = U256::from(10u128).pow(U256::from(decimals_diff_0_to_1 as u32));
            let numerator = reserve1_u256.saturating_mul(multiplier);
            Self::u256_to_f64_ratio(numerator, reserve0_u256)
        } else {
            // Multiply reserve0 by 10^(-decimals_diff)
            let multiplier = U256::from(10u128).pow(U256::from((-decimals_diff_0_to_1) as u32));
            let denominator = reserve0_u256.saturating_mul(multiplier);
            Self::u256_to_f64_ratio(reserve1_u256, denominator)
        };

        // Calculate price of token1 in terms of token0 (accounting for decimals)
        let decimals_diff_1_to_0 = decimals1 as i32 - decimals0 as i32;
        let price_token1_in_token0 = if decimals_diff_1_to_0 >= 0 {
            let multiplier = U256::from(10u128).pow(U256::from(decimals_diff_1_to_0 as u32));
            let numerator = reserve0_u256.saturating_mul(multiplier);
            Self::u256_to_f64_ratio(numerator, reserve1_u256)
        } else {
            let multiplier = U256::from(10u128).pow(U256::from((-decimals_diff_1_to_0) as u32));
            let denominator = reserve1_u256.saturating_mul(multiplier);
            Self::u256_to_f64_ratio(reserve0_u256, denominator)
        };

        // Update prices for both tokens
        Self::update_token_price(
            token0,
            token1,
            price_token0_in_token1,
            pool_address,
            timestamp,
            DexVersion::UniswapV2,
            decimals0,
            price_store,
            config,
        );

        Self::update_token_price(
            token1,
            token0,
            price_token1_in_token0,
            pool_address,
            timestamp,
            DexVersion::UniswapV2,
            decimals1,
            price_store,
            config,
        );

        Ok(())
    }

    /// Listen to Uniswap V3 swap events with auto-reconnection and ping
    async fn listen_v3_swaps(
        websocket_url: String,
        price_store: PriceStore,
        config: EthConfig,
        token_cache: Arc<DashMap<Address, (Address, Address, u8, u8)>>,
        semaphore: Arc<Semaphore>,
    ) -> Result<()> {
        info!("Starting Uniswap V3 swap listener with auto-reconnection");

        let mut reconnect_delay = Duration::from_secs(5);
        let max_reconnect_delay = Duration::from_secs(300); // 5 minutes max

        loop {
            info!(
                "Connecting to Ethereum WebSocket for V3 events: {} (reconnect delay: {:?})",
                websocket_url, reconnect_delay
            );

            match Ws::connect(&websocket_url).await {
                Ok(ws) => {
                    let provider = Arc::new(Provider::new(ws));
                    info!("Connected to Ethereum network for V3 events");

                    match Self::listen_v3_swaps_with_provider(
                        provider,
                        &price_store,
                        &config,
                        &token_cache,
                        &semaphore,
                    )
                    .await
                    {
                        Ok(_) => {
                            info!("V3 swap listener completed normally");
                            break;
                        }
                        Err(e) => {
                            error!("V3 swap listener error: {}", e);
                            // Reset reconnect delay on successful connection but error
                            reconnect_delay = Duration::from_secs(5);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect to Ethereum WebSocket for V3: {}", e);
                }
            }

            // Exponential backoff for reconnection
            warn!("Reconnecting V3 swap listener in {:?}...", reconnect_delay);
            tokio::time::sleep(reconnect_delay).await;

            // Increase delay for next attempt (exponential backoff)
            reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
        }

        Ok(())
    }

    /// Listen to V3 events with a connected provider
    async fn listen_v3_swaps_with_provider(
        provider: Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        token_cache: &Arc<DashMap<Address, (Address, Address, u8, u8)>>,
        semaphore: &Arc<Semaphore>,
    ) -> Result<()> {
        // Create a filter for V3 Swap events
        let swap_filter =
            Filter::new().event("Swap(address,address,int256,int256,uint160,uint128,int24)");

        let mut stream = provider
            .subscribe_logs(&swap_filter)
            .await
            .context("Failed to subscribe to V3 swap events")?;

        info!("Subscribed to Uniswap V3 swap events");

        // Start ping task to keep connection alive
        let ping_provider = provider.clone();
        let ping_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            let mut consecutive_failures = 0;
            const MAX_CONSECUTIVE_FAILURES: u32 = 3;

            loop {
                interval.tick().await;
                // Send a simple ping by requesting the latest block number
                // This keeps the connection alive and detects if it's broken
                match ping_provider.get_block_number().await {
                    Ok(block_num) => {
                        consecutive_failures = 0;
                        debug!("V3 WebSocket ping successful, block: {}", block_num);
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        error!(
                            "V3 Ping failed ({} consecutive), connection may be broken: {}",
                            consecutive_failures, e
                        );

                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            error!(
                                "V3 Too many consecutive ping failures, triggering reconnection"
                            );
                            break;
                        }
                    }
                }
            }
        });

        let result = async {
            let mut event_count = 0;
            let mut last_event_time = std::time::Instant::now();
            let mut timeout_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                tokio::select! {
                    // Handle incoming events
                    log = stream.next() => {
                        match log {
                            Some(log) => {
                                event_count += 1;
                                last_event_time = std::time::Instant::now();

                                // Acquire semaphore permit before spawning task
                                let permit = semaphore.clone().acquire_owned().await.unwrap();

                                // Spawn processing task to not block the stream
                                let provider_clone = provider.clone();
                                let price_store_clone = price_store.clone();
                                let config_clone = config.clone();
                                let token_cache_clone = token_cache.clone();

                                tokio::spawn(async move {
                                    // Permit will be dropped when task completes
                                    let _permit = permit;

                                    if let Err(e) = Self::process_v3_swap_log(
                                        log,
                                        &provider_clone,
                                        &price_store_clone,
                                        &config_clone,
                                        &token_cache_clone,
                                    )
                                    .await
                                    {
                                        error!("Error processing V3 swap: {}", e);
                                    }
                                });
                            }
                            None => {
                                // Stream ended
                                warn!("V3 Event stream ended (received None)");
                                break;
                            }
                        }
                    }

                    // Periodic timeout check
                    _ = timeout_check_interval.tick() => {
                        // Check if ping task has failed (connection issues)
                        if ping_handle.is_finished() {
                            match ping_handle.await {
                                Ok(_) => {
                                    warn!("V3 Ping task completed normally");
                                    return Err(anyhow::anyhow!("V3 Ping task ended unexpectedly"));
                                }
                                Err(e) => {
                                    error!("V3 Ping task panicked: {}", e);
                                    return Err(anyhow::anyhow!("V3 Ping task failed: {}", e));
                                }
                            }
                        }

                        // Check for event timeout (no events for 13 seconds - Ethereum block time + buffer)
                        if last_event_time.elapsed() > std::time::Duration::from_secs(20) && event_count > 0 {
                            warn!("V3 No events received for 20 seconds, connection may be stale");
                            return Err(anyhow::anyhow!("V3 Event timeout - no events received for 20 seconds"));
                        }
                    }
                }
            }

            Ok(())
        }
        .await;

        result
    }

    /// Process a Uniswap V3 swap log (use sqrtPriceX96 to compute price)
    async fn process_v3_swap_log(
        log: Log,
        _provider: &Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        token_cache: &Arc<DashMap<Address, (Address, Address, u8, u8)>>,
    ) -> Result<()> {
        let pool_address = log.address;

        // Try to parse as V3 Swap event
        let swap_event: uniswap_v3_pool::SwapFilter =
            match <uniswap_v3_pool::SwapFilter as ethers::contract::EthEvent>::decode_log(&RawLog {
                topics: log.topics.clone(),
                data: log.data.to_vec(),
            }) {
                Ok(event) => event,
                Err(_) => return Ok(()), // Skip if not a swap event
            };

        // Get token addresses and decimals from cache or fetch via contract
        let (token0, token1, decimals0, decimals1) =
            match Self::get_or_fetch_token_pair(pool_address, token_cache, config).await {
                Ok(pair) => pair,
                Err(e) => {
                    error!(
                        "Failed to get token pair for pool {:?}: {}",
                        pool_address, e
                    );
                    return Ok(());
                }
            };

        // Extract sqrtPriceX96 from the event
        let sqrt_price_x96 = swap_event.sqrt_price_x96;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Calculate price from sqrtPriceX96 using U256 for precision
        // price = (sqrtPriceX96 / 2^96)^2 * (10^decimals0 / 10^decimals1)
        // Rearranged: price = (sqrtPriceX96^2 * 10^decimals0) / (2^192 * 10^decimals1)
        let price_token0_in_token1 =
            Self::sqrt_price_x96_to_price(sqrt_price_x96, decimals0, decimals1);

        let price_token1_in_token0 = if price_token0_in_token1 > 0.0 {
            1.0 / price_token0_in_token1
        } else {
            0.0
        };
        Self::update_token_price(
            token0,
            token1,
            price_token0_in_token1,
            pool_address,
            timestamp,
            DexVersion::UniswapV3,
            decimals0,
            price_store,
            config,
        );

        Self::update_token_price(
            token1,
            token0,
            price_token1_in_token0,
            pool_address,
            timestamp,
            DexVersion::UniswapV3,
            decimals1,
            price_store,
            config,
        );

        Ok(())
    }

    fn sqrt_price_x96_to_price(sqrt_price_x96: U256, decimals0: u8, decimals1: u8) -> f64 {
        // price = (sqrtPriceX96 / 2^96)^2 * (10^decimals0 / 10^decimals1)
        // Rearranged: price = (sqrtPriceX96^2 * 10^decimals0) / (2^192 * 10^decimals1)

        let sqrt_price_squared = sqrt_price_x96.saturating_mul(sqrt_price_x96);
        let q192 = U256::from(1u128) << 192;

        let decimals_diff = decimals0 as i32 - decimals1 as i32;
        let ret = if decimals_diff >= 0 {
            let multiplier = U256::from(10u128).pow(U256::from(decimals_diff as u32));
            let numerator = sqrt_price_squared.saturating_mul(multiplier);
            let temp = Self::u256_to_f64_ratio(numerator, q192);

            temp
        } else {
            let multiplier = U256::from(10u128).pow(U256::from((-decimals_diff) as u32));
            let denominator = q192.saturating_mul(multiplier);
            Self::u256_to_f64_ratio(sqrt_price_squared, denominator)
        };

        ret
    }

    /// Get or fetch token pair from cache (with decimals)
    async fn get_or_fetch_token_pair(
        pool_address: Address,
        token_cache: &Arc<DashMap<Address, (Address, Address, u8, u8)>>,
        config: &EthConfig,
    ) -> Result<(Address, Address, u8, u8)> {
        // Check cache first
        if let Some(pair) = token_cache.get(&pool_address) {
            return Ok(*pair.value());
        }

        // Create RPC provider from environment variable
        let rpc_url =
            std::env::var("ETH_RPC_URL").context("ETH_RPC_URL environment variable not set")?;
        let provider = Arc::new(
            Provider::<Http>::try_from(rpc_url)
                .context("Failed to create HTTP provider from ETH_RPC_URL")?,
        );

        // Not in cache, fetch from contract with retry logic (3 attempts)
        let mut last_error = None;

        for attempt in 1..=3 {
            match Self::fetch_token_pair_with_decimals(pool_address, &provider, config).await {
                Ok((token0, token1, decimals0, decimals1)) => {
                    // Store in cache
                    token_cache.insert(pool_address, (token0, token1, decimals0, decimals1));

                    debug!(
                        "Cached token pair for pool {:?}: token0={:?}, token1={:?}, decimals0={}, decimals1={}",
                        pool_address, token0, token1, decimals0, decimals1
                    );

                    return Ok((token0, token1, decimals0, decimals1));
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch token pair for pool {:?} (attempt {}/3): {}",
                        pool_address, attempt, e
                    );
                    last_error = Some(e);

                    // Wait a bit before retrying (exponential backoff)
                    if attempt < 3 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            100 * attempt as u64,
                        ))
                        .await;
                    }
                }
            }
        }

        // All attempts failed
        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Failed to fetch token pair after 3 attempts")))
    }

    /// Fetch token pair and decimals in parallel
    async fn fetch_token_pair_with_decimals(
        pool_address: Address,
        provider: &Arc<Provider<Http>>,
        config: &EthConfig,
    ) -> Result<(Address, Address, u8, u8)> {
        let pair_contract = UniswapV2Pair::new(pool_address, provider.clone());

        // Fetch token addresses in parallel
        let token0_call = pair_contract.token_0();
        let token1_call = pair_contract.token_1();
        let token0_fut = token0_call.call();
        let token1_fut = token1_call.call();

        let (token0, token1) = tokio::try_join!(token0_fut, token1_fut)
            .context("Failed to fetch token addresses from pool")?;

        // Check if tokens have known decimals (WETH, USDC, USDT) to avoid RPC calls
        let decimals0 = if let Some(known_decimals) = config.get_known_decimals(token0) {
            debug!(
                "Using known decimals for token {:?}: {}",
                token0, known_decimals
            );
            known_decimals
        } else {
            ERC20::new(token0, provider.clone())
                .decimals()
                .call()
                .await
                .unwrap_or(18)
        };

        let decimals1 = if let Some(known_decimals) = config.get_known_decimals(token1) {
            debug!(
                "Using known decimals for token {:?}: {}",
                token1, known_decimals
            );
            known_decimals
        } else {
            ERC20::new(token1, provider.clone())
                .decimals()
                .call()
                .await
                .unwrap_or(18)
        };

        Ok((token0, token1, decimals0, decimals1))
    }

    /// Update token price in the price store
    fn update_token_price(
        token_address: Address,
        paired_with: Address,
        price_in_paired_token: f64,
        pool_address: Address,
        timestamp: u64,
        dex_version: DexVersion,
        decimals: u8,
        price_store: &PriceStore,
        config: &EthConfig,
    ) {
        // we do nothing if token is WETH, USDC or USDT
        if token_address == config.weth_address
            || token_address == config.usdc_address
            || token_address == config.usdt_address
            || token_address == config.native_address
        {
            return;
        }

        let mut price_in_eth = 0.0;
        let mut price_in_usd = None;

        // Determine price based on what token it's paired with
        if paired_with == config.weth_address || paired_with == config.native_address {
            // Paired with WETH: we have the ETH price directly
            price_in_eth = price_in_paired_token;

            // Calculate USD price if ETH/USD rate is available
            if let Ok(eth_price) = config.eth_price_usd.read() {
                if let Some(eth_usd) = *eth_price {
                    price_in_usd = Some(price_in_eth * eth_usd);
                }
            }
        } else if paired_with == config.usdc_address || paired_with == config.usdt_address {
            // Paired with USDC or USDT: we have the USD price directly
            price_in_usd = Some(price_in_paired_token);

            // Calculate ETH price if ETH/USD rate is available
            if let Ok(eth_price) = config.eth_price_usd.read() {
                if let Some(eth_usd) = *eth_price {
                    if eth_usd > 0.0 {
                        price_in_eth = price_in_paired_token / eth_usd;
                    }
                }
            }
        }

        let (token0, token1) = if token_address < paired_with {
            (token_address, paired_with)
        } else {
            (paired_with, token_address)
        };

        let token_price = TokenPrice {
            token_address,
            price_in_eth,
            price_in_usd,
            last_updated: timestamp,
            pool_address,
            dex_version,
            decimals,
            pool_token0: token0,
            pool_token1: token1,
            eth_chain: config.eth_chain,
        };

        price_store.update_price(token_address, token_price);
    }

    /// Get the current price store
    pub fn get_price_store(&self) -> &PriceStore {
        &self.price_store
    }

    /// Convert U256 ratio to f64 with precision handling
    /// Uses BigUint for arbitrary precision arithmetic
    fn u256_to_f64_ratio(numerator: U256, denominator: U256) -> f64 {
        if denominator.is_zero() {
            return 0.0;
        }

        // Convert U256 to BigUint for arbitrary precision
        let num_bytes = {
            let mut bytes = [0u8; 32];
            numerator.to_big_endian(&mut bytes);
            bytes
        };
        let den_bytes = {
            let mut bytes = [0u8; 32];
            denominator.to_big_endian(&mut bytes);
            bytes
        };

        let num_big = BigUint::from_bytes_be(&num_bytes);
        let den_big = BigUint::from_bytes_be(&den_bytes);

        // Scale by 10^18 for precision
        let scale = BigUint::from(10u128).pow(18);
        let scaled_num = num_big * scale;

        // Perform division
        let result = scaled_num / den_big;

        // Convert to f64
        // For very large numbers, we need to be careful
        if let Some(result_f64) = result.to_f64() {
            result_f64 / 1e18
        } else {
            // Number is too large for f64, take a reasonable approximation
            // Get the most significant digits
            let result_str = result.to_string();
            if result_str.len() > 18 {
                // Take first ~15 significant digits (f64 precision limit)
                let significant_digits = &result_str[0..15];
                let exponent = result_str.len() - 15;
                if let Ok(mantissa) = significant_digits.parse::<f64>() {
                    mantissa * 10f64.powi(exponent as i32) / 1e18
                } else {
                    0.0
                }
            } else {
                // Should fit in f64
                result_str.parse::<f64>().unwrap_or(0.0) / 1e18
            }
        }
    }

    /// Listen to Uniswap V4 swap events with auto-reconnection and ping
    /// V4 uses a singleton PoolManager contract (0x000000000004444c5dc75cB358380D2e3dE08A90)
    async fn listen_v4_swaps(
        websocket_url: String,
        price_store: PriceStore,
        config: EthConfig,
        v4_cache: Arc<DashMap<[u8; 32], (Address, Address, u8, u8, Address)>>,
        semaphore: Arc<Semaphore>,
    ) -> Result<()> {
        info!("Starting Uniswap V4 swap listener with auto-reconnection");

        let mut reconnect_delay = Duration::from_secs(5);
        let max_reconnect_delay = Duration::from_secs(300); // 5 minutes max

        loop {
            info!(
                "Connecting to Ethereum WebSocket for V4 events: {} (reconnect delay: {:?})",
                websocket_url, reconnect_delay
            );

            match Ws::connect(&websocket_url).await {
                Ok(ws) => {
                    let provider = Arc::new(Provider::new(ws));
                    info!("Connected to Ethereum network for V4 events");

                    match Self::listen_v4_swaps_with_provider(
                        provider,
                        &price_store,
                        &config,
                        &v4_cache,
                        &semaphore,
                    )
                    .await
                    {
                        Ok(_) => {
                            info!("V4 swap listener completed normally");
                            break;
                        }
                        Err(e) => {
                            error!("V4 swap listener error: {}", e);
                            // Reset reconnect delay on successful connection but error
                            reconnect_delay = Duration::from_secs(5);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect to Ethereum WebSocket for V4: {}", e);
                }
            }

            // Exponential backoff for reconnection
            warn!("Reconnecting V4 swap listener in {:?}...", reconnect_delay);
            tokio::time::sleep(reconnect_delay).await;

            // Increase delay for next attempt (exponential backoff)
            reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
        }

        Ok(())
    }

    /// Listen to V4 events with a connected provider
    async fn listen_v4_swaps_with_provider(
        provider: Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        v4_cache: &Arc<DashMap<[u8; 32], (Address, Address, u8, u8, Address)>>,
        semaphore: &Arc<Semaphore>,
    ) -> Result<()> {
        // V4 PoolManager address on Ethereum mainnet
        let v4_pool_manager: Address = "0x000000000004444c5dc75cB358380D2e3dE08A90"
            .parse()
            .context("Invalid V4 PoolManager address")?;

        info!(
            "🚀 Setting up V4 listener for PoolManager: {:?}",
            v4_pool_manager
        );

        // Create a filter for V4 Swap events from the PoolManager contract
        // event Swap(bytes32 indexed id, address indexed sender, int128 amount0, int128 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick, uint24 fee)
        let swap_filter = Filter::new()
            .address(v4_pool_manager)
            .event("Swap(bytes32,address,int128,int128,uint160,uint128,int24,uint24)");

        let mut stream = provider
            .subscribe_logs(&swap_filter)
            .await
            .context("Failed to subscribe to V4 swap events")?;

        info!("✅ Subscribed to Uniswap V4 swap events from PoolManager - waiting for events...");

        // Start ping task to keep connection alive
        let ping_provider = provider.clone();
        let ping_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            let mut consecutive_failures = 0;
            const MAX_CONSECUTIVE_FAILURES: u32 = 3;

            loop {
                interval.tick().await;
                // Send a simple ping by requesting the latest block number
                match ping_provider.get_block_number().await {
                    Ok(block_num) => {
                        consecutive_failures = 0;
                        debug!("V4 WebSocket ping successful, block: {}", block_num);
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        error!(
                            "V4 Ping failed ({} consecutive), connection may be broken: {}",
                            consecutive_failures, e
                        );

                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            error!(
                                "V4 Too many consecutive ping failures, triggering reconnection"
                            );
                            break;
                        }
                    }
                }
            }
        });

        let result = async {
            let mut event_count = 0;
            let mut last_event_time = std::time::Instant::now();
            let mut timeout_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                tokio::select! {
                    // Handle incoming events
                    log = stream.next() => {
                        match log {
                            Some(log) => {
                                event_count += 1;
                                last_event_time = std::time::Instant::now();

                                // Acquire semaphore permit before spawning task
                                let permit = semaphore.clone().acquire_owned().await.unwrap();

                                // Spawn processing task to not block the stream
                                let provider_clone = provider.clone();
                                let price_store_clone = price_store.clone();
                                let config_clone = config.clone();
                                let v4_cache_clone = v4_cache.clone();

                                tokio::spawn(async move {
                                    // Permit will be dropped when task completes
                                    let _permit = permit;

                                    if let Err(e) = Self::process_v4_swap_log(
                                        log,
                                        &provider_clone,
                                        &price_store_clone,
                                        &config_clone,
                                        &v4_cache_clone,
                                    )
                                    .await
                                    {
                                        error!("❌ Error processing V4 swap: {}", e);
                                    }
                                });
                            }
                            None => {
                                // Stream ended
                                warn!("V4 Event stream ended (received None)");
                                break;
                            }
                        }
                    }

                    // Periodic timeout check
                    _ = timeout_check_interval.tick() => {
                        // Check if ping task has failed (connection issues)
                        if ping_handle.is_finished() {
                            match ping_handle.await {
                                Ok(_) => {
                                    warn!("V4 Ping task completed normally");
                                    return Err(anyhow::anyhow!("V4 Ping task ended unexpectedly"));
                                }
                                Err(e) => {
                                    error!("V4 Ping task panicked: {}", e);
                                    return Err(anyhow::anyhow!("V4 Ping task failed: {}", e));
                                }
                            }
                        }

                        // Check for event timeout (no events for 13 seconds - Ethereum block time + buffer)
                        if last_event_time.elapsed() > std::time::Duration::from_secs(20) && event_count > 0 {
                            warn!("V4 No events received for 20 seconds, connection may be stale");
                            return Err(anyhow::anyhow!("V4 Event timeout - no events received for 20 seconds"));
                        }
                    }
                }
            }

            Ok(())
        }
        .await;

        result
    }

    /// Process a Uniswap V4 swap log
    /// V4 emits: Swap(bytes32 indexed id, address indexed sender, int128 amount0, int128 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick, uint24 fee)
    async fn process_v4_swap_log(
        log: Log,
        _provider: &Arc<Provider<Ws>>,
        price_store: &PriceStore,
        config: &EthConfig,
        v4_cache: &Arc<DashMap<[u8; 32], (Address, Address, u8, u8, Address)>>,
    ) -> Result<()> {
        // Try to parse as V4 Swap event
        let swap_event: uniswap_v4_pool_manager::SwapFilter =
            match <uniswap_v4_pool_manager::SwapFilter as ethers::contract::EthEvent>::decode_log(
                &RawLog {
                    topics: log.topics.clone(),
                    data: log.data.to_vec(),
                },
            ) {
                Ok(event) => event,
                Err(e) => {
                    warn!("Failed to decode V4 swap event: {}", e);
                    return Ok(()); // Skip if not a swap event
                }
            };

        // Extract pool ID (bytes32) from the event
        let pool_id: [u8; 32] = swap_event.id.into();
        let pool_id_hex = format!("0x{}", hex::encode(pool_id));

        // Get or fetch token information for this pool ID
        let (token0, token1, decimals0, decimals1, pool_address) =
            match Self::get_or_fetch_v4_pool_info(pool_id, v4_cache, config).await {
                Ok(info) => info,
                Err(e) => {
                    // V4 pool info not cached and couldn't be fetched
                    // This might happen for new pools or if we don't have a way to derive tokens from pool ID
                    error!(
                        "❌ Failed to get V4 pool info for pool ID {}: {}",
                        pool_id_hex, e
                    );
                    return Ok(());
                }
            };

        // Extract sqrtPriceX96 from the event
        let sqrt_price_x96 = swap_event.sqrt_price_x96;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        debug!(
            "V4 Swap: Pool ID {:?} - sqrtPriceX96: {}, token0: {:?}, token1: {:?}",
            pool_id, sqrt_price_x96, token0, token1
        );

        // Calculate price from sqrtPriceX96 (same as V3)
        let price_token0_in_token1 =
            Self::sqrt_price_x96_to_price(sqrt_price_x96, decimals0, decimals1);

        let price_token1_in_token0 = if price_token0_in_token1 > 0.0 {
            1.0 / price_token0_in_token1
        } else {
            0.0
        };

        // Update prices using the derived pool address
        Self::update_token_price(
            token0,
            token1,
            price_token0_in_token1,
            pool_address,
            timestamp,
            DexVersion::UniswapV4,
            decimals0,
            price_store,
            config,
        );

        Self::update_token_price(
            token1,
            token0,
            price_token1_in_token0,
            pool_address,
            timestamp,
            DexVersion::UniswapV4,
            decimals1,
            price_store,
            config,
        );

        Ok(())
    }

    /// Get or fetch V4 pool information from pool ID using StateView contract or subgraph
    /// The pool ID in V4 is derived from: keccak256(abi.encode(currency0, currency1, fee, tickSpacing, hooks))
    /// We use The Graph subgraph or event lookups to get pool details
    async fn get_or_fetch_v4_pool_info(
        pool_id: [u8; 32],
        v4_cache: &Arc<DashMap<[u8; 32], (Address, Address, u8, u8, Address)>>,
        _config: &EthConfig,
    ) -> Result<(Address, Address, u8, u8, Address)> {
        let pool_id_hex = format!("0x{}", hex::encode(pool_id));

        // Check cache first
        if let Some(info) = v4_cache.get(&pool_id) {
            debug!("✓ V4 pool {} found in cache", pool_id_hex);
            return Ok(*info.value());
        }

        // V4 PoolManager address (mainnet)
        let v4_pool_manager: Address = "0x000000000004444c5dc75cB358380D2e3dE08A90"
            .parse()
            .context("Invalid V4 PoolManager address")?;

        // Try to fetch from subgraph if configured
        let subgraph_url = std::env::var("UNISWAP_V4_SUBGRAPH_URL")
            .context("UNISWAP_V4_SUBGRAPH_URL environment variable not set. V4 requires a subgraph for pool lookups.")?;

        // V4 subgraph structure: Let's query all available fields to understand the schema
        // The official V4 subgraph might use different field names
        let query = format!(
            r#"{{
                pool(id: "{}")
                    {{
                        id
                        token0 {{
                        id
                        decimals
                        }}
                        token1 {{
                        id
                        decimals
                        }}
                        feeTier
                        tickSpacing
                        hooks
                    }}
        }}"#,
            pool_id_hex.to_lowercase()
        );

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let response = client
            .post(&subgraph_url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await
            .context("Failed to query V4 subgraph")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error body".to_string());
            return Err(anyhow::anyhow!(
                "Subgraph returned error status {}: {}",
                status,
                error_body
            ));
        }

        let result = response
            .json::<serde_json::Value>()
            .await
            .context("Failed to parse subgraph response")?;

        // Check for GraphQL errors
        if let Some(errors) = result.get("errors") {
            return Err(anyhow::anyhow!(
                "Subgraph query returned errors: {}",
                serde_json::to_string_pretty(errors).unwrap_or_else(|_| format!("{:?}", errors))
            ));
        }

        // Check if pool exists (might be null for new/unindexed pools)
        let pool_value = &result["data"]["pool"];
        if pool_value.is_null() {
            return Err(anyhow::anyhow!(
                "Pool {} not found in subgraph (pool is null - might be too new or not indexed yet)",
                pool_id_hex
            ));
        }

        let pool = pool_value.as_object().context(format!(
            "Pool {} has invalid format in subgraph. Response: {}",
            pool_id_hex,
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{:?}", result))
        ))?;

        // In V4 subgraph, token0 and token1 are nested objects with id and decimals
        let token0_obj = pool
            .get("token0")
            .and_then(|v| v.as_object())
            .context(format!(
                "Missing or invalid token0 in pool. Pool data: {}",
                serde_json::to_string_pretty(pool).unwrap_or_else(|_| format!("{:?}", pool))
            ))?;

        let token1_obj = pool
            .get("token1")
            .and_then(|v| v.as_object())
            .context("Missing or invalid token1 in pool")?;

        // Extract token addresses (id field)
        let token0_str = token0_obj
            .get("id")
            .and_then(|v| v.as_str())
            .context("Missing token0.id")?;

        let token1_str = token1_obj
            .get("id")
            .and_then(|v| v.as_str())
            .context("Missing token1.id")?;

        // Extract decimals directly from the nested objects
        let dec0 = token0_obj
            .get("decimals")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| token0_obj.get("decimals").and_then(|v| v.as_u64()))
            .context("Missing or invalid token0.decimals")?;

        let dec1 = token1_obj
            .get("decimals")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| token1_obj.get("decimals").and_then(|v| v.as_u64()))
            .context("Missing or invalid token1.decimals")?;

        let token0 = token0_str
            .parse::<Address>()
            .context("Invalid token0 address")?;
        let token1 = token1_str
            .parse::<Address>()
            .context("Invalid token1 address")?;

        let pool_info = (token0, token1, dec0 as u8, dec1 as u8, v4_pool_manager);
        v4_cache.insert(pool_id, pool_info);

        Ok(pool_info)
    }
}
