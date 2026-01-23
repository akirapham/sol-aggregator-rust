use crate::aggregator::DexAggregator;
use crate::api::dto::QuoteRequest;
use crate::api::AppState;
use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
use crate::pool_data_types::*;
use crate::tests::quotes::common::*;
use crate::types::Token;
use base64::Engine;
use borsh::BorshDeserialize;
use solana_client::rpc_config::{CommitmentConfig, RpcSimulateTransactionConfig};
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
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
    // Test Round Trip: SOL -> PINPIN -> SOL
    verify_quote_round_trip(
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
        100, // 1% tolerance
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
    // Test Round Trip: SOL -> SURGE -> SOL
    verify_quote_round_trip(
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
        100, // 1% tolerance
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
    // Test Round Trip: SOL -> RAY -> SOL
    verify_quote_round_trip(
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
        100, // 1% tolerance
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

    // Test Round Trip: PINPIN -> SOL -> PINPIN
    verify_quote_round_trip(
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
        100, // 1% tolerance
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

    // Test Round Trip: SURGE -> SOL -> SURGE
    verify_quote_round_trip(
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
        100, // 1% tolerance
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

    // Test Round Trip: RAY -> SOL -> RAY
    verify_quote_round_trip(
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
        100, // 1% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_raydium_amm_v4_quote_simulation() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_amm_v4"]).await;

    // Real SOL-PINPIN Raydium AMM V4 pool
    let pool_address = Pubkey::from_str("8WwcNqdZjCY5Pt7AkhupAFknV2txca9sq6YBkGzLbvdt").unwrap();
    let pinpin_mint = Pubkey::from_str("Dfh5DzRgSvvCFDoYc2ciTkMrbDfRKybA4SoFbPmApump").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch AMM pool account");

    let amm_info =
        RaydiumAmmInfoRaw::try_from_slice(&account.data).expect("Failed to deserialize AMM info");

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

    let pool_state = PoolState::RaydiumAmmV4(RaydiumAmmV4PoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        base_mint: amm_info.coin_mint,
        quote_mint: amm_info.pc_mint,
        amm_authority: Pubkey::from_str("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1").unwrap(),
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
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state.clone()).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: pinpin_mint,
            symbol: Some("PINPIN".to_string()),
            name: Some("Pinpin Token".to_string()),
            decimals: amm_info.coin_decimals as u8,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // Construct Quote Request: SOL -> PINPIN
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let input_amount = 100_000_000; // 0.1 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: pinpin_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 100,
    };

    println!("Calling get_quote handler (Raydium V4 Forward)...");
    let result =
        crate::api::handlers::get_quote(axum::extract::State(state.clone()), axum::Json(request))
            .await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Quote Response!");
            println!("Transaction Base64: {}", response.transaction);

            let tx_bytes = base64::engine::general_purpose::STANDARD
                .decode(&response.transaction)
                .expect("Failed to decode base64 transaction");

            let (transaction, _): (Transaction, usize) =
                bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
                    .expect("Failed to deserialize transaction");

            println!("Simulating transaction on-chain...");
            let simulation = rpc_client.simulate_transaction(&transaction).await;

            match simulation {
                Ok(sim_result) => {
                    // log all accounts details of each instruction
                    println!("amm_info: {:#?}", amm_info.clone());
                    for instruction in transaction.message.instructions.iter() {
                        println!(
                            "Instruction: {}",
                            transaction.message.account_keys[instruction.program_id_index as usize]
                        );
                        for account_index in instruction.accounts.iter() {
                            // log account address from account index
                            println!(
                                "Account: {}",
                                transaction.message.account_keys[*account_index as usize]
                            );
                        }
                    }

                    if let Some(err) = sim_result.value.err {
                        panic!("Simulation failed: {:?}", err);
                    }

                    let units_consumed = sim_result
                        .value
                        .units_consumed
                        .expect("Should have units consumed");
                    assert!(
                        units_consumed > 0,
                        "Simulation should consume compute units"
                    );
                    println!(
                        "✅ Simulation Successful! Consumed {} compute units",
                        units_consumed
                    );
                }
                Err(e) => panic!("RPC Error during simulation: {}", e),
            }
        }
        Err((status, axum::Json(error_res))) => {
            panic!("Quote request failed: {} - {}", status, error_res.error);
        }
    }
}

#[tokio::test]
async fn test_raydium_amm_v4_quote_simulation_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_amm_v4"]).await;

    // Real SOL-PINPIN Raydium AMM V4 pool (Reverse: PINPIN -> SOL)
    let pool_address = Pubkey::from_str("8WwcNqdZjCY5Pt7AkhupAFknV2txca9sq6YBkGzLbvdt").unwrap();
    let pinpin_mint = Pubkey::from_str("Dfh5DzRgSvvCFDoYc2ciTkMrbDfRKybA4SoFbPmApump").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch AMM pool account");

    let amm_info =
        RaydiumAmmInfoRaw::try_from_slice(&account.data).expect("Failed to deserialize AMM info");

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

    let pool_state = PoolState::RaydiumAmmV4(RaydiumAmmV4PoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        base_mint: amm_info.coin_mint,
        quote_mint: amm_info.pc_mint,
        amm_authority: Pubkey::from_str("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1").unwrap(),
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
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: pinpin_mint,
            symbol: Some("PINPIN".to_string()),
            name: Some("Pinpin Token".to_string()),
            decimals: amm_info.coin_decimals as u8,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // Composite Simulation
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    let buy_input_amount = 100_000_000;
    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        output_token: pinpin_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: buy_input_amount,
        slippage_bps: 100,
    };

    println!("Getting Buy Quote...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(buy_request),
    )
    .await
    .expect("Buy request failed");

    let buy_response = match buy_result {
        axum::Json(res) => res,
    };

    let buy_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&buy_response.transaction)
        .expect("Failed to decode buy transaction");

    let (buy_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&buy_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize buy transaction");

    let buy_output_amount: u64 = buy_response.output_amount;
    let sell_input_amount = buy_output_amount / 2;

    println!("Getting Sell Quote (Amount: {})...", sell_input_amount);
    let sell_request = QuoteRequest {
        input_token: pinpin_mint.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 100,
    };

    let sell_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(sell_request),
    )
    .await
    .expect("Sell request failed");

    let sell_response = match sell_result {
        axum::Json(res) => res,
    };

    let sell_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sell_response.transaction)
        .expect("Failed to decode sell transaction");

    let (sell_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&sell_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize sell transaction");

    let recent_blockhash = buy_transaction.message.recent_blockhash;

    let get_instructions = |tx: &Transaction| -> Vec<Instruction> {
        let message = &tx.message;
        message
            .instructions
            .iter()
            .map(|ix| Instruction {
                program_id: message.account_keys[ix.program_id_index as usize],
                accounts: ix
                    .accounts
                    .iter()
                    .map(|&acc_idx| {
                        let idx = acc_idx as usize;
                        let is_signer = idx < message.header.num_required_signatures as usize;
                        let is_writable = if is_signer {
                            idx < (message.header.num_required_signatures
                                - message.header.num_readonly_signed_accounts)
                                as usize
                        } else {
                            idx < (message.account_keys.len()
                                - message.header.num_readonly_unsigned_accounts as usize)
                        };

                        solana_sdk::instruction::AccountMeta {
                            pubkey: message.account_keys[idx],
                            is_signer,
                            is_writable,
                        }
                    })
                    .collect(),
                data: ix.data.clone(),
            })
            .collect::<Vec<_>>()
    };

    let mut instructions = get_instructions(&buy_transaction);
    let sell_instructions = get_instructions(&sell_transaction);

    instructions.extend(sell_instructions);

    let composite_transaction = Transaction::new_with_payer(&instructions, Some(&payer));
    let mut composite_transaction = composite_transaction;
    composite_transaction.message.recent_blockhash = recent_blockhash;

    println!("Simulating Composite Buy -> Sell Transaction...");
    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::processed()),
        ..RpcSimulateTransactionConfig::default()
    };

    let simulation = rpc_client
        .simulate_transaction_with_config(&composite_transaction, config)
        .await;

    match simulation {
        Ok(sim_result) => {
            println!("Composite Simulation Result: {:#?}", sim_result);
            if let Some(logs) = &sim_result.value.logs {
                for log in logs {
                    println!("  {}", log);
                }
            }
            if let Some(err) = sim_result.value.err {
                panic!("Composite Simulation failed: {:?}", err);
            }

            let units_consumed = sim_result
                .value
                .units_consumed
                .expect("Should have units consumed");
            assert!(
                units_consumed > 0,
                "Simulation should consume compute units"
            );
            println!(
                "✅ Composite Raydium V4 Simulation Successful! Consumed {} compute units",
                units_consumed
            );
        }
        Err(e) => panic!("RPC Error during Composite simulation: {}", e),
    }
}

