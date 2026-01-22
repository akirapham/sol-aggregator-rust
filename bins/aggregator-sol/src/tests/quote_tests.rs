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
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_amm_v4::types::AmmInfo as RaydiumAmmInfoRaw;

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
        last_updated: current_timestamp(),
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
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::types::PoolState as RaydiumCpmmStateRaw;

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
        last_updated: current_timestamp(),
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
async fn test_pumpfun_quote_simulation() {
    use crate::api::dto::{QuoteRequest, QuoteResponse};
    use crate::api::AppState;
    use base64::Engine;
    use borsh::{BorshDeserialize, BorshSerialize};
    use solana_sdk::transaction::Transaction;

    // Define the raw on-chain layout matching the IDL
    #[derive(BorshDeserialize, Debug)]
    struct BondingCurveRaw {
        virtual_token_reserves: u64,
        virtual_sol_reserves: u64,
        real_token_reserves: u64,
        real_sol_reserves: u64,
        token_total_supply: u64,
        complete: bool,
        creator: Pubkey,
        is_mayhem_mode: bool,
    }

    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    // Real Data
    let bonding_curve_address =
        Pubkey::from_str("9Exw9tyEYPEv5wz7WsUZZVQEG262csVyHEKYcgNeEaf1").unwrap();
    let token_mint_address =
        Pubkey::from_str("BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump").unwrap();

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
    // assert_eq!(mint_account.owner.to_string(), "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", "Mint is not owned by Standard Token Program");

    // Fetch real bonding curve state
    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    // Deserialize (skip 8 byte discriminator)
    // Note: try_from_slice expects exact length match. We use try_from_slice but catch the error or slice responsibly.
    // However, Borsh deserialization for structs is strict.
    // We should use a reader that allows trailing bytes or just slice the exact size of the struct if known.
    // The struct has 8 u64s + 1 bool + 32 bytes + 1 bool = 64 + 1 + 32 + 1 = 98 bytes?
    // Let's actually check expected size or just deserialize what we need.
    // Safer way: Use `BorshDeserialize::deserialize(&mut &data[..])` which reads what it needs and leaves the rest.

    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurveRaw::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    println!("Fetched Real Bonding Curve State: {:#?}", raw_state);

    // Create PoolState from real data
    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: current_timestamp(),
        liquidity_usd: 30000.0, // Mock value, doesn't affect simulation validity
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager
        .inject_token(crate::tests::quote_tests::wsol_token())
        .await;

    // Inject the real token metadata (dummy metadata but correct address)
    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("PUMP".to_string()),
            name: Some("Pump Token".to_string()),
            decimals: 6, // PumpFun tokens usually 6
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
    let input_amount = 100_000_000; // 0.1 SOL (to simulate a realistic trade)

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: token_mint_address.to_string(), // Target Token (Mint)
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 100, // 1%
    };

    println!("Calling get_quote handler...");
    // Call the handler
    let result =
        crate::api::handlers::get_quote(axum::extract::State(state), axum::Json(request)).await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Quote Response!");
            println!("Routes: {}", response.routes.len());
            println!("Output Amount: {}", response.output_amount);
            println!("Transaction Base64: {}", response.transaction);

            // Decode Transaction
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

                    // Assert no errors
                    assert!(
                        sim_result.value.err.is_none(),
                        "Simulation should succeed with real bonding curve data. Error: {:?}",
                        sim_result.value.err
                    );

                    // Assert compute units consumed
                    let units_consumed = sim_result
                        .value
                        .units_consumed
                        .expect("Should have units consumed");
                    assert!(units_consumed > 0, "Should consume compute units");
                    assert!(
                        units_consumed < 200_000,
                        "Should not consume excessive compute units"
                    );

                    // Assert token balances changed (user received tokens)
                    if let Some(post_balances) = sim_result.value.post_token_balances {
                        assert!(
                            !post_balances.is_empty(),
                            "Should have token balance changes"
                        );
                        // Verify user received PUMP tokens
                        let user_received_tokens = post_balances.iter().any(|b| {
                            b.mint == "BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump"
                                && b.owner.as_ref().map(|o| o.as_str())
                                    == Some("DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm")
                        });
                        assert!(user_received_tokens, "User should receive PUMP tokens");
                    }

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
            eprintln!("get_quote failed: {} - {}", status, error_res.error);
            for detail in error_res.details {
                eprintln!("  Detail: {}", detail);
            }
            panic!("Quote request failed");
        }
    }
}

