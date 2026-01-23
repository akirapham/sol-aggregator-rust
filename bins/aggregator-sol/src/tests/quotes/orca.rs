use crate::aggregator::DexAggregator;
use crate::api::dto::QuoteRequest;
use crate::api::AppState;
use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
use crate::pool_data_types::*;
use crate::tests::quotes::common::*;
use crate::types::Token;
use base64::Engine;
use borsh::BorshDeserialize;
use solana_client::rpc_config::{CommitmentConfig, RpcSimulateTransactionConfig};
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::types::{
    OracleState, Tick, TickArrayState, WhirlpoolPoolState as WhirlpoolStateRaw,
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

    // Test Round Trip: SOL -> BONK -> SOL
    verify_quote_round_trip(
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
        100, // 1% tolerance
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

    // Test Round Trip: BONK -> SOL -> BONK
    verify_quote_round_trip(
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
        100, // 1% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_orca_whirlpool_quote_simulation() {
    let (pool_manager, config) = create_test_setup(vec!["orca"]).await;

    // Real SOL-BONK Whirlpool pool
    let pool_address = Pubkey::from_str("5zpyutJu9ee6jFymDGoK7F6S5Kczqtc9FomP3ueKuyA9").unwrap();
    let bonk_mint = Pubkey::from_str("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");

    let whirlpool_state = WhirlpoolStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize whirlpool state");

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

    let tick_arrays = fetcher
        .fetch_all_tick_arrays(pool_address, &temp_pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    let mut tick_array_state = HashMap::new();
    for tick_array in tick_arrays {
        let ticks_array: [Tick; 88] = tick_array
            .ticks()
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
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: bonk_mint,
            symbol: Some("BONK".to_string()),
            name: Some("Bonk".to_string()),
            decimals: 5,
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

    // Construct Quote Request: SOL -> BONK
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let input_amount = 100_000_000; // 0.1 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: bonk_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 100,
    };

    println!("Calling get_quote handler...");
    let result =
        crate::api::handlers::get_quote(axum::extract::State(state), axum::extract::Query(request))
            .await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Quote Response!");
            println!("Routes: {}", response.routes.len());
            println!("Output Amount: {}", response.output_amount);
            println!("Transaction Base64: {}", response.transaction);

            let tx_bytes = base64::engine::general_purpose::STANDARD
                .decode(&response.transaction)
                .expect("Failed to decode base64 transaction");

            let (transaction, _): (Transaction, usize) =
                bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
                    .expect("Failed to deserialize transaction");

            // Simulate Transaction
            println!("Simulating transaction on-chain...");
            let simulation = rpc_client.simulate_transaction(&transaction).await;

            match simulation {
                Ok(sim_result) => {
                    println!("Simulation Result: {:#?}", sim_result);
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
                Err(e) => {
                    eprintln!("RPC Error during simulation: {}", e);
                    panic!("RPC call failed");
                }
            }
        }
        Err((status, axum::Json(error_res))) => {
            panic!("Quote request failed: {} - {}", status, error_res.error);
        }
    }
}

#[tokio::test]
async fn test_orca_whirlpool_quote_simulation_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["orca"]).await;

    // Real SOL-BONK Whirlpool pool
    let pool_address = Pubkey::from_str("5zpyutJu9ee6jFymDGoK7F6S5Kczqtc9FomP3ueKuyA9").unwrap();
    let bonk_mint = Pubkey::from_str("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
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

    let token_a_reserve = u64::from_le_bytes(vault_a_account.data[64..72].try_into().unwrap());
    let token_b_reserve = u64::from_le_bytes(vault_b_account.data[64..72].try_into().unwrap());

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

    let tick_fetcher = OrcaTickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array()),
    );

    let tick_arrays = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &temp_pool_state)
        .await
        .expect("Failed to fetch tick arrays");

    let mut tick_array_state = HashMap::new();
    for tick_array in tick_arrays {
        let ticks_array: [Tick; 88] = tick_array
            .ticks()
            .iter()
            .map(|t| Tick {
                initialized: t.initialized,
                liquidity_net: t.liquidity_net as i128,
                liquidity_gross: t.liquidity_gross as u128,
                fee_growth_outside_a: t.fee_growth_outside_a as u128,
                fee_growth_outside_b: t.fee_growth_outside_b as u128,
                reward_growths_outside: [
                    t.reward_growths_outside[0] as u128,
                    t.reward_growths_outside[1] as u128,
                    t.reward_growths_outside[2] as u128,
                ],
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        tick_array_state.insert(
            tick_array.start_tick_index(),
            TickArrayState {
                start_tick_index: tick_array.start_tick_index(),
                ticks: ticks_array,
                whirlpool: pool_address,
            },
        );
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
        oracle_state: OracleState::default(),
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: bonk_mint,
            symbol: Some("BONK".to_string()),
            name: Some("Bonk".to_string()),
            decimals: 5,
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
        output_token: bonk_mint.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: buy_input_amount,
        slippage_bps: 100,
    };

    println!("Getting Buy Quote...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::extract::Query(buy_request),
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
        input_token: bonk_mint.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 100,
    };

    let sell_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::extract::Query(sell_request),
    )
    .await
    .expect("Sell request failed");

    let sell_response = match sell_result {
        axum::Json(res) => res,
    };

    println!("Sell Quote Output: {}", sell_response.output_amount);

    // Verify Round Trip (approx 50% of initial input)
    // Buy Input: 100_000_000 SOL
    // Sell Input: 50% of tokens from Buy
    // Expected Return: approx 50_000_000 SOL
    let expected_return = buy_input_amount / 2;
    let actual_return = sell_response.output_amount;
    let diff = if actual_return > expected_return {
        actual_return - expected_return
    } else {
        expected_return - actual_return
    };

    // Tolerance 1% (Orca Whirlpool is concentrated liquidity, low slippage)
    let max_diff = expected_return * 1 / 100;
    assert!(
        diff <= max_diff,
        "Orca Simulation Reverse Verification Failed: Expected ~{}, Got {}, Diff {}",
        expected_return,
        actual_return,
        diff
    );
    println!("✅ Orca Round Trip Verification Passed (Diff: {})", diff);

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
                "✅ Composite Orca Whirlpool Simulation Successful! Consumed {} compute units",
                units_consumed
            );
        }
        Err(e) => panic!("RPC Error during Composite simulation: {}", e),
    }
}