#[tokio::test]
async fn test_raydium_cpmm_quote_simulation() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_cpmm"]).await;

    // Real SOL-SURGE Raydium CPMM pool
    let pool_address = Pubkey::from_str("BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5").unwrap();
    let surge_mint = Pubkey::from_str("3z2tRjNuQjoq6UDcw4zyEPD1Eb5KXMPYb4GWFzVT1DPg").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch CPMM pool account");

    // Deserialize Raydium CPMM state (skip 8-byte discriminator)
    let cpmm_state = RaydiumCpmmStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize CPMM state");

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
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: surge_mint,
            symbol: Some("SURGE".to_string()),
            name: Some("Surge Token".to_string()),
            decimals: cpmm_state.mint1_decimals,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // Construct Quote Request: SOL -> SURGE
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let input_amount = 100_000_000; // 0.1 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: surge_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 100,
    };

    println!("Calling get_quote handler (Raydium CPMM Forward)...");
    let result =
        crate::api::handlers::get_quote(axum::extract::State(state.clone()), axum::Json(request))
            .await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Quote Response!");
            println!("Transaction Base64: {}", response.transaction);

            let tx_bytes = base64::engine::general_purpose::STANDARD
                .decode(&response.transaction)
                .expect("Failed to decode base64 transaction");

            let (transaction, _): (Transaction, usize) =
                bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
                    .expect("Failed to deserialize transaction");

            println!("Simulating transaction on-chain...");
            let simulation = rpc_client.simulate_transaction(&transaction).await;

            match simulation {
                Ok(sim_result) => {
                    if let Some(logs) = &sim_result.value.logs {
                        for log in logs {
                            println!("  {}", log);
                        }
                    }
                    if let Some(err) = sim_result.value.err {
                        panic!("Simulation failed: {:?}", err);
                    }

                    let units_consumed = sim_result
                        .value
                        .units_consumed
                        .expect("Should have units consumed");
                    assert!(
                        units_consumed > 0,
                        "Simulation should consume compute units"
                    );
                    println!(
                        "✅ Simulation Successful! Consumed {} compute units",
                        units_consumed
                    );
                }
                Err(e) => panic!("RPC Error during simulation: {}", e),
            }
        }
        Err((status, axum::Json(error_res))) => {
            panic!("Quote request failed: {} - {}", status, error_res.error);
        }
    }
}