#[tokio::test]
async fn test_pumpfun_quote() {
    use borsh::BorshDeserialize;

    // Define the raw on-chain layout matching the IDL
    #[derive(BorshDeserialize, Debug)]
    struct BondingCurveRaw {
        virtual_token_reserves: u64,
        virtual_sol_reserves: u64,
        real_token_reserves: u64,
        real_sol_reserves: u64,
        token_total_supply: u64,
        complete: bool,
        creator: Pubkey,
        is_mayhem_mode: bool,
    }

    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    // Real PumpFun bonding curve
    let bonding_curve_address =
        Pubkey::from_str("9Exw9tyEYPEv5wz7WsUZZVQEG262csVyHEKYcgNeEaf1").unwrap();
    let token_mint_address =
        Pubkey::from_str("BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    // Deserialize (skip 8 byte discriminator)
    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurveRaw::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    // Create PoolState from real data
    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: current_timestamp(),
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: Token -> SOL
    verify_quote(
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
        1_000_000, // 1 PUMP token
        bonding_curve_address,
    )
    .await;
}

#[tokio::test]
async fn test_bonk_quote() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::bonk::types::PoolState as BonkPoolStateRaw;

    let (pool_manager, config) = create_test_setup(vec!["bonk"]).await;

    // Real Bonk token from bonk.fun
    let token_mint = Pubkey::from_str("71L6279XNuu9uvXZ5iPoMe8r2aTQzP9qy9FKzKbbbonk").unwrap();

    // Use the actual pool address provided
    let bonding_curve_address =
        Pubkey::from_str("4RqyvRAYj2s9zkCEn8jRq2R21v8ZPoAsX6GTZmf1Xies").unwrap();

    println!("Token mint: {}", token_mint);
    println!("Bonding curve address: {}", bonding_curve_address);

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account");

    // Deserialize Bonk bonding curve state (skip 8-byte discriminator)
    let bonk_pool_state = BonkPoolStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize bonding curve state");

    println!("Bonk pool state: {:#?}", bonk_pool_state);

    // Fetch vault accounts to get real reserves
    let base_vault_account = rpc_client
        .get_account(&bonk_pool_state.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let quote_vault_account = rpc_client
        .get_account(&bonk_pool_state.quote_vault)
        .await
        .expect("Failed to fetch quote vault");

    // Parse token account data (amount at offset 64)
    let real_base = u64::from_le_bytes(base_vault_account.data[64..72].try_into().unwrap());
    let real_quote = u64::from_le_bytes(quote_vault_account.data[64..72].try_into().unwrap());

    println!("Real reserves: base={}, quote={}", real_base, real_quote);

    // Create pool state with real data
    let pool_state = PoolState::Bonk(BonkPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        status: bonk_pool_state.status,
        total_base_sell: bonk_pool_state.total_base_sell,
        base_reserve: bonk_pool_state.virtual_base,
        quote_reserve: bonk_pool_state.virtual_quote,
        liquidity_usd: (real_base as f64 * 2.0) / 1e9 * 200.0, // Estimate
        real_base,
        real_quote,
        quote_protocol_fee: bonk_pool_state.quote_protocol_fee,
        platform_fee: bonk_pool_state.platform_fee,
        global_config: bonk_pool_state.global_config,
        platform_config: bonk_pool_state.platform_config,
        base_mint: bonk_pool_state.base_mint,
        quote_mint: bonk_pool_state.quote_mint,
        base_vault: bonk_pool_state.base_vault,
        quote_vault: bonk_pool_state.quote_vault,
        creator: bonk_pool_state.creator,
        last_updated: current_timestamp(),
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;

    // The pool uses base_mint (the bonk token) and quote_mint (USD stablecoin)
    // Swap from quote (USD) to base (BONK token)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: bonk_pool_state.quote_mint,
            symbol: Some("USD".to_string()),
            name: Some("USD Stablecoin".to_string()),
            decimals: bonk_pool_state.quote_decimals,
            is_token_2022: false,
            logo_uri: None,
        },
        Token {
            address: bonk_pool_state.base_mint,
            symbol: Some("BONK".to_string()),
            name: Some("Bonk Token".to_string()),
            decimals: bonk_pool_state.base_decimals,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000, // 1 USD (6 decimals)
        bonding_curve_address,
    )
    .await;
}

// Note: CLMM, Whirlpool, Meteora pools require tick arrays for quote calculations
// These tests are marked as ignored until tick array support is added to the test infrastructure

#[tokio::test]
async fn test_raydium_clmm_quote() {
    use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState as RaydiumClmmStateRaw;

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
        last_updated: current_timestamp(),
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
        last_updated: current_timestamp(),
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

#[tokio::test]
async fn test_pumpswap_quote() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::types::{
        Pool as PumpSwapPoolRaw, POOL_SIZE,
    };

    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;

    // Real SOL-Token PumpSwap pool
    let pool_address = Pubkey::from_str("4w2cysotX6czaUGmmWg13hDpY4QEMG2CzeKYEQyK9Ama").unwrap();
    let token_mint = Pubkey::from_str("5UUH9RTDiSpq6HKS6bp4NdU9PNJpXRXuiw6ShBTBhgH2").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");

    println!("PumpSwap account data length: {}", account.data.len());

    // Deserialize PumpSwap pool state using the correct size
    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    println!("PumpSwap pool state: {:#?}", pumpswap_pool);

    // Fetch token vault accounts to get real reserves
    let vault_base_account = rpc_client
        .get_account(&pumpswap_pool.pool_base_token_account)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&pumpswap_pool.pool_quote_token_account)
        .await
        .expect("Failed to fetch quote vault");

    // Parse token account data (amount at offset 64)
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    println!(
        "Real reserves: base={}, quote={}",
        base_reserve, quote_reserve
    );

    // Create pool state with real data
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
        last_updated: current_timestamp(),
        base_reserve,
        quote_reserve,
        liquidity_usd: (quote_reserve as f64 * 2.0) / 1e9 * 200.0, // Estimate
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> Token (quote is SOL, base is token)
    verify_quote(
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
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_meteora_dbc_quote() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::{
        PoolConfig as MeteoraDbcConfigRaw, VirtualPool as MeteoraDbcPoolRaw, POOL_CONFIG_SIZE,
        VIRTUAL_POOL_SIZE,
    };

    let (pool_manager, config) = create_test_setup(vec!["meteora_dbc"]).await;

    // Real SOL-Token Meteora DBC pool
    let pool_address = Pubkey::from_str("6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu").unwrap();
    let token_mint = Pubkey::from_str("6yXTqNnj8PGbJosD6dvpQLFVxaDNpkQmPo7fMxLeUh6A").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch Meteora DBC pool account");

    // Deserialize Meteora DBC pool state (skip 8-byte discriminator)
    let dbc_pool = MeteoraDbcPoolRaw::try_from_slice(&account.data[8..8 + VIRTUAL_POOL_SIZE])
        .expect("Failed to deserialize Meteora DBC pool");

    println!("Meteora DBC pool state - config: {}", dbc_pool.config);

    // Fetch PoolConfig to get quote_mint
    let config_account = rpc_client
        .get_account(&dbc_pool.config)
        .await
        .expect("Failed to fetch Meteora DBC config account");

    let dbc_config =
        MeteoraDbcConfigRaw::try_from_slice(&config_account.data[8..8 + POOL_CONFIG_SIZE])
            .expect("Failed to deserialize Meteora DBC config");

    println!("Meteora DBC config - quote_mint: {}", dbc_config.quote_mint);

    // Fetch token vault accounts to get real reserves
    let vault_base_account = rpc_client
        .get_account(&dbc_pool.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&dbc_pool.quote_vault)
        .await
        .expect("Failed to fetch quote vault");

    // Parse token account data (amount at offset 64)
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    println!(
        "Real reserves: base={}, quote={}",
        base_reserve, quote_reserve
    );

    // Create pool state with real data
    let pool_state = PoolState::MeteoraDbc(Box::new(DbcPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        config: dbc_pool.config,
        creator: dbc_pool.creator,
        base_mint: dbc_pool.base_mint,
        base_vault: dbc_pool.base_vault,
        quote_vault: dbc_pool.quote_vault,
        base_reserve,
        quote_reserve,
        protocol_base_fee: dbc_pool.protocol_base_fee,
        protocol_quote_fee: dbc_pool.protocol_quote_fee,
        partner_base_fee: dbc_pool.partner_base_fee,
        partner_quote_fee: dbc_pool.partner_quote_fee,
        sqrt_price: dbc_pool.sqrt_price,
        activation_point: dbc_pool.activation_point,
        pool_type: dbc_pool.pool_type,
        is_migrated: dbc_pool.is_migrated,
        is_partner_withdraw_surplus: dbc_pool.is_partner_withdraw_surplus,
        is_protocol_withdraw_surplus: dbc_pool.is_protocol_withdraw_surplus,
        migration_progress: dbc_pool.migration_progress,
        is_withdraw_leftover: dbc_pool.is_withdraw_leftover,
        is_creator_withdraw_surplus: dbc_pool.is_creator_withdraw_surplus,
        migration_fee_withdraw_status: dbc_pool.migration_fee_withdraw_status,
        finish_curve_timestamp: dbc_pool.finish_curve_timestamp,
        creator_base_fee: dbc_pool.creator_base_fee,
        creator_quote_fee: dbc_pool.creator_quote_fee,
        liquidity_usd: 1_000_000.0, // High liquidity to pass aggregator filter
        last_updated: current_timestamp(),
        pool_config: Some(dbc_config.clone()),
        volatility_tracker: Some(dbc_pool.volatility_tracker),
    }));
    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> Token (quote is SOL, base is token)
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct MeteoraDammV2PoolRaw {
    pub pool_fees: solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::PoolFeesStruct,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub whitelisted_vault: Pubkey,
    pub partner: Pubkey,
    pub liquidity: u128,
    pub padding: u128,
    pub protocol_a_fee: u64,
    pub protocol_b_fee: u64,
    pub partner_a_fee: u64,
    pub partner_b_fee: u64,
    pub sqrt_min_price: u128,
    pub sqrt_max_price: u128,
    pub sqrt_price: u128,
    pub activation_point: u64,
    pub activation_type: u8,
    pub pool_status: u8,
    pub token_a_flag: u8,
    pub token_b_flag: u8,
    pub collect_fee_mode: u8,
    pub pool_type: u8,
    pub version: u8,
    pub padding_0: u8,
    pub fee_a_per_liquidity: [u8; 32],
    pub fee_b_per_liquidity: [u8; 32],
    pub permanent_lock_liquidity: u128,
    pub metrics: solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::PoolMetrics,
    pub creator: Pubkey,
    pub padding_1: [u64; 6],
    pub reward_infos: [solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::RewardInfo; 2],
}

// Implement safe deserialization manually or via bytemuck if aligned
impl MeteoraDammV2PoolRaw {
    pub fn try_from_slice(data: &[u8]) -> Result<Self, std::io::Error> {
        let size = std::mem::size_of::<Self>();
        if data.len() < size + 8 {
            // +8 for discriminator
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Data too short",
            ));
        }
        let data = &data[8..]; // Skip discriminator

        // Safety: We assume the data is POD and field alignment matches logic.
        // Given complexity of nested structs, safe parsing is better, but for test we try unsafe cast if layout matches
        if data.len() < size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Data too short after discriminator",
            ));
        }

        let ptr = data.as_ptr() as *const MeteoraDammV2PoolRaw;
        Ok(unsafe { ptr.read_unaligned() })
    }
}

