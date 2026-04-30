use crate::aggregator::DexAggregator;
use crate::api::dto::QuoteRequest;
use crate::api::AppState;
use crate::pool_data_types::pumpf::functions::get_bonding_curve_pda;
use crate::pool_data_types::*;
use crate::tests::quotes::common::*;
use crate::types::Token;
use base64::Engine;
use borsh::BorshDeserialize;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpfun::types::BondingCurve;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::types::{
    Pool as PumpSwapPoolRaw, POOL_SIZE,
};
use std::str::FromStr;
use std::sync::Arc;

#[tokio::test]
async fn test_pumpfun_quote_simulation() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    // Real Data
    let token_mint_address =
        Pubkey::from_str("7JnSRL1kFKBhQKW4B4BusYcTq5mXPtjnC5pnqqLFpump").unwrap();
    // derive bonding curve from token mint
    let bonding_curve_address = get_bonding_curve_pda(&token_mint_address).unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    // Debug: Check Mint Owner
    let mint_account = rpc_client
        .get_account(&token_mint_address)
        .await
        .expect("Failed to fetch Mint account");
    println!("Mint Owner: {:?}", mint_account.owner);

    // Fetch real bonding curve state
    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurve::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    println!("Fetched Real Bonding Curve State: {:#?}", raw_state);

    // Create PoolState from real data
    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: u64::MAX,
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
        is_cashback: raw_state.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("PUMP".to_string()),
            name: Some("Pump Token".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        })
        .await;

    // Create AppState
    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    // Construct Quote Request with REAL addresses and REAL wallet
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let input_amount = 100_000_000; // 0.1 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: token_mint_address.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 500,
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
                    assert!(
                        sim_result.value.err.is_none(),
                        "Simulation should succeed. Error: {:?}",
                        sim_result.value.err
                    );
                    let units_consumed = sim_result
                        .value
                        .units_consumed
                        .expect("Should have units consumed");
                    assert!(units_consumed > 0, "Should consume compute units");
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
async fn test_pumpfun_quote_hydrates_missing_token_and_pool() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let token_mint_address =
        Pubkey::from_str("7JnSRL1kFKBhQKW4B4BusYcTq5mXPtjnC5pnqqLFpump").unwrap();
    let bonding_curve_address = get_bonding_curve_pda(&token_mint_address).unwrap();

    pool_manager.inject_token(wsol_token()).await;

    assert!(pool_manager.get_token(&token_mint_address).await.is_none());
    assert!(pool_manager.get_pool(&bonding_curve_address).is_none());

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client,
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    let request = QuoteRequest {
        input_token: wsol_token().address.to_string(),
        output_token: token_mint_address.to_string(),
        user_wallet: "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string(),
        input_amount: 100_000_000,
        slippage_bps: 500,
    };

    let response =
        crate::api::handlers::get_quote(axum::extract::State(state), axum::extract::Query(request))
            .await
            .expect("Quote should hydrate missing PumpFun token/pool");

    assert!(response.0.output_amount > 0);
    assert!(pool_manager.get_token(&token_mint_address).await.is_some());
    assert!(pool_manager.get_pool(&bonding_curve_address).is_some());
}

#[tokio::test]
async fn test_pumpfun_quote_hydrates_cashback_sell_token_and_pool() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let token_mint_address =
        Pubkey::from_str("J7xppwhH1WBzjthJsbvPi9bZ5bVUnSdzhoMwSLRGPsMW").unwrap();
    let bonding_curve_address = get_bonding_curve_pda(&token_mint_address).unwrap();

    pool_manager.inject_token(wsol_token()).await;

    assert!(pool_manager.get_token(&token_mint_address).await.is_none());
    assert!(pool_manager.get_pool(&bonding_curve_address).is_none());

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client,
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    let request = QuoteRequest {
        input_token: token_mint_address.to_string(),
        output_token: wsol_token().address.to_string(),
        user_wallet: "HCp3JTAW85o9pCKqP7enJRiKWaVrbufVQRzaXAi3Du6X".to_string(),
        input_amount: 84_895_713_871,
        slippage_bps: 1000,
    };

    let response =
        crate::api::handlers::get_quote(axum::extract::State(state), axum::extract::Query(request))
            .await
            .expect("Quote should hydrate missing cashback PumpFun token/pool");

    assert!(response.0.output_amount > 0);
    assert!(pool_manager.get_token(&token_mint_address).await.is_some());
    assert!(pool_manager.get_pool(&bonding_curve_address).is_some());
}