#[tokio::test]
async fn test_raydium_cpmm_quote_simulation_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_cpmm"]).await;

    // Real SOL-SURGE Raydium CPMM pool
    let pool_address = Pubkey::from_str("BScfGKZf9YDfpL11hZQnCQPskPrdeyFcvCjSA5qupEH5").unwrap();
    let surge_mint = Pubkey::from_str("3z2tRjNuQjoq6UDcw4zyEPD1Eb5KXMPYb4GWFzVT1DPg").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
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
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: surge_mint,
            symbol: Some("SURGE".to_string()),
            name: Some("Surge Token".to_string()),
            decimals: cpmm_state.mint1_decimals,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // Composite Simulation
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    let buy_input_amount = 100_000_000; // 0.1 SOL
    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        output_token: surge_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: buy_input_amount,
        slippage_bps: 100,
    };

    println!("Getting Buy Quote...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(buy_request),
    )
    .await
    .expect("Buy request failed");

    let buy_response = match buy_result {
        axum::Json(res) => res,
    };

    let buy_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&buy_response.transaction)
        .expect("Failed to decode buy transaction");

    let (buy_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&buy_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize buy transaction");

    let buy_output_amount: u64 = buy_response.output_amount;
    let sell_input_amount = buy_output_amount / 2;

    println!("Getting Sell Quote (Amount: {})...", sell_input_amount);
    let sell_request = QuoteRequest {
        input_token: surge_mint.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 100,
    };

    let sell_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(sell_request),
    )
    .await
    .expect("Sell request failed");

    let sell_response = match sell_result {
        axum::Json(res) => res,
    };

    let sell_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sell_response.transaction)
        .expect("Failed to decode sell transaction");

    let (sell_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&sell_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize sell transaction");

    let recent_blockhash = buy_transaction.message.recent_blockhash;

    let get_instructions = |tx: &Transaction| -> Vec<Instruction> {
        let message = &tx.message;
        message
            .instructions
            .iter()
            .map(|ix| Instruction {
                program_id: message.account_keys[ix.program_id_index as usize],
                accounts: ix
                    .accounts
                    .iter()
                    .map(|&acc_idx| {
                        let idx = acc_idx as usize;
                        let is_signer = idx < message.header.num_required_signatures as usize;
                        let is_writable = if is_signer {
                            idx < (message.header.num_required_signatures
                                - message.header.num_readonly_signed_accounts)
                                as usize
                        } else {
                            idx < (message.account_keys.len()
                                - message.header.num_readonly_unsigned_accounts as usize)
                        };

                        solana_sdk::instruction::AccountMeta {
                            pubkey: message.account_keys[idx],
                            is_signer,
                            is_writable,
                        }
                    })
                    .collect(),
                data: ix.data.clone(),
            })
            .collect::<Vec<_>>()
    };

    let mut instructions = get_instructions(&buy_transaction);
    let mut sell_instructions = get_instructions(&sell_transaction);

    // Remove Compute Budget instruction from sell_instructions to avoid duplicate
    let compute_budget_id =
        Pubkey::from_str("ComputeBudget111111111111111111111111111111").unwrap();
    sell_instructions.retain(|ix| ix.program_id != compute_budget_id);

    instructions.extend(sell_instructions);

    let composite_transaction = Transaction::new_with_payer(&instructions, Some(&payer));
    let mut composite_transaction = composite_transaction;
    composite_transaction.message.recent_blockhash = recent_blockhash;

    println!("Simulating Composite Buy -> Sell Transaction...");
    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::processed()),
        ..RpcSimulateTransactionConfig::default()
    };

    let simulation = rpc_client
        .simulate_transaction_with_config(&composite_transaction, config)
        .await;

    match simulation {
        Ok(sim_result) => {
            if let Some(logs) = &sim_result.value.logs {
                for log in logs {
                    println!("  {}", log);
                }
            }
            if let Some(err) = sim_result.value.err {
                panic!("Composite Simulation failed: {:?}", err);
            }

            let units_consumed = sim_result
                .value
                .units_consumed
                .expect("Should have units consumed");
            assert!(
                units_consumed > 0,
                "Simulation should consume compute units"
            );
            println!(
                "✅ Composite Raydium CPMM Simulation Successful! Consumed {} compute units",
                units_consumed
            );
        }
        Err(e) => panic!("RPC Error during Composite simulation: {}", e),
    }
}