#[tokio::test]
async fn test_meteora_damm_v2_quote() {
    let _ = env_logger::builder().is_test(true).try_init();
    let (pool_manager, config) = create_test_setup(vec!["meteora_dammv2"]).await;

    // Meteora DAMM V2 Pool: SOL-RALPH
    // Pool Address: DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf
    let pool_address = Pubkey::from_str("DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf").unwrap();
    let _token_a_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // SOL
    let token_b_mint = Pubkey::from_str("CxWPdDBqxVo3fnTMRTvNuSrd4gkp78udSrFvkVDBAGS").unwrap(); // RALPH

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");

    // Deserialize
    let raw_pool = MeteoraDammV2PoolRaw::try_from_slice(&account.data)
        .expect("Failed to deserialize DAMM V2 pool");

    println!(
        "Meteora DAMM V2 pool found - Liquidity: {}",
        raw_pool.liquidity
    );

    // Create PoolState
    let pool_state =
        PoolState::MeteoraDammV2(Box::new(crate::pool_data_types::MeteoraDammV2PoolState {
            slot: 0,
            transaction_index: Some(0),
            address: pool_address,
            pool_fees: raw_pool.pool_fees,
            token_a_mint: raw_pool.token_a_mint,
            token_b_mint: raw_pool.token_b_mint, // Should match RALPH or SOL
            token_a_vault: raw_pool.token_a_vault,
            token_b_vault: raw_pool.token_b_vault,
            whitelisted_vault: raw_pool.whitelisted_vault,
            partner: raw_pool.partner,
            liquidity: raw_pool.liquidity,
            protocol_a_fee: raw_pool.protocol_a_fee,
            protocol_b_fee: raw_pool.protocol_b_fee,
            partner_a_fee: raw_pool.partner_a_fee,
            partner_b_fee: raw_pool.partner_b_fee,
            sqrt_min_price: raw_pool.sqrt_min_price,
            sqrt_max_price: raw_pool.sqrt_max_price,
            sqrt_price: raw_pool.sqrt_price,
            activation_point: raw_pool.activation_point,
            activation_type: raw_pool.activation_type,
            pool_status: raw_pool.pool_status,
            token_a_flag: raw_pool.token_a_flag,
            token_b_flag: raw_pool.token_b_flag,
            collect_fee_mode: raw_pool.collect_fee_mode,
            pool_type: raw_pool.pool_type,
            version: raw_pool.version,
            fee_a_per_liquidity: raw_pool.fee_a_per_liquidity,
            fee_b_per_liquidity: raw_pool.fee_b_per_liquidity,
            permanent_lock_liquidity: raw_pool.permanent_lock_liquidity,
            metrics: raw_pool.metrics,
            creator: raw_pool.creator,
            reward_infos: raw_pool.reward_infos,
            liquidity_usd: 1_000_000.0, // High liquidity to pass aggregator filter
            last_updated: current_timestamp(),
        }));

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> RALPH
    // Determine which is base/quote/a/b.
    // Usually sorted or defined by mint order.
    // We try injecting and swapping.

    verify_quote(
        pool_manager,
        config,
        wsol_token(), // Input SOL
        Token {
            address: token_b_mint, // RALPH (assuming token_b is RALPH, checks below)
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9, // Assuming 9 for now, should verify
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 SOL
        pool_address,
    )
    .await;
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProtocolFeeRaw {
    pub amount_x: u64,
    pub amount_y: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RewardInfoRaw {
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub funder: Pubkey,
    pub reward_duration: u64,
    pub reward_duration_end: u64,
    pub reward_rate: u128,
    pub last_update_time: u64,
    pub cumulative_seconds_with_empty_liquidity_reward: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StaticParametersRaw {
    pub base_factor: u16,
    pub filter_period: u16,
    pub decay_period: u16,
    pub reduction_factor: u16,
    pub variable_fee_control: u32,
    pub max_volatility_accumulator: u32,
    pub min_bin_id: i32,
    pub max_bin_id: i32,
    pub protocol_share: u16,
    pub base_fee_power_factor: u8,
    pub function_type: u8,
    pub padding: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VariableParametersRaw {
    pub volatility_accumulator: u32,
    pub volatility_reference: u32,
    pub index_reference: i32,
    pub padding: [u8; 4],
    pub last_update_timestamp: i64,
    pub padding_1: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct LbPairRaw {
    pub parameters: StaticParametersRaw,
    pub v_parameters: VariableParametersRaw,
    pub bump_seed: [u8; 1],
    pub bin_step_seed: [u8; 2],
    pub pair_type: u8,
    pub active_id: i32,
    pub bin_step: u16,
    pub status: u8,
    pub require_base_factor_seed: u8,
    pub base_factor_seed: [u8; 2],
    pub activation_type: u8,
    pub creator_pool_on_off_control: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee: ProtocolFeeRaw,
    pub padding_1: [u8; 32],
    pub reward_infos: [RewardInfoRaw; 2],
    pub oracle: Pubkey,
    pub bin_array_bitmap: [u64; 16],
    pub last_updated_at: i64,
    pub padding_2: [u8; 32],
    pub pre_activation_swap_address: Pubkey,
    pub base_key: Pubkey,
    pub activation_point: u64,
    pub pre_activation_duration: u64,
    pub padding_3: [u8; 8],
    pub padding_4: u64,
    pub creator: Pubkey,
    pub token_mint_x_program_flag: u8,
    pub token_mint_y_program_flag: u8,
    pub version: u8,
    pub reserved: [u8; 21],
}

impl LbPairRaw {
    pub fn try_from_slice(data: &[u8]) -> Result<Self, std::io::Error> {
        let size = std::mem::size_of::<Self>();
        if data.len() < size + 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Data too short: {} < {}", data.len(), size + 8),
            ));
        }
        let data = &data[8..];
        if data.len() < size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Data too short after discriminator",
            ));
        }
        let ptr = data.as_ptr() as *const LbPairRaw;
        Ok(unsafe { ptr.read_unaligned() })
    }
}

