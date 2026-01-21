use std::str::FromStr;
use crate::types::Token;
use solana_sdk::pubkey::Pubkey;
use solana_client::rpc_client::RpcClient;

/// Helper to create RPC client from environment
fn create_rpc_client() -> RpcClient {
    dotenvy::dotenv().ok();
    let rpc_url = std::env::var("RPC_URL")
        .expect("RPC_URL must be set in .env for integration tests");
    RpcClient::new(rpc_url)
}

/// Helper to create test tokens
#[allow(dead_code)]
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

#[allow(dead_code)]
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

// Note: Full integration tests with PoolStateManager require:
// - GrpcService instance
// - BinancePriceStream instance  
// - Database connection
// - Arbitrage pool broadcast channel
//
// These are better suited for end-to-end tests in a separate test binary
// that can set up the full application context.
//
// For now, these tests verify that the known pool addresses are valid
// and exist on-chain, which is the foundation for quote calculations.