#[tokio::test]
async fn test_pumpfun_quote_repairs_cached_pool_with_missing_mint() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let token_mint_address =
        Pubkey::from_str("J7xppwhH1WBzjthJsbvPi9bZ5bVUnSdzhoMwSLRGPsMW").unwrap();
    let bonding_curve_address = get_bonding_curve_pda(&token_mint_address).unwrap();

    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("Predator".to_string()),
            name: Some("Super Predator".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        })
        .await;
    pool_manager
        .inject_pool(PoolState::Pumpfun(PumpfunPoolState {
            slot: 0,
            transaction_index: None,
            address: bonding_curve_address,
            mint: Pubkey::default(),
            last_updated: u64::MAX,
            liquidity_usd: 1.0,
            is_state_keys_initialized: true,
            virtual_token_reserves: 1,
            virtual_sol_reserves: 1,
            real_token_reserves: 1,
            real_sol_reserves: 1,
            complete: false,
            creator: Pubkey::default(),
            is_mayhem_mode: false,
            is_cashback: false,
        }))
        .await;

    assert!(pool_manager.get_pool(&bonding_curve_address).is_some());
    assert_eq!(
        pool_manager.get_pool_count_for_pair(&token_mint_address, &wsol_token().address),
        0
    );

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client,
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    let request = QuoteRequest {
        input_token: token_mint_address.to_string(),
        output_token: wsol_token().address.to_string(),
        user_wallet: "HCp3JTAW85o9pCKqP7enJRiKWaVrbufVQRzaXAi3Du6X".to_string(),
        input_amount: 84_895_713_871,
        slippage_bps: 1000,
    };

    let response =
        crate::api::handlers::get_quote(axum::extract::State(state), axum::extract::Query(request))
            .await
            .expect("Quote should repair cached PumpFun pool with missing mint");

    assert!(response.0.output_amount > 0);
    assert_eq!(
        pool_manager.get_pool_count_for_pair(&token_mint_address, &wsol_token().address),
        1
    );
    match pool_manager.get_pool(&bonding_curve_address).unwrap() {
        PoolState::Pumpfun(pool) => {
            assert_eq!(pool.mint, token_mint_address);
            assert!(pool.is_cashback);
        }
        _ => panic!("Expected PumpFun pool"),
    }
}

#[tokio::test]
async fn test_pumpfun_quote_hydrates_missing_token_and_pool_simulation() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let token_mint_address =
        Pubkey::from_str("7JnSRL1kFKBhQKW4B4BusYcTq5mXPtjnC5pnqqLFpump").unwrap();
    let bonding_curve_address = get_bonding_curve_pda(&token_mint_address).unwrap();

    pool_manager.inject_token(wsol_token()).await;

    assert!(pool_manager.get_token(&token_mint_address).await.is_none());
    assert!(pool_manager.get_pool(&bonding_curve_address).is_none());

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));
    let state = Arc::new(AppState {
        aggregator,
        rpc_client: rpc_client.clone(),
        arbitrage_config: None,
        arbitrage_monitor: None,
    });

    let request = QuoteRequest {
        input_token: wsol_token().address.to_string(),
        output_token: token_mint_address.to_string(),
        user_wallet: "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string(),
        input_amount: 100_000_000,
        slippage_bps: 500,
    };

    let response =
        crate::api::handlers::get_quote(axum::extract::State(state), axum::extract::Query(request))
            .await
            .expect("Quote should hydrate and simulate missing PumpFun token/pool")
            .0;

    assert!(response.output_amount > 0);
    assert!(pool_manager.get_token(&token_mint_address).await.is_some());
    assert!(pool_manager.get_pool(&bonding_curve_address).is_some());

    let tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&response.transaction)
        .expect("Failed to decode base64 transaction");

    let (transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize transaction");

    let simulation = rpc_client
        .simulate_transaction(&transaction)
        .await
        .expect("Simulation RPC call failed");

    assert!(
        simulation.value.err.is_none(),
        "Simulation should succeed. Error: {:?}",
        simulation.value.err
    );
    assert!(
        simulation.value.units_consumed.unwrap_or_default() > 0,
        "Should consume compute units"
    );
}

