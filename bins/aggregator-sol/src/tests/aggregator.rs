use super::mocks::{MockDatabase, MockGrpcService, MockPriceService};
use crate::aggregator::DexAggregator;
use crate::pool_data_types::{DexType, PoolState};
use crate::pool_manager::{PoolDataProvider, PoolStateManager};
use crate::types::{AggregatorConfig, ExecutionPriority, SwapParams, Token};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
/// Comprehensive integration tests that fetch real pool data and compute quotes
use std::str::FromStr;
use std::sync::Arc;

/// Helper to create RPC client from environment
fn create_rpc_client() -> RpcClient {
    dotenvy::dotenv().ok();
    let rpc_url =
        std::env::var("RPC_URL").expect("RPC_URL must be set in .env for integration tests");
    RpcClient::new(rpc_url)
}

/// Helper to create test tokens
fn create_wsol_token() -> Token {
    Token {
        address: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(),
        symbol: Some("SOL".to_string()),
        name: Some("Wrapped SOL".to_string()),
        decimals: 9,
        is_token_2022: false,
        logo_uri: None,
    }
}

fn create_usdc_token() -> Token {
    Token {
        address: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
        symbol: Some("USDC".to_string()),
        name: Some("USD Coin".to_string()),
        decimals: 6,
        is_token_2022: false,
        logo_uri: None,
    }
}

// Known pool addresses for testing different DEX types
mod test_pools {
    use super::*;

    // Raydium AMM V4: SOL-USDC
    pub fn raydium_sol_usdc() -> Pubkey {
        Pubkey::from_str("58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2").unwrap()
    }

    // Raydium CLMM: SOL-USDC
    pub fn raydium_clmm_sol_usdc() -> Pubkey {
        Pubkey::from_str("61R1ndXxvsWXXkWSyNkCxnzwd3zUNB8Q2ibmkiLPC8ht").unwrap()
    }

    // Orca Whirlpool: SOL-USDC
    pub fn orca_whirlpool_sol_usdc() -> Pubkey {
        Pubkey::from_str("HJPjoWUrhoZzkNfRpHuieeFk9WcZWjwy6PBjZ81ngndJ").unwrap()
    }

    // Meteora DLMM: SOL-USDC
    pub fn meteora_dlmm_sol_usdc() -> Pubkey {
        Pubkey::from_str("Bz1kKXV74cznsVJSu4cPcdrD2ZbCv6raez9Bq5Edmtgw").unwrap()
    }
}

#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored --test-threads=1
async fn test_raydium_amm_v4_pool_exists() {
    let rpc_client = create_rpc_client();
    let pool_address = test_pools::raydium_sol_usdc();

    // Verify pool account exists on-chain
    let account = rpc_client.get_account(&pool_address);
    assert!(account.is_ok(), "Raydium AMM V4 pool account should exist");

    let account = account.unwrap();
    assert!(account.data.len() > 0, "Pool account should have data");

    println!("✓ Raydium AMM V4 pool verified: {}", pool_address);
    println!("  Account owner: {}", account.owner);
    println!("  Data length: {} bytes", account.data.len());
}

#[tokio::test]
#[ignore]
async fn test_raydium_clmm_pool_exists() {
    let rpc_client = create_rpc_client();
    let pool_address = test_pools::raydium_clmm_sol_usdc();

    let account = rpc_client.get_account(&pool_address);
    assert!(account.is_ok(), "Raydium CLMM pool account should exist");

    let account = account.unwrap();
    assert!(account.data.len() > 0, "Pool account should have data");

    println!("✓ Raydium CLMM pool verified: {}", pool_address);
    println!("  Account owner: {}", account.owner);
    println!("  Data length: {} bytes", account.data.len());
}

#[tokio::test]
#[ignore]
async fn test_orca_whirlpool_pool_exists() {
    let rpc_client = create_rpc_client();
    let pool_address = test_pools::orca_whirlpool_sol_usdc();

    let account = rpc_client.get_account(&pool_address);
    assert!(account.is_ok(), "Orca Whirlpool pool account should exist");

    let account = account.unwrap();
    assert!(account.data.len() > 0, "Pool account should have data");

    println!("✓ Orca Whirlpool pool verified: {}", pool_address);
    println!("  Account owner: {}", account.owner);
    println!("  Data length: {} bytes", account.data.len());
}

#[tokio::test]
#[ignore]
async fn test_meteora_dlmm_pool_exists() {
    let rpc_client = create_rpc_client();
    let pool_address = test_pools::meteora_dlmm_sol_usdc();

    let account = rpc_client.get_account(&pool_address);
    assert!(account.is_ok(), "Meteora DLMM pool account should exist");

    let account = account.unwrap();
    assert!(account.data.len() > 0, "Pool account should have data");

    println!("✓ Meteora DLMM pool verified: {}", pool_address);
    println!("  Account owner: {}", account.owner);
    println!("  Data length: {} bytes", account.data.len());
}

#[tokio::test]
#[ignore]
async fn test_all_known_pools_exist() {
    let rpc_client = create_rpc_client();

    let pools = vec![
        ("Raydium AMM V4", test_pools::raydium_sol_usdc()),
        ("Raydium CLMM", test_pools::raydium_clmm_sol_usdc()),
        ("Orca Whirlpool", test_pools::orca_whirlpool_sol_usdc()),
        ("Meteora DLMM", test_pools::meteora_dlmm_sol_usdc()),
    ];

    println!("\nVerifying {} known pools...\n", pools.len());

    for (name, address) in pools {
        let account = rpc_client.get_account(&address);
        assert!(account.is_ok(), "{} pool should exist", name);

        let account = account.unwrap();
        println!("✓ {}: {}", name, address);
        println!("  Owner: {}", account.owner);
        println!("  Data: {} bytes\n", account.data.len());
    }
}

// TODO: Comprehensive quote calculation tests
// These require implementing trait-based dependency injection in PoolStateManager
// to allow mocking GrpcService, Database, and PriceService.
//
// Once implemented, tests will:
// 1. Create PoolStateManager with mock dependencies
// 2. Fetch real pool data from RPC
// 3. Populate pool manager with real pool states
// 4. Call DexAggregator::get_swap_route_with_exclude
// 5. Verify quote calculations are correct
// 6. Test pool exclusion logic