#[tokio::test]
async fn test_meteora_dlmm_quote() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let (pool_manager, config) = create_test_setup(vec!["meteora_dlmm"]).await;

    // Meteora DLMM Pool: SOL-Token
    // Pool Address: 6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP
    let pool_address = Pubkey::from_str("6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP").unwrap();
    let ralph_mint = Pubkey::from_str("8116V1BW9zaXUM6pVhWVaAduKrLcEBi3RGXedKTrBAGS").unwrap(); // Token X
    let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // Token Y

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");

    // Deserialize
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    println!(
        "Meteora DLMM pool found - Active ID: {}, Token X: {}, Token Y: {}",
        raw_pool.active_id, raw_pool.token_x_mint, raw_pool.token_y_mint
    );

    // Debug Raw Pool details
    println!(
        "Bin Step: {}, Status: {}, Bin Array Bitmap: {:?}",
        raw_pool.bin_step, raw_pool.status, raw_pool.bin_array_bitmap
    );

    // Verify token match
    assert_eq!(
        raw_pool.token_x_mint, ralph_mint,
        "Token X mismatch (Expected RALPH)"
    );
    assert_eq!(
        raw_pool.token_y_mint, sol_mint,
        "Token Y mismatch (Expected SOL)"
    );

    println!(
        "Program ID from State: {}",
        crate::pool_data_types::MeteoraDlmmPoolState::get_program_id()
    );

    // Attempt to deserialize using SDK type directly for simplicity in constructing PoolState
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    // Skip 8 bytes discriminator
    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        let size_sdk = std::mem::size_of::<SdkLbPair>();
        let size_raw = std::mem::size_of::<LbPairRaw>();
        println!(
            "SDK LbPair Size: {}, Raw LbPair Size: {}",
            size_sdk, size_raw
        );

        if size_sdk == size_raw {
            unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
        } else {
            panic!(
                "SDK LbPair size ({}) != Raw LbPair size ({}). IDL vs SDK mismatch.",
                size_sdk, size_raw
            );
        }
    } else {
        panic!("Account data too short");
    };

    // Fix mismatch by copying from verified raw_pool
    // SdkLbPair layout is likely shifted relative to on-chain data due to padding or version differences
    // We overwrite critical fields with values we KNOW are correct from Borsh decoding
    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    // Copy parameters to ensure logic validation passes
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    // Copy bitmap to ensure we find the right bin arrays
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    // Fetch Bitmap Extension
    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;

    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        println!("Bitmap Extension Found! Size: {}", bitmap_acc.data.len());
        if bitmap_acc.data.len() >= 8 {
            // Assuming SDK BitmapExtension matches
            let size_sdk = std::mem::size_of::<SdkBitmapExtension>();
            if bitmap_acc.data.len() - 8 >= size_sdk {
                let ext: SdkBitmapExtension = unsafe {
                    (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
                };
                bitmap_extension = Some(ext);
            } else {
                println!("Bitmap extension size mismatch or too small");
            }
        }
    } else {
        println!("Bitmap Extension Not Found (Optional)");
    }

    // Create PoolState
    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(), // To be populated
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: current_timestamp(),
    };

    // Fetch Bin Arrays
    // Use the optimized fetcher to get all necessary bin arrays
    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());

    println!("Fetching bin arrays using MeteoraDlmmBinArrayFetcher...");
    match fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        Ok(bin_arrays) => {
            println!("Fetcher returned {} bin arrays", bin_arrays.len());
            for ba in bin_arrays {
                pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
            }
        }
        Err(e) => panic!("Failed to fetch bin arrays: {:?}", e),
    }
    println!("Fetched {} bin arrays", pool_state_struct.bin_arrays.len());

    // Debug Bin Liquidity
    let mut total_liquidity_x = 0u64;
    let mut total_liquidity_y = 0u64;
    for (idx, ba) in &pool_state_struct.bin_arrays {
        let mut active_bins = 0;
        for bin in &ba.bins {
            if bin.amount_x > 0 || bin.amount_y > 0 {
                total_liquidity_x += bin.amount_x;
                total_liquidity_y += bin.amount_y;
                active_bins += 1;
            }
        }
        println!("BinArray {}: {} active bins", idx, active_bins);
    }
    println!(
        "Total Liquidity X: {}, Y: {}",
        total_liquidity_x, total_liquidity_y
    );

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct));
    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> RALPH
    verify_quote(
        pool_manager,
        config,
        wsol_token(), // Input SOL (Token Y)
        Token {
            address: ralph_mint, // Output RALPH (Token X)
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        100_000_000, // 0.1 SOL (Reduced from 1 SOL to ensure liquidity coverage)
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_amm_v4_quote_reverse() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_amm_v4::types::AmmInfo as RaydiumAmmInfoRaw;

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
        last_updated: current_timestamp(),
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
async fn test_meteora_dlmm_quote_reverse() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let (pool_manager, config) = create_test_setup(vec!["meteora_dlmm"]).await;

    let pool_address = Pubkey::from_str("6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP").unwrap();
    let ralph_mint = Pubkey::from_str("8116V1BW9zaXUM6pVhWVaAduKrLcEBi3RGXedKTrBAGS").unwrap();
    let _sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();

    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    // Initialize SdkLbPair and fix data mismatch (same logic as forward test)
    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
    } else {
        panic!("Account data too short");
    };

    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;
    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        if bitmap_acc.data.len() >= 8 {
            let ext: SdkBitmapExtension = unsafe {
                (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
            };
            bitmap_extension = Some(ext);
        }
    }

    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(),
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: current_timestamp(),
    };

    // Use Fetcher
    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());
    if let Ok(bin_arrays) = fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        for ba in bin_arrays {
            pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
        }
    }

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct));
    pool_manager.inject_pool(pool_state).await;

    // Test swap: RALPH -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        100_000_000, // Swap amount
        pool_address,
    )
    .await;
}