#[tokio::test]
async fn test_pumpfun_quote() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let bonding_curve_address =
        Pubkey::from_str("9Exw9tyEYPEv5wz7WsUZZVQEG262csVyHEKYcgNeEaf1").unwrap();
    let token_mint_address =
        Pubkey::from_str("BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurve::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: u64::MAX,
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
        is_cashback: raw_state.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: SOL -> PUMP -> SOL
    verify_quote_round_trip(
        pool_manager,
        config,
        Token {
            address: token_mint_address,
            symbol: Some("PUMP".to_string()),
            name: Some("Pump Token".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        },
        wsol_token(),
        10_000_000,
        bonding_curve_address,
        600, // 4% tolerance (1% fee x2 + slippage)
    )
    .await;
}

#[tokio::test]
async fn test_pumpswap_quote() {
    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;

    let pool_address = Pubkey::from_str("4w2cysotX6czaUGmmWg13hDpY4QEMG2CzeKYEQyK9Ama").unwrap();
    let token_mint = Pubkey::from_str("5UUH9RTDiSpq6HKS6bp4NdU9PNJpXRXuiw6ShBTBhgH2").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");

    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    let vault_base_account = rpc_client
        .get_account(&pumpswap_pool.pool_base_token_account)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&pumpswap_pool.pool_quote_token_account)
        .await
        .expect("Failed to fetch quote vault");

    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::PumpSwap(PumpSwapPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        index: pumpswap_pool.index,
        creator: Some(pumpswap_pool.creator),
        base_mint: pumpswap_pool.base_mint,
        quote_mint: pumpswap_pool.quote_mint,
        pool_base_token_account: pumpswap_pool.pool_base_token_account,
        pool_quote_token_account: pumpswap_pool.pool_quote_token_account,
        last_updated: u64::MAX,
        base_reserve,
        quote_reserve,
        liquidity_usd: (quote_reserve as f64 * 2.0) / 1e9 * 200.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
        is_cashback: pumpswap_pool.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: SOL -> Token -> SOL
    verify_quote_round_trip(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        10_000_000,
        pool_address,
        400, // 4% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_pumpfun_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let bonding_curve_address =
        Pubkey::from_str("9Exw9tyEYPEv5wz7WsUZZVQEG262csVyHEKYcgNeEaf1").unwrap();
    let token_mint_address =
        Pubkey::from_str("BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurve::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: u64::MAX,
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
        is_cashback: raw_state.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: PUMP -> SOL -> PUMP
    verify_quote_round_trip(
        pool_manager,
        config,
        Token {
            address: token_mint_address,
            symbol: Some("PUMP".to_string()),
            name: Some("Pump Token".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        },
        wsol_token(),
        10_000_000,
        bonding_curve_address,
        400, // 4% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_pumpswap_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;
    let pool_address = Pubkey::from_str("F3g7TCcqpQHuFzDcn9xXhCTgrxzuRANnYX9jkzWGpJrZ").unwrap();
    let token_mint = Pubkey::from_str("3few1wmJAtaFLd4mwT9e7gaaTuccnn5BakUTJSz9pump").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");
    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    let vault_base_account = rpc_client
        .get_account(&pumpswap_pool.pool_base_token_account)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&pumpswap_pool.pool_quote_token_account)
        .await
        .expect("Failed to fetch quote vault");
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::PumpSwap(PumpSwapPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        index: pumpswap_pool.index,
        creator: Some(pumpswap_pool.creator),
        base_mint: pumpswap_pool.base_mint,
        quote_mint: pumpswap_pool.quote_mint,
        pool_base_token_account: pumpswap_pool.pool_base_token_account,
        pool_quote_token_account: pumpswap_pool.pool_quote_token_account,
        last_updated: u64::MAX,
        base_reserve,
        quote_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
        is_cashback: pumpswap_pool.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        })
        .await;

    // Test Round Trip: Token -> SOL -> Token
    verify_quote_round_trip(
        pool_manager,
        config,
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        },
        wsol_token(),
        10_000_000,
        pool_address,
        400, // 4% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_pumpfun_quote_simulation_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let token_mint_address =
        Pubkey::from_str("7JnSRL1kFKBhQKW4B4BusYcTq5mXPtjnC5pnqqLFpump").unwrap();
    let bonding_curve_address = get_bonding_curve_pda(&token_mint_address).unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurve::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");
    println!("Raw state: {:#?}", raw_state);

    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: u64::MAX,
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
        is_cashback: raw_state.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("PUMP".to_string()),
            name: Some("Pump Token".to_string()),
            decimals: 6,
            is_token_2022: true,
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

    // Composite Simulation: Buy -> Sell (Atomic)
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    // 1. Get BUY Quote (SOL -> Token)
    let buy_input_amount = 100_000_000;
    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(),
        output_token: token_mint_address.to_string(),
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

    // 2. Get SELL Quote (Token -> SOL)
    let buy_output_amount: u64 = buy_response.output_amount;
    let sell_input_amount = buy_output_amount / 2;

    println!("Getting Sell Quote (Amount: {})...", sell_input_amount);
    let sell_request = QuoteRequest {
        input_token: token_mint_address.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(),
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

    // Verify Round Trip (should regain approx 50% of input value)
    // Initial Buy Input: 100_000_000 SOL
    // Sell Input: 50% of Buy Output (approx 50M SOL worth of Tokens)
    // Expected Output: approx 50_000_000 SOL
    let expected_return = buy_input_amount / 2;
    let actual_return = sell_response.output_amount;
    let diff = if actual_return > expected_return {
        actual_return - expected_return
    } else {
        expected_return - actual_return
    };

    // Tolerance 4% (PumpFun)
    let max_diff = expected_return * 4 / 100;
    assert!(
        diff <= max_diff,
        "PumpFun Simulation Reverse Verification Failed: Expected ~{}, Got {}, Diff {}",
        expected_return,
        actual_return,
        diff
    );
    println!("✅ PumpFun Round Trip Verification Passed (Diff: {})", diff);

    let sell_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sell_response.transaction)
        .expect("Failed to decode sell transaction");

    let (sell_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&sell_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize sell transaction");

    // 3. Merge Instructions
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

    // 4. Build Atomic Transaction
    let composite_transaction = Transaction::new_with_payer(&instructions, Some(&payer));
    let mut composite_transaction = composite_transaction;
    composite_transaction.message.recent_blockhash = recent_blockhash;

    // 5. Simulate Atomic Transaction
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
            assert!(
                sim_result.value.err.is_none(),
                "Composite Simulation should succeed. Error: {:?}",
                sim_result.value.err
            );
            let units_consumed = sim_result
                .value
                .units_consumed
                .expect("Should have units consumed");
            assert!(units_consumed > 0, "Should consume compute units");
            println!(
                "✅ Composite PumpFun Simulation Successful! Consumed {} compute units",
                units_consumed
            );
        }
        Err(e) => panic!("RPC Error during Composite simulation: {}", e),
    }
}

#[tokio::test]
async fn test_pumpswap_quote_simulation() {
    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;

    let pool_address = Pubkey::from_str("F3g7TCcqpQHuFzDcn9xXhCTgrxzuRANnYX9jkzWGpJrZ").unwrap();
    let token_mint_address =
        Pubkey::from_str("3few1wmJAtaFLd4mwT9e7gaaTuccnn5BakUTJSz9pump").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");

    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    let vault_base_account = rpc_client
        .get_account(&pumpswap_pool.pool_base_token_account)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&pumpswap_pool.pool_quote_token_account)
        .await
        .expect("Failed to fetch quote vault");

    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::PumpSwap(PumpSwapPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        index: pumpswap_pool.index,
        creator: Some(pumpswap_pool.creator),
        base_mint: pumpswap_pool.base_mint,
        quote_mint: pumpswap_pool.quote_mint,
        pool_base_token_account: pumpswap_pool.pool_base_token_account,
        pool_quote_token_account: pumpswap_pool.pool_quote_token_account,
        last_updated: u64::MAX,
        base_reserve,
        quote_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
        is_cashback: pumpswap_pool.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: true,
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

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let input_amount = 10_000_000; // 0.01 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: token_mint_address.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 200,
    };

    println!("Calling get_quote handler (Buy)...");
    let result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::extract::Query(request),
    )
    .await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Buy Quote Response!");
            println!("Transaction Base64: {}", response.transaction);

            let tx_bytes = base64::engine::general_purpose::STANDARD
                .decode(&response.transaction)
                .expect("Failed to decode base64 transaction");

            let (transaction, _): (Transaction, usize) =
                bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
                    .expect("Failed to deserialize transaction");

            println!("Simulating Buy transaction on-chain...");
            let simulation = rpc_client.simulate_transaction(&transaction).await;

            match simulation {
                Ok(sim_result) => {
                    println!("Buy Simulation Result: {:#?}", sim_result);
                    if let Some(err) = sim_result.value.err {
                        panic!("Buy Simulation failed: {:?}", err);
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
                        "✅ Buy Simulation Successful! Consumed {} compute units",
                        units_consumed
                    );
                }
                Err(e) => panic!("RPC Error during Buy simulation: {}", e),
            }
        }
        Err((status, axum::Json(error_res))) => {
            panic!("Buy Quote request failed: {} - {}", status, error_res.error);
        }
    }
}