#[tokio::test]
async fn test_raydium_clmm_quote_simulation() {
    use crate::aggregator::DexAggregator;
    use crate::api::dto::QuoteRequest;
    use crate::api::AppState;
    use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
    use crate::pool_data_types::PoolState;
    use crate::tests::quotes::common::create_test_setup;
    use crate::tests::quotes::common::wsol_token;
    use crate::types::Token;
    use base64::Engine;
    use borsh::BorshDeserialize;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_sdk::transaction::Transaction;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;

    let (pool_manager, config) = create_test_setup(vec!["raydium"]).await;

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    // 1. Fetch real pool state for SOL-RAY CLMM
    // Pool Address: 2AXXcN6oN9bBT5owwmTH53C7QHUXvhLeu718Kqt8rvY2 (Raydium CLMM SOL-RAY)
    let pool_address = Pubkey::from_str("2AXXcN6oN9bBT5owwmTH53C7QHUXvhLeu718Kqt8rvY2").unwrap();

    println!("Fetching Raydium CLMM Pool State: {}", pool_address);
    let account = rpc_client.get_account(&pool_address).await.unwrap();

    use crate::pool_data_types::raydium_clmm::RaydiumClmmPoolState;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState as RaydiumClmmStateRaw;

    let raw_state = if account.data.len() > 8 {
        RaydiumClmmStateRaw::try_from_slice(&account.data[8..])
            .expect("Failed to deserialize Raydium CLMM pool state")
    } else {
        panic!("Account data too short");
    };

    // Map Raw state to Internal state
    let pool_state = RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: raw_state.amm_config,
        token_mint0: raw_state.token_mint0,
        token_mint1: raw_state.token_mint1,
        token_vault0: raw_state.token_vault0,
        token_vault1: raw_state.token_vault1,
        observation_key: raw_state.observation_key,
        tick_spacing: raw_state.tick_spacing,
        liquidity: raw_state.liquidity,
        liquidity_usd: 100_000.0,
        sqrt_price_x64: raw_state.sqrt_price_x64,
        tick_current_index: raw_state.tick_current,
        status: raw_state.status,
        tick_array_bitmap: raw_state.tick_array_bitmap,
        open_time: raw_state.open_time,
        tick_array_state: std::collections::HashMap::new(),
        tick_array_bitmap_extension: None,
        last_updated: u64::MAX,
        token0_reserve: 0,
        token1_reserve: 0,
        is_state_keys_initialized: true,
    };

    println!("Pool Liquidity: {}", pool_state.liquidity);
    println!("Sqrt Price X64: {}", pool_state.sqrt_price_x64);

    // Fetch tick arrays
    let tick_fetcher = TickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array()),
    );

    println!("Fetching tick arrays...");
    let tick_arrays = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    println!("Fetched {} tick arrays", tick_arrays.len());

    let tick_array_state: std::collections::HashMap<i32, _> = tick_arrays
        .into_iter()
        .map(|ta| (ta.start_tick_index, ta))
        .collect();

    let mut pool_state = pool_state;
    pool_state.tick_array_state = tick_array_state;

    // Inject into PoolStateManager
    let pool_enum = PoolState::RadyiumClmm(Box::new(pool_state.clone()));
    pool_manager.inject_pool(pool_enum).await;

    // Inject Tokens
    pool_manager.inject_token(wsol_token()).await;
    let output_mint = pool_state.token_mint1; // RAY
    pool_manager
        .inject_token(Token {
            address: output_mint,
            symbol: Some("RAY".to_string()),
            name: Some("Raydium".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    // Create App State
    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // 2. Perform Quote (SOL -> RAY)
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let amount_in = 100_000_000; // 0.1 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: output_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: amount_in,
        slippage_bps: 100,
    };

    println!("Requesting Quote for 0.1 SOL -> RAY...");
    let result =
        crate::api::handlers::get_quote(axum::extract::State(state.clone()), axum::Json(request))
            .await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Quote Response!");
            println!("Output Amount: {}", response.output_amount);

            let tx_bytes = base64::engine::general_purpose::STANDARD
                .decode(&response.transaction)
                .expect("Failed to decode base64 transaction");

            let (transaction, _): (Transaction, usize) =
                bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
                    .expect("Failed to deserialize transaction");

            println!("Simulating transaction on-chain...");
            let simulation = rpc_client.simulate_transaction(&transaction).await;

            match simulation {
                Ok(sim_result) => {
                    if let Some(logs) = &sim_result.value.logs {
                        for log in logs {
                            println!("  {}", log);
                        }
                    }
                    if let Some(err) = sim_result.value.err {
                        panic!("Simulation failed: {:?}", err);
                    }
                    let units = sim_result.value.units_consumed.unwrap_or(0);
                    println!(
                        "✅ Raydium CLMM Simulation Successful! Consumed {} units",
                        units
                    );
                    assert!(units > 0);
                }
                Err(e) => panic!("RPC Error: {}", e),
            }
        }
        Err((status, axum::Json(error_res))) => {
            panic!("Quote request failed: {} - {}", status, error_res.error);
        }
    }
}