#[tokio::test]
async fn test_orca_whirlpool_quote_reverse() {
    use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
    use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::types::{
        Tick, TickArrayState, WhirlpoolPoolState as WhirlpoolStateRaw,
    };

    let (pool_manager, config) = create_test_setup(vec!["orca"]).await;
    let pool_address = Pubkey::from_str("5zpyutJu9ee6jFymDGoK7F6S5Kczqtc9FomP3ueKuyA9").unwrap();
    let bonk_mint = Pubkey::from_str("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263").unwrap();

    let rpc_client = pool_manager.get_rpc_client();
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
        last_updated: current_timestamp(),
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

#[tokio::test]
async fn test_raydium_cpmm_quote_reverse() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::types::PoolState as RaydiumCpmmStateRaw;

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
        last_updated: current_timestamp(),
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
async fn test_pumpfun_quote_reverse() {
    use borsh::BorshDeserialize;

    // Define the raw on-chain layout matching the IDL
    #[derive(BorshDeserialize, Debug)]
    struct BondingCurveRaw {
        virtual_token_reserves: u64,
        virtual_sol_reserves: u64,
        real_token_reserves: u64,
        real_sol_reserves: u64,
        token_total_supply: u64,
        complete: bool,
        creator: Pubkey,
        is_mayhem_mode: bool,
    }

    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    // Real PumpFun bonding curve
    let bonding_curve_address =
        Pubkey::from_str("9Exw9tyEYPEv5wz7WsUZZVQEG262csVyHEKYcgNeEaf1").unwrap();
    let token_mint_address =
        Pubkey::from_str("BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account from mainnet");

    // Deserialize (skip 8 byte discriminator)
    let mut data_slice = &account.data[8..];
    let raw_state = BondingCurveRaw::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    // Create PoolState from real data
    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: current_timestamp(),
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: SOL -> Token (reverse direction)
    verify_quote(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: token_mint_address,
            symbol: Some("PUMP".to_string()),
            name: Some("Pump Token".to_string()),
            decimals: 6,
            is_token_2022: true,
            logo_uri: None,
        },
        100_000_000, // 0.1 SOL
        bonding_curve_address,
    )
    .await;
}

#[tokio::test]
async fn test_bonk_quote_reverse() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::bonk::types::PoolState as BonkPoolStateRaw;

    let (pool_manager, config) = create_test_setup(vec!["bonk"]).await;
    let _token_mint = Pubkey::from_str("71L6279XNuu9uvXZ5iPoMe8r2aTQzP9qy9FKzKbbbonk").unwrap();
    let bonding_curve_address =
        Pubkey::from_str("4RqyvRAYj2s9zkCEn8jRq2R21v8ZPoAsX6GTZmf1Xies").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&bonding_curve_address)
        .await
        .expect("Failed to fetch bonding curve account");
    let bonk_pool_state = BonkPoolStateRaw::try_from_slice(&account.data[8..])
        .expect("Failed to deserialize bonding curve state");

    let base_vault_account = rpc_client
        .get_account(&bonk_pool_state.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let quote_vault_account = rpc_client
        .get_account(&bonk_pool_state.quote_vault)
        .await
        .expect("Failed to fetch quote vault");
    let real_base = u64::from_le_bytes(base_vault_account.data[64..72].try_into().unwrap());
    let real_quote = u64::from_le_bytes(quote_vault_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::Bonk(BonkPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        status: bonk_pool_state.status,
        total_base_sell: bonk_pool_state.total_base_sell,
        base_reserve: bonk_pool_state.virtual_base,
        quote_reserve: bonk_pool_state.virtual_quote,
        liquidity_usd: 1_000_000.0,
        real_base,
        real_quote,
        quote_protocol_fee: bonk_pool_state.quote_protocol_fee,
        platform_fee: bonk_pool_state.platform_fee,
        global_config: bonk_pool_state.global_config,
        platform_config: bonk_pool_state.platform_config,
        base_mint: bonk_pool_state.base_mint,
        quote_mint: bonk_pool_state.quote_mint,
        base_vault: bonk_pool_state.base_vault,
        quote_vault: bonk_pool_state.quote_vault,
        creator: bonk_pool_state.creator,
        last_updated: current_timestamp(),
        is_state_keys_initialized: true,
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: BONK -> USD (reverse direction from forward test)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: bonk_pool_state.base_mint,
            symbol: Some("BONK".to_string()),
            name: Some("Bonk Token".to_string()),
            decimals: bonk_pool_state.base_decimals,
            is_token_2022: false,
            logo_uri: None,
        },
        Token {
            address: bonk_pool_state.quote_mint,
            symbol: Some("USD".to_string()),
            name: Some("USD Stablecoin".to_string()),
            decimals: bonk_pool_state.quote_decimals,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000_000_000, // 1 BONK (assuming 9 decimals)
        bonding_curve_address,
    )
    .await;
}

#[tokio::test]
async fn test_raydium_clmm_quote_reverse() {
    use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;
    use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState as RaydiumClmmStateRaw;

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
        last_updated: current_timestamp(),
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
        last_updated: current_timestamp(),
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

#[tokio::test]
async fn test_pumpswap_quote_reverse() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::types::{
        Pool as PumpSwapPoolRaw, POOL_SIZE,
    };

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
        last_updated: current_timestamp(),
        base_reserve,
        quote_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
    });

    pool_manager.inject_pool(pool_state).await;

    // Test swap: Token -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
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