#[tokio::test]
async fn test_pumpswap_quote_simulation_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;

    let pool_address = Pubkey::from_str("F3g7TCcqpQHuFzDcn9xXhCTgrxzuRANnYX9jkzWGpJrZ").unwrap();
    let token_mint_address =
        Pubkey::from_str("3few1wmJAtaFLd4mwT9e7gaaTuccnn5BakUTJSz9pump").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");

    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    let vault_base_account = rpc_client
        .get_account(&pumpswap_pool.pool_base_token_account)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&pumpswap_pool.pool_quote_token_account)
        .await
        .expect("Failed to fetch quote vault");

    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::PumpSwap(PumpSwapPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        index: pumpswap_pool.index,
        creator: Some(pumpswap_pool.creator),
        base_mint: pumpswap_pool.base_mint,
        quote_mint: pumpswap_pool.quote_mint,
        pool_base_token_account: pumpswap_pool.pool_base_token_account,
        pool_quote_token_account: pumpswap_pool.pool_quote_token_account,
        last_updated: u64::MAX,
        base_reserve,
        quote_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
        is_cashback: pumpswap_pool.is_cashback,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;

    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: true,
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
    // Use an account with lots of SOL (e.g. a Binance Hot Wallet or a super rich address: 5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1 - Raydium Authority)
    let user_wallet_str = "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    let buy_input_amount = 100; // tiny amount
    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        output_token: token_mint_address.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: buy_input_amount,
        slippage_bps: 200,
    };

    println!("Getting Buy Quote...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::extract::Query(buy_request),
    )
    .await
    .expect("Buy request failed");

    let buy_response = match buy_result {
        axum::Json(res) => {
            println!("buy_response.output_amount: {}", res.output_amount);
            res
        }
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
        input_token: token_mint_address.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 200,
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

    // Verify Round Trip (should regain approx 50% of input value)
    // Initial Buy Input: 100_000_000 SOL
    // Sell Input: 50% of Buy Output (approx 50M SOL worth of Tokens)
    // Expected Output: approx 50_000_000 SOL
    let expected_return = buy_input_amount / 2;
    let actual_return = sell_response.output_amount;
    let diff = if actual_return > expected_return {
        actual_return - expected_return
    } else {
        expected_return - actual_return
    };

    // Tolerance 2% (PumpFun)
    let max_diff = expected_return * 2 / 100;
    assert!(
        diff <= max_diff,
        "PumpFun Simulation Reverse Verification Failed: Expected ~{}, Got {}, Diff {}",
        expected_return,
        actual_return,
        diff
    );
    println!("✅ PumpFun Round Trip Verification Passed (Diff: {})", diff);

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
                "✅ Composite PumpSwap Simulation Successful! Consumed {} compute units",
                units_consumed
            );
        }
        Err(e) => panic!("RPC Error during Composite simulation: {}", e),
    }
}
