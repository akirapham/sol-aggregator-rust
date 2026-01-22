use crate::aggregator::DexAggregator;
use crate::pool_manager::PoolStateManager;
use crate::types::{AggregatorConfig, ExecutionPriority, SwapParams, Token};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;

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
        name: Some("USDC".to_string()),
        decimals: 6,
        is_token_2022: false,
        logo_uri: None,
    }
}

pub fn test_token(mint: Pubkey) -> Token {
    Token {
        address: mint,
        symbol: Some("TEST".to_string()),
        name: Some("Test Token".to_string()),
        decimals: 6,
        is_token_2022: false,
        logo_uri: None,
    }
}

pub async fn create_test_setup(_protocols: Vec<&str>) -> (Arc<PoolStateManager>, AggregatorConfig) {
    let mut config = AggregatorConfig::default();
    config.rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    // Create mocks
    let mock_grpc = Arc::new(crate::tests::mocks::MockGrpcService);
    let mock_db = Arc::new(crate::tests::mocks::MockDatabase::new());
    let mock_price = Arc::new(crate::tests::mocks::MockPriceService::new(150.0)); // 150 SOL price
    let (arbitrage_tx, _) = tokio::sync::broadcast::channel(100);

    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        config.rpc_url.clone(),
    ));

    let pool_manager = Arc::new(
        PoolStateManager::new(
            mock_grpc,
            config.clone(),
            rpc_client,
            mock_price,
            arbitrage_tx,
            mock_db,
        )
        .await,
    );

    (pool_manager, config)
}

pub async fn verify_quote(
    pool_manager: Arc<PoolStateManager>,
    config: AggregatorConfig,
    input_token: Token,
    output_token: Token,
    input_amount: u64,
    expected_pool: Pubkey,
) {
    let aggregator = DexAggregator::new(config, pool_manager);

    let params = SwapParams {
        input_token: input_token.clone(),
        output_token: output_token.clone(),
        input_amount,
        slippage_bps: 50, // 0.5%
        user_wallet: Pubkey::new_unique(),
        priority: ExecutionPriority::High,
    };

    let result = aggregator.get_swap_route(&params).await;
    assert!(result.is_some(), "Failed to find quote");

    let route = result.unwrap();
    assert!(!route.paths.is_empty(), "Route has no paths");

    let first_path = &route.paths[0];
    assert!(!first_path.steps.is_empty(), "Path has no steps");

    let first_step = &first_path.steps[0];
    assert_eq!(
        first_step.pool_address, expected_pool,
        "Route selected wrong pool"
    );

    println!(
        "Quote verified: {} {} -> {} {} via {}",
        input_amount,
        input_token.symbol.unwrap_or_default(),
        route.output_amount,
        output_token.symbol.unwrap_or_default(),
        first_step.dex
    );
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct BondingCurveRaw {
    pub blob1: u64,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
}
