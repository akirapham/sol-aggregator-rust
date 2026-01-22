use crate::aggregator::DexAggregator;
use crate::pool_data_types::*;
use crate::pool_manager::PoolStateManager;
use crate::tests::quotes::common::*;
use crate::types::Token;
use borsh::BorshDeserialize;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::common::current_timestamp;
use solana_streamer_sdk::streaming::event_parser::protocols::bonk::types::PoolState as BonkPoolStateRaw;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::test]
async fn test_bonk_quote() {
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
        last_updated: u64::MAX,
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

#[tokio::test]
async fn test_bonk_quote_reverse() {
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
        last_updated: u64::MAX,
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
