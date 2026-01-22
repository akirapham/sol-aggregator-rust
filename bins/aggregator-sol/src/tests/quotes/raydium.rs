use crate::aggregator::DexAggregator;
use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
use crate::pool_data_types::*;
use crate::pool_manager::PoolStateManager;
use crate::tests::quotes::common::*;
use crate::types::Token;
use borsh::BorshDeserialize;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::common::current_timestamp;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_amm_v4::types::AmmInfo as RaydiumAmmInfoRaw;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState as RaydiumClmmStateRaw;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::types::PoolState as RaydiumCpmmStateRaw;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::test]
async fn test_raydium_amm_v4_quote() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_amm_v4"]).await;

    // Real SOL-PINPIN Raydium AMM V4 pool
    let pool_address = Pubkey::from_str("8WwcNqdZjCY5Pt7AkhupAFknV2txca9sq6YBkGzLbvdt").unwrap();
    let pinpin_mint = Pubkey::from_str("Dfh5DzRgSvvCFDoYc2ciTkMrbDfRKybA4SoFbPmApump").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch AMM pool account");

    // Deserialize Raydium AMM state (no discriminator for AMM V4)
    let amm_info =
        RaydiumAmmInfoRaw::try_from_slice(&account.data).expect("Failed to deserialize AMM info");

    println!("AMM info: {:#?}", amm_info);

    // Fetch token vault accounts to get real reserves
    let vault_coin_account = rpc_client
        .get_account(&amm_info.token_coin)
        .await
        .expect("Failed to fetch coin vault");
    let vault_pc_account = rpc_client
        .get_account(&amm_info.token_pc)
        .await
        .expect("Failed to fetch pc vault");

    // Parse token account data (amount at offset 64)
    let coin_reserve = u64::from_le_bytes(vault_coin_account.data[64..72].try_into().unwrap());
    let pc_reserve = u64::from_le_bytes(vault_pc_account.data[64..72].try_into().unwrap());

    println!("Real reserves: coin={}, pc={}", coin_reserve, pc_reserve);

    // Create pool state with real data
    // Note: Serum market fields are not critical for AMM quote calculation
    let pool_state = PoolState::RaydiumAmmV4(RaydiumAmmV4PoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        base_mint: amm_info.coin_mint,
        quote_mint: amm_info.pc_mint,
        amm_authority: amm_info.amm_owner,
        amm_open_orders: amm_info.open_orders,
        amm_target_orders: amm_info.target_orders,
        pool_coin_token_account: amm_info.token_coin,
        pool_pc_token_account: amm_info.token_pc,
        serum_program: amm_info.serum_dex,
        serum_market: amm_info.market,
        serum_bids: Pubkey::default(),
        serum_asks: Pubkey::default(),
        serum_event_queue: Pubkey::default(),
        serum_coin_vault_account: Pubkey::default(),
        serum_pc_vault_account: Pubkey::default(),
        serum_vault_signer: Pubkey::default(),
        last_updated: u64::MAX,
        base_reserve: coin_reserve,
        quote_reserve: pc_reserve,
        liquidity_usd: (coin_reserve as f64 * 2.0) / 1e9 * 200.0, // Estimate
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> PINPIN (base is PINPIN/coin, quote is SOL/pc)
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: pinpin_mint,
            symbol: Some("PINPIN".to_string()),
            name: Some("Pinpin Token".to_string()),
            decimals: amm_info.coin_decimals as u8,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_cpmm_quote() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_cpmm"]).await;

    // Real SOL-SURGE Raydium CPMM pool
    let pool_address = Pubkey::from_str("BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5").unwrap();
    let surge_mint = Pubkey::from_str("3z2tRjNuQjoq6UDcw4zyEPD1Eb5KXMPYb4GWFzVT1DPg").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch CPMM pool account");

    // Deserialize Raydium CPMM state (skip 8-byte discriminator)
    let cpmm_state = RaydiumCpmmStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize CPMM state");

    println!("CPMM pool state: {:#?}", cpmm_state);

    // Fetch token vault accounts to get real reserves
    let vault0_account = rpc_client
        .get_account(&cpmm_state.token0_vault)
        .await
        .expect("Failed to fetch token0 vault");
    let vault1_account = rpc_client
        .get_account(&cpmm_state.token1_vault)
        .await
        .expect("Failed to fetch token1 vault");

    // Parse token account data (amount at offset 64)
    let token0_reserve = u64::from_le_bytes(vault0_account.data[64..72].try_into().unwrap());
    let token1_reserve = u64::from_le_bytes(vault1_account.data[64..72].try_into().unwrap());

    println!(
        "Real reserves: token0={}, token1={}",
        token0_reserve, token1_reserve
    );

    // Create pool state with real data
    let pool_state = PoolState::RaydiumCpmm(RaydiumCpmmPoolState {
        slot: 100,
        transaction_index: None,
        status: cpmm_state.status,
        address: pool_address,
        token0: cpmm_state.token0_mint,
        token1: cpmm_state.token1_mint,
        token0_vault: cpmm_state.token0_vault,
        token1_vault: cpmm_state.token1_vault,
        token0_reserve,
        token1_reserve,
        amm_config: cpmm_state.amm_config,
        observation_state: cpmm_state.observation_key,
        last_updated: u64::MAX,
        liquidity_usd: (token0_reserve as f64 * 2.0) / 1e9 * 200.0, // Estimate
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> SURGE (token0 is SOL, token1 is SURGE)
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: surge_mint,
            symbol: Some("SURGE".to_string()),
            name: Some("Surge Token".to_string()),
            decimals: cpmm_state.mint1_decimals,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_clmm_quote() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_clmm"]).await;

    // Real SOL-RAY Raydium CLMM pool
    let pool_address = Pubkey::from_str("2AXXcN6oN9bBT5owwmTH53C7QHUXvhLeu718Kqt8rvY2").unwrap();
    let ray_mint = Pubkey::from_str("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch CLMM pool account");

    // Deserialize Raydium CLMM state (skip 8-byte discriminator)
    let clmm_state = RaydiumClmmStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize CLMM state");

    println!(
        "DEBUG: Deserialized CLMM State Mints: {:?} / {:?}",
        clmm_state.token_mint0, clmm_state.token_mint1
    );
    println!(
        "DEBUG: SOL Mint: {:?}",
        Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap()
    );

    println!("CLMM pool state: {:#?}", clmm_state);

    // Fetch token vault accounts to get real reserves
    let vault0_account = rpc_client
        .get_account(&clmm_state.token_vault0)
        .await
        .expect("Failed to fetch vault 0");
    let vault1_account = rpc_client
        .get_account(&clmm_state.token_vault1)
        .await
        .expect("Failed to fetch vault 1");

    // Parse token account data (amount at offset 64)
    let token0_reserve = u64::from_le_bytes(vault0_account.data[64..72].try_into().unwrap());
    let token1_reserve = u64::from_le_bytes(vault1_account.data[64..72].try_into().unwrap());

    println!(
        "Real reserves: token0={}, token1={}",
        token0_reserve, token1_reserve
    );

    // Create temporary RaydiumClmmPoolState for fetching ticks
    let temp_pool_state = crate::pool_data_types::raydium_clmm::RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: clmm_state.amm_config,
        token_mint0: clmm_state.token_mint0,
        token_mint1: clmm_state.token_mint1,
        token_vault0: clmm_state.token_vault0,
        token_vault1: clmm_state.token_vault1,
        observation_key: clmm_state.observation_key,
        tick_spacing: clmm_state.tick_spacing,
        liquidity: clmm_state.liquidity,
        liquidity_usd: 100000.0,
        sqrt_price_x64: clmm_state.sqrt_price_x64,
        tick_current_index: clmm_state.tick_current,
        status: clmm_state.status,
        tick_array_bitmap: clmm_state.tick_array_bitmap,
        open_time: clmm_state.open_time,
        tick_array_state: HashMap::new(),
        tick_array_bitmap_extension: None,
        last_updated: u64::MAX,
        token0_reserve,
        token1_reserve,
        is_state_keys_initialized: true,
    };

    // Fetch all tick arrays using the helper function
    let tick_fetcher = TickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array()),
    );

    let tick_arrays = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &temp_pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    println!("Fetched {} tick arrays", tick_arrays.len());

    // Convert fetched tick arrays to HashMap
    let tick_array_state: HashMap<i32, _> = tick_arrays
        .into_iter()
        .map(|ta| (ta.start_tick_index, ta))
        .collect();

    // Create final RaydiumClmmPoolState with real data and tick arrays
    let pool_state = PoolState::RadyiumClmm(Box::new(RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: clmm_state.amm_config,
        token_mint0: clmm_state.token_mint0,
        token_mint1: clmm_state.token_mint1,
        token_vault0: clmm_state.token_vault0,
        token_vault1: clmm_state.token_vault1,
        observation_key: clmm_state.observation_key,
        tick_spacing: clmm_state.tick_spacing,
        liquidity: clmm_state.liquidity,
        liquidity_usd: 100000.0,
        sqrt_price_x64: clmm_state.sqrt_price_x64,
        tick_current_index: clmm_state.tick_current,
        status: clmm_state.status,
        tick_array_bitmap: clmm_state.tick_array_bitmap,
        open_time: clmm_state.open_time,
        tick_array_state,
        tick_array_bitmap_extension: None,
        last_updated: u64::MAX,
        token0_reserve,
        token1_reserve,
        is_state_keys_initialized: true,
    }));

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> RAY (token0 is SOL, token1 is RAY)
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: ray_mint,
            symbol: Some("RAY".to_string()),
            name: Some("Raydium".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_amm_v4_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_amm_v4"]).await;

    // Real SOL-PINPIN Raydium AMM V4 pool
    let pool_address = Pubkey::from_str("8WwcNqdZjCY5Pt7AkhupAFknV2txca9sq6YBkGzLbvdt").unwrap();
    let pinpin_mint = Pubkey::from_str("Dfh5DzRgSvvCFDoYc2ciTkMrbDfRKybA4SoFbPmApump").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch AMM pool account");

    // Deserialize Raydium AMM state
    let amm_info =
        RaydiumAmmInfoRaw::try_from_slice(&account.data).expect("Failed to deserialize AMM info");

    // Fetch reserves
    let vault_coin_account = rpc_client
        .get_account(&amm_info.token_coin)
        .await
        .expect("Failed to fetch coin vault");
    let vault_pc_account = rpc_client
        .get_account(&amm_info.token_pc)
        .await
        .expect("Failed to fetch pc vault");
    let coin_reserve = u64::from_le_bytes(vault_coin_account.data[64..72].try_into().unwrap());
    let pc_reserve = u64::from_le_bytes(vault_pc_account.data[64..72].try_into().unwrap());

    // Create pool state
    let pool_state = PoolState::RaydiumAmmV4(RaydiumAmmV4PoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        base_mint: amm_info.coin_mint,
        quote_mint: amm_info.pc_mint,
        amm_authority: amm_info.amm_owner,
        amm_open_orders: amm_info.open_orders,
        amm_target_orders: amm_info.target_orders,
        pool_coin_token_account: amm_info.token_coin,
        pool_pc_token_account: amm_info.token_pc,
        serum_program: amm_info.serum_dex,
        serum_market: amm_info.market,
        serum_bids: Pubkey::default(),
        serum_asks: Pubkey::default(),
        serum_event_queue: Pubkey::default(),
        serum_coin_vault_account: Pubkey::default(),
        serum_pc_vault_account: Pubkey::default(),
        serum_vault_signer: Pubkey::default(),
        last_updated: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
            + 3600_000_000,
        base_reserve: coin_reserve,
        quote_reserve: pc_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: PINPIN -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: pinpin_mint,
            symbol: Some("PINPIN".to_string()),
            name: Some("Pinpin Token".to_string()),
            decimals: amm_info.coin_decimals as u8,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        100_000_000, // Amount of PINPIN to swap
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_cpmm_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_cpmm"]).await;
    let pool_address = Pubkey::from_str("BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5").unwrap();
    let surge_mint = Pubkey::from_str("3z2tRjNuQjoq6UDcw4zyEPD1Eb5KXMPYb4GWFzVT1DPg").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch CPMM pool account");
    let cpmm_state = RaydiumCpmmStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize CPMM state");

    let vault0_account = rpc_client
        .get_account(&cpmm_state.token0_vault)
        .await
        .expect("Failed to fetch token0 vault");
    let vault1_account = rpc_client
        .get_account(&cpmm_state.token1_vault)
        .await
        .expect("Failed to fetch token1 vault");
    let token0_reserve = u64::from_le_bytes(vault0_account.data[64..72].try_into().unwrap());
    let token1_reserve = u64::from_le_bytes(vault1_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::RaydiumCpmm(RaydiumCpmmPoolState {
        slot: 100,
        transaction_index: None,
        status: cpmm_state.status,
        address: pool_address,
        token0: cpmm_state.token0_mint,
        token1: cpmm_state.token1_mint,
        token0_vault: cpmm_state.token0_vault,
        token1_vault: cpmm_state.token1_vault,
        token0_reserve,
        token1_reserve,
        amm_config: cpmm_state.amm_config,
        observation_state: cpmm_state.observation_key,
        last_updated: u64::MAX,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SURGE -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: surge_mint,
            symbol: Some("SURGE".to_string()),
            name: Some("Surge Token".to_string()),
            decimals: cpmm_state.mint1_decimals,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_clmm_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_clmm"]).await;
    let pool_address = Pubkey::from_str("2AXXcN6oN9bBT5owwmTH53C7QHUXvhLeu718Kqt8rvY2").unwrap();
    let ray_mint = Pubkey::from_str("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch CLMM pool account");
    let clmm_state = RaydiumClmmStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize CLMM state");

    let vault0_account = rpc_client
        .get_account(&clmm_state.token_vault0)
        .await
        .expect("Failed to fetch vault 0");
    let vault1_account = rpc_client
        .get_account(&clmm_state.token_vault1)
        .await
        .expect("Failed to fetch vault 1");
    let token0_reserve = u64::from_le_bytes(vault0_account.data[64..72].try_into().unwrap());
    let token1_reserve = u64::from_le_bytes(vault1_account.data[64..72].try_into().unwrap());

    let temp_pool_state = crate::pool_data_types::raydium_clmm::RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: clmm_state.amm_config,
        token_mint0: clmm_state.token_mint0,
        token_mint1: clmm_state.token_mint1,
        token_vault0: clmm_state.token_vault0,
        token_vault1: clmm_state.token_vault1,
        observation_key: clmm_state.observation_key,
        tick_spacing: clmm_state.tick_spacing,
        liquidity: clmm_state.liquidity,
        liquidity_usd: 100000.0,
        sqrt_price_x64: clmm_state.sqrt_price_x64,
        tick_current_index: clmm_state.tick_current,
        status: clmm_state.status,
        tick_array_bitmap: clmm_state.tick_array_bitmap,
        open_time: clmm_state.open_time,
        tick_array_state: HashMap::new(),
        tick_array_bitmap_extension: None,
        last_updated: u64::MAX,
        token0_reserve,
        token1_reserve,
        is_state_keys_initialized: true,
    };

    let tick_fetcher = TickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array()),
    );

    let tick_arrays = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &temp_pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    let tick_array_state: HashMap<i32, _> = tick_arrays
        .into_iter()
        .map(|ta| (ta.start_tick_index, ta))
        .collect();

    let pool_state = PoolState::RadyiumClmm(Box::new(RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: clmm_state.amm_config,
        token_mint0: clmm_state.token_mint0,
        token_mint1: clmm_state.token_mint1,
        token_vault0: clmm_state.token_vault0,
        token_vault1: clmm_state.token_vault1,
        observation_key: clmm_state.observation_key,
        tick_spacing: clmm_state.tick_spacing,
        liquidity: clmm_state.liquidity,
        liquidity_usd: 100000.0,
        sqrt_price_x64: clmm_state.sqrt_price_x64,
        tick_current_index: clmm_state.tick_current,
        status: clmm_state.status,
        tick_array_bitmap: clmm_state.tick_array_bitmap,
        open_time: clmm_state.open_time,
        tick_array_state,
        tick_array_bitmap_extension: None,
        last_updated: u64::MAX,
        token0_reserve,
        token1_reserve,
        is_state_keys_initialized: true,
    }));

    pool_manager.inject_pool(pool_state).await;

    // Test swap: RAY -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: ray_mint,
            symbol: Some("RAY".to_string()),
            name: Some("Raydium".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}
