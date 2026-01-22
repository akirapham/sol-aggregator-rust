use crate::aggregator::DexAggregator;
use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
use crate::pool_data_types::*;
use crate::pool_manager::PoolStateManager;
use crate::tests::quotes::common::*;
use crate::types::Token;
use borsh::BorshDeserialize;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::common::current_timestamp;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::types::{
    Tick, TickArrayState, WhirlpoolPoolState as WhirlpoolStateRaw,
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::test]
async fn test_orca_whirlpool_quote() {
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
        last_updated: u64::MAX,
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
        last_updated: u64::MAX,
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
            name: Some("Bonk Token".to_string()),
            decimals: 5,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_orca_whirlpool_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["orca"]).await;
    let pool_address = Pubkey::from_str("5zpyutJu9ee6jFymDGoK7F6S5Kczqtc9FomP3ueKuyA9").unwrap();
    let bonk_mint = Pubkey::from_str("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");
    let whirlpool_state = WhirlpoolStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize whirlpool state");

    // Fetch reserves
    let vault_a_account = rpc_client
        .get_account(&whirlpool_state.token_vault_a)
        .await
        .expect("Failed to fetch vault A");
    let vault_b_account = rpc_client
        .get_account(&whirlpool_state.token_vault_b)
        .await
        .expect("Failed to fetch vault B");
    let token_a_reserve = u64::from_le_bytes(vault_a_account.data[64..72].try_into().unwrap());
    let token_b_reserve = u64::from_le_bytes(vault_b_account.data[64..72].try_into().unwrap());

    // Fetch tick arrays
    let program_id = Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array());
    let fetcher = OrcaTickArrayFetcher::new(rpc_client.clone(), program_id);

    let tick_spacing = whirlpool_state.tick_spacing as i32;
    let current_tick = whirlpool_state.tick_current_index;
    let start_tick = (current_tick / (tick_spacing * 88)) * (tick_spacing * 88);

    let mut tick_array_addresses = vec![];
    for offset in -2..=2 {
        let tick_index = start_tick + (offset * tick_spacing * 88);
        if let Ok((pda, _)) = fetcher.derive_tick_array_pda(&pool_address, tick_index) {
            tick_array_addresses.push(pda);
        }
    }

    let tick_arrays = fetcher
        .fetch_multiple_tick_arrays(tick_array_addresses)
        .await
        .unwrap_or_default();

    let mut tick_array_state = std::collections::HashMap::new();
    for tick_array in tick_arrays {
        let ticks_array: [Tick; 88] = tick_array
            .ticks()
            .into_iter()
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
        liquidity_usd: 1_000_000.0,
        sqrt_price: whirlpool_state.sqrt_price,
        tick_current_index: whirlpool_state.tick_current_index,
        token_mint_a: whirlpool_state.token_mint_a,
        token_vault_a: whirlpool_state.token_vault_a,
        token_mint_b: whirlpool_state.token_mint_b,
        token_vault_b: whirlpool_state.token_vault_b,
        tick_array_state,
        last_updated: u64::MAX,
        token_a_reserve,
        token_b_reserve,
        is_state_keys_initialized: true,
        oracle_state: Default::default(),
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: BONK -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: bonk_mint,
            symbol: Some("BONK".to_string()),
            name: Some("Bonk".to_string()),
            decimals: 5,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        1_000_000_000,
        pool_address,
    )
    .await;
}