#[tokio::test]
async fn test_raydium_clmm_quote_simulation_reverse() {
    use crate::aggregator::DexAggregator;
    use crate::api::dto::QuoteRequest;
    use crate::api::AppState;
    use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
    use crate::pool_data_types::PoolState;
    use crate::tests::quotes::common::create_test_setup;
    use crate::tests::quotes::common::wsol_token;
    use crate::types::Token;
    use base64::Engine;
    use borsh::BorshDeserialize;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_sdk::instruction::Instruction;
    use solana_sdk::transaction::Transaction;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;

    let (pool_manager, config) = create_test_setup(vec!["raydium"]).await;

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    // 1. Fetch real pool state for SOL-RAY CLMM
    // Pool Address: 2AXXcN6oN9bBT5owwmTH53C7QHUXvhLeu718Kqt8rvY2 (Raydium CLMM SOL-RAY)
    let pool_address = Pubkey::from_str("2AXXcN6oN9bBT5owwmTH53C7QHUXvhLeu718Kqt8rvY2").unwrap();

    let account = rpc_client.get_account(&pool_address).await.unwrap();

    use crate::pool_data_types::raydium_clmm::RaydiumClmmPoolState;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState as RaydiumClmmStateRaw;

    let raw_state = if account.data.len() > 8 {
        RaydiumClmmStateRaw::try_from_slice(&account.data[8..])
            .expect("Failed to deserialize Raydium CLMM pool state")
    } else {
        panic!("Account data too short");
    };

    // Map Raw state to Internal state
    let pool_state = RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: raw_state.amm_config,
        token_mint0: raw_state.token_mint0,
        token_mint1: raw_state.token_mint1,
        token_vault0: raw_state.token_vault0,
        token_vault1: raw_state.token_vault1,
        observation_key: raw_state.observation_key,
        tick_spacing: raw_state.tick_spacing,
        liquidity: raw_state.liquidity,
        liquidity_usd: 100_000.0,
        sqrt_price_x64: raw_state.sqrt_price_x64,
        tick_current_index: raw_state.tick_current,
        status: raw_state.status,
        tick_array_bitmap: raw_state.tick_array_bitmap,
        open_time: raw_state.open_time,
        tick_array_state: std::collections::HashMap::new(),
        tick_array_bitmap_extension: None,
        last_updated: u64::MAX,
        token0_reserve: 0,
        token1_reserve: 0,
        is_state_keys_initialized: true,
    };

    // Fetch tick arrays
    let tick_fetcher = TickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array()),
    );

    let tick_arrays = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    let tick_array_state: std::collections::HashMap<i32, _> = tick_arrays
        .into_iter()
        .map(|ta| (ta.start_tick_index, ta))
        .collect();

    let mut pool_state = pool_state;
    pool_state.tick_array_state = tick_array_state;

    // Inject into PoolStateManager
    let pool_enum = PoolState::RadyiumClmm(Box::new(pool_state.clone()));
    pool_manager.inject_pool(pool_enum).await;

    // Inject Tokens
    pool_manager.inject_token(wsol_token()).await;
    let output_mint = pool_state.token_mint1; // RAY
    pool_manager
        .inject_token(Token {
            address: output_mint,
            symbol: Some("RAY".to_string()),
            name: Some("Raydium".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    // Create App State
    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // 2. Perform Quote (SOL -> RAY)
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();
    let amount_in = 100_000_000; // 0.1 SOL

    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: output_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: amount_in,
        slippage_bps: 100,
    };

    println!("Requesting Buy Quote for 0.1 SOL -> RAY...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(buy_request),
    )
    .await
    .expect("Buy quote failed");

    let buy_response = match buy_result {
        axum::Json(res) => res,
    };

    let buy_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&buy_response.transaction)
        .expect("Failed to decode buy transaction");

    let (buy_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&buy_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize buy transaction");

    // 3. Perform Sell Quote (RAY -> SOL) with half the output amount
    let buy_output_amount: u64 = buy_response.output_amount;
    let sell_input_amount = buy_output_amount / 2;

    let sell_request = QuoteRequest {
        input_token: output_mint.to_string(), // RAY
        output_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 100,
    };

    println!(
        "Requesting Sell Quote for {} RAY -> SOL...",
        sell_input_amount
    );
    let sell_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(sell_request),
    )
    .await
    .expect("Sell quote failed");

    let sell_response = match sell_result {
        axum::Json(res) => res,
    };

    let sell_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sell_response.transaction)
        .expect("Failed to decode sell transaction");

    let (sell_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&sell_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize sell transaction");

    // 4. Combine transactions
    let recent_blockhash = buy_transaction.message.recent_blockhash;

    let get_instructions = |tx: &Transaction| -> Vec<Instruction> {
        let message = &tx.message;
        message
            .instructions
            .iter()
            .map(|ix| Instruction {
                program_id: message.account_keys[ix.program_id_index as usize],
                accounts: ix
                    .accounts
                    .iter()
                    .map(|&acc_idx| {
                        let idx = acc_idx as usize;
                        let is_signer = idx < message.header.num_required_signatures as usize;
                        let is_writable = if is_signer {
                            idx < (message.header.num_required_signatures
                                - message.header.num_readonly_signed_accounts)
                                as usize
                        } else {
                            idx < (message.account_keys.len()
                                - message.header.num_readonly_unsigned_accounts as usize)
                        };

                        solana_sdk::instruction::AccountMeta {
                            pubkey: message.account_keys[idx],
                            is_signer,
                            is_writable,
                        }
                    })
                    .collect(),
                data: ix.data.clone(),
            })
            .collect::<Vec<_>>()
    };

    let mut instructions = get_instructions(&buy_transaction);
    let sell_instructions = get_instructions(&sell_transaction);

    instructions.extend(sell_instructions);

    println!("Simulating Composite Transaction...");
    use solana_client::rpc_config::RpcSimulateTransactionConfig;
    use solana_commitment_config::CommitmentConfig;

    let sim_config = RpcSimulateTransactionConfig {
        sig_verify: false,
        commitment: Some(CommitmentConfig::processed()),
        ..RpcSimulateTransactionConfig::default()
    };

    let mut transaction = Transaction::new_with_payer(&instructions, Some(&payer));
    transaction.message.recent_blockhash = recent_blockhash;

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, sim_config)
        .await;

    match result {
        Ok(sim_result) => {
            if let Some(logs) = &sim_result.value.logs {
                for log in logs {
                    println!("  {}", log);
                }
            }
            if let Some(err) = sim_result.value.err {
                panic!("Simulation failed: {:?}", err);
            }
            let units = sim_result.value.units_consumed.unwrap_or(0);
            println!(
                "✅ Raydium CLMM Composite Simulation Successful! Consumed {} units",
                units
            );
            assert!(units > 0);
        }
        Err(e) => panic!("RPC Error: {}", e),
    }
}
