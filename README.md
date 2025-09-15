# Solana DEX Aggregator

A high-performance DEX aggregator for Solana built in Rust that finds the best swap routes across multiple decentralized exchanges.

## Supported DEXs

- **PumpFun** - Bonding curve-based token launches
- **PumpFun Swap** - Traditional AMM for PumpFun tokens
- **Raydium** - Popular AMM with multiple pool types
- **Raydium CPMM** - Concentrated liquidity pools
- **Orca** - User-friendly AMM with Whirlpools

## Features

- 🚀 **High Performance** - Built in Rust for maximum speed and efficiency
- 🔍 **Route Optimization** - Finds the best routes across all supported DEXs
- 💰 **Price Comparison** - Compare prices across different DEXs
- ⚡ **Async/Await** - Non-blocking operations for better performance
- 🛡️ **Error Handling** - Comprehensive error handling and logging
- 🔧 **Configurable** - Customizable settings for different use cases
- 📊 **Price Impact** - Calculates price impact and slippage
- 🎯 **Slippage Protection** - Built-in slippage tolerance controls

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
sol-agg-rust = "0.1.0"
```

## Quick Start

```rust
use sol_agg_rust::{DexAggregator, AggregatorConfig, SwapParams, ExecutionPriority};
use solana_sdk::pubkey::Pubkey;
use rust_decimal::Decimal;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create aggregator configuration
    let config = AggregatorConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
        max_slippage: Decimal::new(5, 2), // 5%
        max_routes: 3,
        enabled_dexs: vec![
            sol_agg_rust::DexType::PumpFun,
            sol_agg_rust::DexType::Raydium,
            sol_agg_rust::DexType::Orca,
        ],
    };
    
    // Create the aggregator
    let aggregator = DexAggregator::new(config);
    
    // Define swap parameters
    let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112")?;
    let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")?;
    
    let swap_params = SwapParams {
        input_token: sol_mint,
        output_token: usdc_mint,
        input_amount: 1000000000, // 1 SOL
        slippage_tolerance: Decimal::new(1, 2), // 1%
        user_wallet: Pubkey::new_unique(),
        priority: ExecutionPriority::Medium,
    };
    
    // Find the best route
    let best_route = aggregator.find_best_route(&swap_params).await?;
    
    println!("Best route found!");
    println!("Total output: {} USDC", best_route.total_output_amount);
    println!("Total fee: {} lamports", best_route.total_fee);
    
    Ok(())
}
```

## Configuration

The `AggregatorConfig` struct allows you to customize the aggregator behavior:

```rust
let config = AggregatorConfig {
    rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
    commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
    max_slippage: Decimal::new(5, 2), // 5% max slippage
    max_routes: 5, // Maximum number of routes to return
    enabled_dexs: vec![
        DexType::PumpFun,
        DexType::Raydium,
        DexType::Orca,
    ],
};
```

## API Reference

### DexAggregator

The main aggregator class that coordinates between different DEXs.

#### Methods

- `find_best_route(params: &SwapParams) -> Result<BestRoute>` - Find the best swap route
- `get_price_comparison(input_token, output_token, amount) -> Result<Vec<PriceInfo>>` - Compare prices across DEXs
- `get_all_supported_tokens() -> Result<Vec<Token>>` - Get all supported tokens
- `is_token_pair_supported(token_a, token_b) -> Result<bool>` - Check if a token pair is supported

### SwapParams

Parameters for a swap operation:

```rust
pub struct SwapParams {
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub input_amount: u64,
    pub slippage_tolerance: Decimal,
    pub user_wallet: Pubkey,
    pub priority: ExecutionPriority,
}
```

### BestRoute

The result of finding the best route:

```rust
pub struct BestRoute {
    pub routes: Vec<SwapRoute>,
    pub total_input_amount: u64,
    pub total_output_amount: u64,
    pub total_fee: u64,
    pub total_price_impact: Decimal,
    pub execution_priority: ExecutionPriority,
}
```

## Examples

Run the basic usage example:

```bash
cargo run --example basic_usage
```

## Error Handling

The library provides comprehensive error handling through the `DexAggregatorError` enum:

```rust
pub enum DexAggregatorError {
    DexError(String),
    InvalidTokenAddress(String),
    InsufficientLiquidity,
    PriceCalculationError(String),
    RouteNotFound,
    RpcError(String),
    SerializationError(String),
    NetworkError(reqwest::Error),
    SolanaError(solana_client::client_error::ClientError),
    AnchorError(anchor_client::anchor_lang::error::Error),
}
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Disclaimer

This software is for educational and research purposes. Always verify the code and use at your own risk when dealing with real funds.
