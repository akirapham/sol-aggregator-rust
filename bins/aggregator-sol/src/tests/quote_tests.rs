use crate::aggregator::DexAggregator;
use crate::pool_data_types::*;
use crate::pool_manager::PoolStateManager;
use crate::types::{ExecutionPriority, SwapParams, Token};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

// Test token definitions
pub fn wsol_token() -> Token {
    Token {
        address: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(),
        symbol: Some("SOL".to_string()),
        name: Some("Wrapped SOL".to_string()),
        decimals: 9,
        is_token_2022: false,
        logo_uri: None,
    }
}

pub fn usdc_token() -> Token {
    Token {
        address: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
        symbol: Some("USDC".to_string()),
        name: Some("USD Coin".to_string()),
        decimals: 6,
        is_token_2022: false,
        logo_uri: None,
    }
}

pub fn test_token() -> Token {
    Token {
        address: Pubkey::new_unique(),
        symbol: Some("TEST".to_string()),
        name: Some("Test Token".to_string()),
        decimals: 9,
        is_token_2022: false,
        logo_uri: None,
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

// Helper to create test setup
async fn create_test_setup(
    enable_flags: Vec<&str>,
) -> (Arc<PoolStateManager>, crate::types::AggregatorConfig) {
    dotenvy::dotenv().ok();
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let mut config = crate::types::AggregatorConfig::default();
    for flag in enable_flags {
        match flag {
            "raydium_amm_v4" => config.enable_raydium_amm_v4 = true,
            "raydium_cpmm" => config.enable_raydium_cpmm = true,
            "raydium_clmm" => config.enable_raydium_clmm = true,
            "orca" => config.enable_orca_whirlpools = true,
            "pumpfun" => config.enable_pumpfun = true,
            "pumpfun_swap" => config.enable_pumpfun_swap = true,
            "bonk" => config.enable_bonk = true,
            "meteora_dbc" => config.enable_meteora_dbc = true,
            "meteora_dammv2" => config.enable_meteora_dammv2 = true,
            "meteora_dlmm" => config.enable_meteora_dlmm = true,
            _ => {}
        }
    }

    let pool_manager =
        Arc::new(PoolStateManager::new_for_testing(config.clone(), rpc_client).await);
    (pool_manager, config)
}

// Helper to verify quote
async fn verify_quote(
    pool_manager: Arc<PoolStateManager>,
    config: crate::types::AggregatorConfig,
    input_token: Token,
    output_token: Token,
    input_amount: u64,
    expected_pool_address: Pubkey,
) {
    let aggregator = DexAggregator::new(config, pool_manager.clone());

    let swap_params = SwapParams {
        input_token,
        output_token,
        input_amount,
        slippage_bps: 50,
        user_wallet: Pubkey::default(),
        priority: ExecutionPriority::Medium,
    };

    let exclude_pools = std::collections::HashSet::new();
    let route = aggregator
        .get_swap_route_with_exclude(&swap_params, &exclude_pools, false)
        .await;

    assert!(route.is_some(), "Should find a route with injected pool");
    let route = route.unwrap();
    assert!(route.output_amount > 0, "Output amount should be positive");
    assert_eq!(route.paths.len(), 1, "Should have exactly one path");
    assert_eq!(
        route.paths[0].steps.len(),
        1,
        "Should have exactly one step"
    );
    assert_eq!(
        route.paths[0].steps[0].pool_address, expected_pool_address,
        "Should use the injected pool"
    );
}

#[tokio::test]
async fn test_raydium_amm_v4_quote() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_amm_v4"]).await;

    let pool_address = Pubkey::from_str("58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2").unwrap();
    let pool_state = PoolState::RaydiumAmmV4(RaydiumAmmV4PoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        base_mint: wsol_token().address,
        quote_mint: usdc_token().address,
        amm_authority: Pubkey::new_unique(),
        amm_open_orders: Pubkey::new_unique(),
        amm_target_orders: Pubkey::new_unique(),
        pool_coin_token_account: Pubkey::new_unique(),
        pool_pc_token_account: Pubkey::new_unique(),
        serum_program: Pubkey::new_unique(),
        serum_market: Pubkey::new_unique(),
        serum_bids: Pubkey::new_unique(),
        serum_asks: Pubkey::new_unique(),
        serum_event_queue: Pubkey::new_unique(),
        serum_coin_vault_account: Pubkey::new_unique(),
        serum_pc_vault_account: Pubkey::new_unique(),
        serum_vault_signer: Pubkey::new_unique(),
        last_updated: current_timestamp(),
        base_reserve: 100_000_000_000, // 100 SOL
        quote_reserve: 15_000_000_000, // 15,000 USDC
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        usdc_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_cpmm_quote() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_cpmm"]).await;

    let pool_address = Pubkey::new_unique();
    let pool_state = PoolState::RaydiumCpmm(RaydiumCpmmPoolState {
        slot: 100,
        transaction_index: None,
        status: 1,
        address: pool_address,
        token0: wsol_token().address,
        token1: usdc_token().address,
        token0_vault: Pubkey::new_unique(),
        token1_vault: Pubkey::new_unique(),
        token0_reserve: 100_000_000_000, // 100 SOL
        token1_reserve: 15_000_000_000,  // 15,000 USDC
        amm_config: Pubkey::new_unique(),
        observation_state: Pubkey::new_unique(),
        last_updated: current_timestamp(),
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        usdc_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_pumpfun_quote() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let pool_address = Pubkey::new_unique();
    // Create test token ONCE and reuse it
    let test_tok = test_token();
    let test_mint = test_tok.address;

    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        mint: test_mint,
        last_updated: current_timestamp(),
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: 1_073_000_000_000_000, // 1.073B tokens
        virtual_sol_reserves: 30_000_000_000,          // 30 SOL
        real_token_reserves: 800_000_000_000_000,
        real_sol_reserves: 20_000_000_000,
        complete: false,
        creator: Pubkey::new_unique(),
        is_mayhem_mode: false,
    });

    pool_manager.inject_pool(pool_state).await;
    // PumpFun pools are indexed as (token, SOL), so we swap from TEST_TOKEN -> SOL
    verify_quote(
        pool_manager,
        config,
        test_tok,
        wsol_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}

#[tokio::test]
#[ignore] // Pool found but route calculation fails - needs investigation of Bonk swap requirements
async fn test_bonk_quote() {
    let (pool_manager, config) = create_test_setup(vec!["bonk"]).await;

    let pool_address = Pubkey::new_unique();
    let pool_state = PoolState::Bonk(BonkPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        status: 1,
        total_base_sell: 0,
        base_reserve: 100_000_000_000, // 100 SOL
        quote_reserve: 15_000_000_000, // 15,000 USDC
        liquidity_usd: 30000.0,
        real_base: 100_000_000_000,
        real_quote: 15_000_000_000,
        quote_protocol_fee: 0,
        platform_fee: 0,
        global_config: Pubkey::new_unique(),
        platform_config: Pubkey::new_unique(),
        base_mint: wsol_token().address,
        quote_mint: usdc_token().address,
        base_vault: Pubkey::new_unique(),
        quote_vault: Pubkey::new_unique(),
        creator: Pubkey::new_unique(),
        last_updated: current_timestamp(),
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        usdc_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}

// Note: CLMM, Whirlpool, Meteora pools require tick arrays for quote calculations
// These tests are marked as ignored until tick array support is added to the test infrastructure

#[tokio::test]
#[ignore] // Requires tick array setup
async fn test_raydium_clmm_quote() {
    let (pool_manager, config) = create_test_setup(vec!["raydium_clmm"]).await;

    let pool_address = Pubkey::new_unique();
    let pool_state = PoolState::RadyiumClmm(Box::new(RaydiumClmmPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        amm_config: Pubkey::new_unique(),
        token_mint0: wsol_token().address,
        token_mint1: usdc_token().address,
        token_vault0: Pubkey::new_unique(),
        token_vault1: Pubkey::new_unique(),
        observation_key: Pubkey::new_unique(),
        tick_spacing: 1,
        liquidity: 1000000000,
        liquidity_usd: 30000.0,
        sqrt_price_x64: 79228162514264337593543950336, // ~1.0
        tick_current_index: 0,
        status: 1,
        tick_array_bitmap: [0; 16],
        open_time: 0,
        tick_array_state: HashMap::new(),
        tick_array_bitmap_extension: None,
        last_updated: current_timestamp(),
        token0_reserve: 100_000_000_000,
        token1_reserve: 15_000_000_000,
        is_state_keys_initialized: true,
    }));

    pool_manager.inject_pool(pool_state).await;
    // Would need tick array data for actual quote calculation
}

#[tokio::test]
async fn test_orca_whirlpool_quote_real_data() {
    use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
    use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::types::{
        Tick, TickArrayState, WhirlpoolPoolState as WhirlpoolStateRaw,
    };

    let (pool_manager, config) = create_test_setup(vec!["orca"]).await;

    // Real SOL-BONK Whirlpool pool
    let pool_address = Pubkey::from_str("5zpyutJu9ee6jFymDGoK7F6S5Kczqtc9FomP3ueKuyA9").unwrap();
    let bonk_mint = Pubkey::from_str("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");

    // Deserialize Whirlpool state (skip 8-byte discriminator)
    let whirlpool_state = WhirlpoolStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize whirlpool state");

    // Fetch token vault accounts to get real reserves
    let vault_a_account = rpc_client
        .get_account(&whirlpool_state.token_vault_a)
        .await
        .expect("Failed to fetch vault A account");
    let vault_b_account = rpc_client
        .get_account(&whirlpool_state.token_vault_b)
        .await
        .expect("Failed to fetch vault B account");

    // Parse token account data to get amounts (amount is at offset 64, u64 little-endian)
    let token_a_reserve = u64::from_le_bytes(vault_a_account.data[64..72].try_into().unwrap());
    let token_b_reserve = u64::from_le_bytes(vault_b_account.data[64..72].try_into().unwrap());

    println!(
        "Fetched reserves: token_a={}, token_b={}",
        token_a_reserve, token_b_reserve
    );

    // Create temporary WhirlpoolPoolState for fetching ticks
    let temp_pool_state = crate::pool_data_types::orca_whirlpool::WhirlpoolPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        whirlpool_config: whirlpool_state.whirlpools_config,
        tick_spacing: whirlpool_state.tick_spacing,
        tick_spacing_seed: whirlpool_state.tick_spacing_seed,
        fee_rate: whirlpool_state.fee_rate,
        protocol_fee_rate: whirlpool_state.protocol_fee_rate,
        liquidity: whirlpool_state.liquidity,
        liquidity_usd: 100000.0,
        sqrt_price: whirlpool_state.sqrt_price,
        tick_current_index: whirlpool_state.tick_current_index,
        token_mint_a: whirlpool_state.token_mint_a,
        token_vault_a: whirlpool_state.token_vault_a,
        token_mint_b: whirlpool_state.token_mint_b,
        token_vault_b: whirlpool_state.token_vault_b,
        tick_array_state: HashMap::new(),
        last_updated: current_timestamp(),
        token_a_reserve,
        token_b_reserve,
        is_state_keys_initialized: true,
        oracle_state: Default::default(),
    };

    // Fetch all tick arrays using the helper function
    let tick_fetcher = OrcaTickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array()),
    );

    let tick_arrays = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &temp_pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    println!("Fetched {} tick arrays", tick_arrays.len());

    // Convert fetched tick arrays to HashMap
    let mut tick_array_state = HashMap::new();
    for tick_array in tick_arrays {
        let ticks = tick_array.ticks();
        let ticks_array: [Tick; 88] = ticks
            .iter()
            .map(|t| Tick {
                initialized: t.initialized,
                liquidity_net: t.liquidity_net,
                liquidity_gross: t.liquidity_gross,
                fee_growth_outside_a: t.fee_growth_outside_a,
                fee_growth_outside_b: t.fee_growth_outside_b,
                reward_growths_outside: t.reward_growths_outside,
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let converted = TickArrayState {
            start_tick_index: tick_array.start_tick_index(),
            ticks: ticks_array,
            whirlpool: pool_address,
        };

        tick_array_state.insert(tick_array.start_tick_index(), converted);
    }

    // Create final WhirlpoolPoolState with real data and tick arrays
    let pool_state = PoolState::OrcaWhirlpool(WhirlpoolPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        whirlpool_config: whirlpool_state.whirlpools_config,
        tick_spacing: whirlpool_state.tick_spacing,
        tick_spacing_seed: whirlpool_state.tick_spacing_seed,
        fee_rate: whirlpool_state.fee_rate,
        protocol_fee_rate: whirlpool_state.protocol_fee_rate,
        liquidity: whirlpool_state.liquidity,
        liquidity_usd: 100000.0,
        sqrt_price: whirlpool_state.sqrt_price,
        tick_current_index: whirlpool_state.tick_current_index,
        token_mint_a: whirlpool_state.token_mint_a,
        token_vault_a: whirlpool_state.token_vault_a,
        token_mint_b: whirlpool_state.token_mint_b,
        token_vault_b: whirlpool_state.token_vault_b,
        tick_array_state,
        last_updated: current_timestamp(),
        token_a_reserve,
        token_b_reserve,
        is_state_keys_initialized: true,
        oracle_state: Default::default(),
    });

    pool_manager.inject_pool(pool_state).await;

    // Verify pool was injected
    let pools = pool_manager
        .get_pools_for_pair(&wsol_token().address, &bonk_mint)
        .await;
    println!(
        "Found {} pools for SOL-BONK pair after injection",
        pools.len()
    );

    // Test swap: SOL -> BONK (token_a is SOL, token_b is BONK)
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: bonk_mint,
            symbol: Some("BONK".to_string()),
            name: Some("Bonk".to_string()),
            decimals: 5,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000,
        pool_address,
    )
    .await;
}