#[tokio::test]
async fn test_meteora_dbc_quote_reverse() {
    use borsh::BorshDeserialize;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::{
        PoolConfig as MeteoraDbcConfigRaw, VirtualPool as MeteoraDbcPoolRaw, POOL_CONFIG_SIZE,
        VIRTUAL_POOL_SIZE,
    };

    let (pool_manager, config) = create_test_setup(vec!["meteora_dbc"]).await;
    let pool_address = Pubkey::from_str("6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu").unwrap();
    let token_mint = Pubkey::from_str("6yXTqNnj8PGbJosD6dvpQLFVxaDNpkQmPo7fMxLeUh6A").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch Meteora DBC pool account");
    let dbc_pool = MeteoraDbcPoolRaw::try_from_slice(&account.data[8..8 + VIRTUAL_POOL_SIZE])
        .expect("Failed to deserialize Meteora DBC pool");

    let config_account = rpc_client
        .get_account(&dbc_pool.config)
        .await
        .expect("Failed to fetch Meteora DBC config account");
    let dbc_config =
        MeteoraDbcConfigRaw::try_from_slice(&config_account.data[8..8 + POOL_CONFIG_SIZE])
            .expect("Failed to deserialize Meteora DBC config");

    let vault_base_account = rpc_client
        .get_account(&dbc_pool.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&dbc_pool.quote_vault)
        .await
        .expect("Failed to fetch quote vault");
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::MeteoraDbc(Box::new(DbcPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        config: dbc_pool.config,
        creator: dbc_pool.creator,
        base_mint: dbc_pool.base_mint,
        base_vault: dbc_pool.base_vault,
        quote_vault: dbc_pool.quote_vault,
        base_reserve,
        quote_reserve,
        protocol_base_fee: dbc_pool.protocol_base_fee,
        protocol_quote_fee: dbc_pool.protocol_quote_fee,
        partner_base_fee: dbc_pool.partner_base_fee,
        partner_quote_fee: dbc_pool.partner_quote_fee,
        sqrt_price: dbc_pool.sqrt_price,
        activation_point: dbc_pool.activation_point,
        pool_type: dbc_pool.pool_type,
        is_migrated: dbc_pool.is_migrated,
        is_partner_withdraw_surplus: dbc_pool.is_partner_withdraw_surplus,
        is_protocol_withdraw_surplus: dbc_pool.is_protocol_withdraw_surplus,
        migration_progress: dbc_pool.migration_progress,
        is_withdraw_leftover: dbc_pool.is_withdraw_leftover,
        is_creator_withdraw_surplus: dbc_pool.is_creator_withdraw_surplus,
        migration_fee_withdraw_status: dbc_pool.migration_fee_withdraw_status,
        finish_curve_timestamp: dbc_pool.finish_curve_timestamp,
        creator_base_fee: dbc_pool.creator_base_fee,
        creator_quote_fee: dbc_pool.creator_quote_fee,
        liquidity_usd: 1_000_000.0,
        last_updated: current_timestamp(),
        pool_config: Some(dbc_config.clone()),
        volatility_tracker: Some(dbc_pool.volatility_tracker),
    }));
    pool_manager.inject_pool(pool_state).await;

    // Test swap: Token -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
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

#[tokio::test]
async fn test_meteora_damm_v2_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["meteora_dammv2"]).await;
    let pool_address = Pubkey::from_str("DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf").unwrap();
    let token_b_mint = Pubkey::from_str("CxWPdDBqxVo3fnTMRTvNuSrd4gkp78udSrFvkVDBAGS").unwrap(); // RALPH

    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");
    let raw_pool = MeteoraDammV2PoolRaw::try_from_slice(&account.data)
        .expect("Failed to deserialize DAMM V2 pool");

    let pool_state =
        PoolState::MeteoraDammV2(Box::new(crate::pool_data_types::MeteoraDammV2PoolState {
            slot: 0,
            transaction_index: Some(0),
            address: pool_address,
            pool_fees: raw_pool.pool_fees,
            token_a_mint: raw_pool.token_a_mint,
            token_b_mint: raw_pool.token_b_mint,
            token_a_vault: raw_pool.token_a_vault,
            token_b_vault: raw_pool.token_b_vault,
            whitelisted_vault: raw_pool.whitelisted_vault,
            partner: raw_pool.partner,
            liquidity: raw_pool.liquidity,
            protocol_a_fee: raw_pool.protocol_a_fee,
            protocol_b_fee: raw_pool.protocol_b_fee,
            partner_a_fee: raw_pool.partner_a_fee,
            partner_b_fee: raw_pool.partner_b_fee,
            sqrt_min_price: raw_pool.sqrt_min_price,
            sqrt_max_price: raw_pool.sqrt_max_price,
            sqrt_price: raw_pool.sqrt_price,
            activation_point: raw_pool.activation_point,
            activation_type: raw_pool.activation_type,
            pool_status: raw_pool.pool_status,
            token_a_flag: raw_pool.token_a_flag,
            token_b_flag: raw_pool.token_b_flag,
            collect_fee_mode: raw_pool.collect_fee_mode,
            pool_type: raw_pool.pool_type,
            version: raw_pool.version,
            fee_a_per_liquidity: raw_pool.fee_a_per_liquidity,
            fee_b_per_liquidity: raw_pool.fee_b_per_liquidity,
            permanent_lock_liquidity: raw_pool.permanent_lock_liquidity,
            metrics: raw_pool.metrics,
            creator: raw_pool.creator,
            reward_infos: raw_pool.reward_infos,
            liquidity_usd: 1_000_000.0,
            last_updated: current_timestamp(),
        }));

    pool_manager.inject_pool(pool_state).await;

    // Test swap: RALPH -> SOL (reverse direction)
    verify_quote(
        pool_manager,
        config,
        Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
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
async fn test_pumpfun_quote_simulation_reverse() {
    use crate::api::dto::{QuoteRequest, QuoteResponse};
    use crate::api::AppState;
    use base64::Engine;
    use borsh::{BorshDeserialize, BorshSerialize};
    use solana_sdk::transaction::Transaction;
    use solana_client::rpc_config::RpcSimulateTransactionConfig;
    use solana_commitment_config::CommitmentConfig;
    use solana_sdk::instruction::Instruction;

    #[derive(BorshDeserialize, Debug)]
    struct BondingCurveRaw {
        virtual_token_reserves: u64,
        virtual_sol_reserves: u64,
        real_token_reserves: u64,
        real_sol_reserves: u64,
        token_total_supply: u64,
        complete: bool,
        creator: Pubkey,
        is_mayhem_mode: bool,
    }

    let (pool_manager, config) = create_test_setup(vec!["pumpfun"]).await;

    let bonding_curve_address =
        Pubkey::from_str("9Exw9tyEYPEv5wz7WsUZZVQEG262csVyHEKYcgNeEaf1").unwrap();
    let token_mint_address =
        Pubkey::from_str("BuaDPEf3AN4Lty7Ge1xgk9sFBwLXLP1J5uxGsBdDpump").unwrap();

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
    let raw_state = BondingCurveRaw::deserialize(&mut data_slice)
        .expect("Failed to deserialize bonding curve data");

    let pool_state = PoolState::Pumpfun(PumpfunPoolState {
        slot: 100,
        transaction_index: None,
        address: bonding_curve_address,
        mint: token_mint_address,
        last_updated: current_timestamp(),
        liquidity_usd: 30000.0,
        is_state_keys_initialized: true,
        virtual_token_reserves: raw_state.virtual_token_reserves,
        virtual_sol_reserves: raw_state.virtual_sol_reserves,
        real_token_reserves: raw_state.real_token_reserves,
        real_sol_reserves: raw_state.real_sol_reserves,
        complete: raw_state.complete,
        creator: raw_state.creator,
        is_mayhem_mode: raw_state.is_mayhem_mode,
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager
        .inject_token(crate::tests::quote_tests::wsol_token())
        .await;

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

    // -------------------------------------------------------------------------
    // Composite Simulation: Buy -> Sell (Atomic)
    // -------------------------------------------------------------------------

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    // 1. Get BUY Quote (SOL -> Token)
    let buy_input_amount = 100_000_000; // 0.1 SOL
    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        output_token: token_mint_address.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: buy_input_amount,
        slippage_bps: 100,
    };

    println!("Getting Buy Quote...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(buy_request),
    ).await.expect("Buy request failed");

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
    // Sell half of what we bought
    let buy_output_amount: u64 = buy_response.output_amount;
    let sell_input_amount = buy_output_amount / 2;

    println!("Getting Sell Quote (Amount: {})...", sell_input_amount);
    let sell_request = QuoteRequest {
        input_token: token_mint_address.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 100,
    };

    let sell_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(sell_request),
    ).await.expect("Sell request failed");

    let sell_response = match sell_result {
        axum::Json(res) => res,
    };

    let sell_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sell_response.transaction)
        .expect("Failed to decode sell transaction");
    
    let (sell_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&sell_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize sell transaction");

    // 3. Merge Instructions Helper check
    // Since we are running on mainnet fork basically (simulation), we need correct recent blockhash.
    let recent_blockhash = buy_transaction.message.recent_blockhash;

    // Helper to extract instructions
    let get_instructions = |tx: &Transaction| -> Vec<Instruction> {
        let message = &tx.message;
        message.instructions.iter().map(|ix| {
            Instruction {
                program_id: message.account_keys[ix.program_id_index as usize],
                accounts: ix.accounts.iter().map(|&acc_idx| {
                    let idx = acc_idx as usize;
                    let is_signer = idx < message.header.num_required_signatures as usize;
                    let is_writable = if is_signer {
                        idx < (message.header.num_required_signatures - message.header.num_readonly_signed_accounts) as usize
                    } else {
                        idx < (message.account_keys.len() - message.header.num_readonly_unsigned_accounts as usize)
                    };
                    
                    solana_sdk::instruction::AccountMeta {
                        pubkey: message.account_keys[idx],
                        is_signer,
                        is_writable,
                    }
                }).collect(),
                data: ix.data.clone(),
            }
        }).collect::<Vec<_>>()
    };

    let mut instructions = get_instructions(&buy_transaction);
    let sell_instructions = get_instructions(&sell_transaction);
    
    // Naively append sell instructions.
    instructions.extend(sell_instructions);

    // 4. Build Atomic Transaction
    let composite_transaction = Transaction::new_with_payer(
        &instructions,
        Some(&payer),
    );
     // Update blockhash
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

    let simulation = rpc_client.simulate_transaction_with_config(&composite_transaction, config).await;

    match simulation {
        Ok(sim_result) => {
            println!("Composite Simulation Result: {:#?}", sim_result);
            
            if let Some(logs) = &sim_result.value.logs {
                for log in logs {
                    println!("  {}", log);
                }
            }

            // Assert no errors
            assert!(
                sim_result.value.err.is_none(),
                "Composite Simulation should succeed. Error: {:?}",
                sim_result.value.err
            );

            // Assert compute units consumed
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
    use crate::api::dto::{QuoteRequest, QuoteResponse};
    use crate::api::AppState;
    use base64::Engine;
    use borsh::{BorshDeserialize, BorshSerialize};
    use solana_sdk::transaction::Transaction;
    use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::types::{
        Pool as PumpSwapPoolRaw, POOL_SIZE,
    };

    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;

    // Real SOL-Token PumpSwap pool
    let pool_address = Pubkey::from_str("4w2cysotX6czaUGmmWg13hDpY4QEMG2CzeKYEQyK9Ama").unwrap();
    let token_mint_address =
        Pubkey::from_str("5UUH9RTDiSpq6HKS6bp4NdU9PNJpXRXuiw6ShBTBhgH2").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    // Fetch real pool state from RPC
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");

    // Deserialize PumpSwap pool state
    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    // Fetch token vault accounts to get real reserves
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
        last_updated: current_timestamp(),
        base_reserve,
        quote_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager
        .inject_token(crate::tests::quote_tests::wsol_token())
        .await;

    // Inject the real token metadata
    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: false,
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

    // Test Buy: SOL -> Token
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let input_amount = 100_000_000; // 0.1 SOL

    let request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // WSOL
        output_token: token_mint_address.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount,
        slippage_bps: 100, // 1%
    };

    println!("Calling get_quote handler (Buy)...");
    let result =
        crate::api::handlers::get_quote(axum::extract::State(state.clone()), axum::Json(request))
            .await;

    match result {
        Ok(axum::Json(response)) => {
            println!("Got Buy Quote Response!");
            println!("Transaction Base64: {}", response.transaction);

            // Decode Transaction
            let tx_bytes = base64::engine::general_purpose::STANDARD
                .decode(&response.transaction)
                .expect("Failed to decode base64 transaction");

            let (transaction, _): (Transaction, usize) =
                bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard())
                    .expect("Failed to deserialize transaction");

            // Simulate Transaction
            println!("Simulating Buy transaction on-chain...");
            let simulation = rpc_client.simulate_transaction(&transaction).await;

            match simulation {
                Ok(sim_result) => {
                    println!("Buy Simulation Result: {:#?}", sim_result);

                    // Assert no errors
                    assert!(
                        sim_result.value.err.is_none(),
                        "Buy Simulation should succeed. Error: {:?}",
                        sim_result.value.err
                    );

                    // Assert compute units consumed
                    let units_consumed = sim_result
                        .value
                        .units_consumed
                        .expect("Should have units consumed");
                    assert!(units_consumed > 0, "Should consume compute units");

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
    use crate::api::dto::{QuoteRequest, QuoteResponse};
    use crate::api::AppState;
    use base64::Engine;
    use borsh::{BorshDeserialize, BorshSerialize};
    use solana_sdk::transaction::Transaction;
    use solana_client::rpc_config::RpcSimulateTransactionConfig;
    use solana_commitment_config::CommitmentConfig;
    use solana_sdk::instruction::Instruction;
    use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::types::{
        Pool as PumpSwapPoolRaw, POOL_SIZE,
    };

    let (pool_manager, config) = create_test_setup(vec!["pumpswap"]).await;

    // Real SOL-Token PumpSwap pool
    let pool_address = Pubkey::from_str("4w2cysotX6czaUGmmWg13hDpY4QEMG2CzeKYEQyK9Ama").unwrap();
    let token_mint_address =
        Pubkey::from_str("5UUH9RTDiSpq6HKS6bp4NdU9PNJpXRXuiw6ShBTBhgH2").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url.clone(),
    ));

    // Fetch real pool state from RPC
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch PumpSwap pool account");

    // Deserialize PumpSwap pool state
    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])
        .expect("Failed to deserialize PumpSwap pool");

    // Fetch token vault accounts to get real reserves
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
        last_updated: current_timestamp(),
        base_reserve,
        quote_reserve,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
    });

    pool_manager.inject_pool(pool_state).await;
    pool_manager
        .inject_token(crate::tests::quote_tests::wsol_token())
        .await;

    // Inject the real token metadata
    pool_manager
        .inject_token(Token {
            address: token_mint_address,
            symbol: Some("TOKEN".to_string()),
            name: Some("PumpSwap Token".to_string()),
            decimals: 6,
            is_token_2022: false,
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

    // -------------------------------------------------------------------------
    // Composite Simulation: Buy -> Sell (Atomic)
    // -------------------------------------------------------------------------

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    // 1. Get BUY Quote (SOL -> Token)
    let buy_input_amount = 100_000_000; // 0.1 SOL
    let buy_request = QuoteRequest {
        input_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        output_token: token_mint_address.to_string(),
        user_wallet: user_wallet_str.clone(),
        input_amount: buy_input_amount,
        slippage_bps: 100,
    };

    println!("Getting Buy Quote...");
    let buy_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(buy_request),
    ).await.expect("Buy request failed");

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
    // Sell half of what we bought
    let buy_output_amount: u64 = buy_response.output_amount;
    let sell_input_amount = buy_output_amount / 2;

    println!("Getting Sell Quote (Amount: {})...", sell_input_amount);
    let sell_request = QuoteRequest {
        input_token: token_mint_address.to_string(),
        output_token: "So11111111111111111111111111111111111111112".to_string(), // SOL
        user_wallet: user_wallet_str.clone(),
        input_amount: sell_input_amount,
        slippage_bps: 100,
    };

    let sell_result = crate::api::handlers::get_quote(
        axum::extract::State(state.clone()),
        axum::Json(sell_request),
    ).await.expect("Sell request failed");

    let sell_response = match sell_result {
        axum::Json(res) => res,
    };

    let sell_tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sell_response.transaction)
        .expect("Failed to decode sell transaction");
    
    let (sell_transaction, _): (Transaction, usize) =
        bincode::serde::decode_from_slice(&sell_tx_bytes, bincode::config::standard())
            .expect("Failed to deserialize sell transaction");

    // 3. Merge Instructions Helper check
    // Since we are running on mainnet fork basically (simulation), we need correct recent blockhash.
    let recent_blockhash = buy_transaction.message.recent_blockhash;

    // Helper to extract instructions
    let get_instructions = |tx: &Transaction| -> Vec<Instruction> {
        let message = &tx.message;
        message.instructions.iter().map(|ix| {
            Instruction {
                program_id: message.account_keys[ix.program_id_index as usize],
                accounts: ix.accounts.iter().map(|&acc_idx| {
                    let idx = acc_idx as usize;
                    let is_signer = idx < message.header.num_required_signatures as usize;
                    let is_writable = if is_signer {
                        idx < (message.header.num_required_signatures - message.header.num_readonly_signed_accounts) as usize
                    } else {
                        idx < (message.account_keys.len() - message.header.num_readonly_unsigned_accounts as usize)
                    };
                    
                    solana_sdk::instruction::AccountMeta {
                        pubkey: message.account_keys[idx],
                        is_signer,
                        is_writable,
                    }
                }).collect(),
                data: ix.data.clone(),
            }
        }).collect::<Vec<_>>()
    };

    let mut instructions = get_instructions(&buy_transaction);
    let sell_instructions = get_instructions(&sell_transaction);
    
    // Naively append sell instructions. IDEMPOTENT ATZ creation should handle duplicates.
    instructions.extend(sell_instructions);

    // 4. Build Atomic Transaction
    let composite_transaction = Transaction::new_with_payer(
        &instructions,
        Some(&payer),
    );
     // Update blockhash
    let mut composite_transaction = composite_transaction;
    composite_transaction.message.recent_blockhash = recent_blockhash;

    // 5. Simulate Atomic Transaction
    println!("Simulating Composite Buy -> Sell Transaction...");
    // Use sig_verify: false because we cannot sign for this user
    let config = RpcSimulateTransactionConfig {
        sig_verify: false, 
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::processed()),
        ..RpcSimulateTransactionConfig::default()
    };

    let simulation = rpc_client.simulate_transaction_with_config(&composite_transaction, config).await;

    match simulation {
        Ok(sim_result) => {
            println!("Composite Simulation Result: {:#?}", sim_result);
            
            if let Some(logs) = &sim_result.value.logs {
                for log in logs {
                    println!("  {}", log);
                }
            }

            // Assert no errors
            assert!(
                sim_result.value.err.is_none(),
                "Composite Simulation should succeed. Error: {:?}",
                sim_result.value.err
            );

            // Assert compute units consumed
            let units_consumed = sim_result
                .value
                .units_consumed
                .expect("Should have units consumed");
            assert!(units_consumed > 0, "Should consume compute units");

            println!(
                "✅ Composite PumpSwap Simulation Successful! Consumed {} compute units",
                units_consumed
            );
        }
        Err(e) => panic!("RPC Error during Composite simulation: {}", e),
    }
}
