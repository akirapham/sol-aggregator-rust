# Solana DEX Aggregator Documentation

A high-performance, intelligent DEX aggregator for Solana built in Rust with advanced smart routing capabilities.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Smart Routing Features](#smart-routing-features)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Examples](#examples)
- [Advanced Usage](#advanced-usage)
- [Performance](#performance)
- [Security](#security)

## Overview

The Solana DEX Aggregator is a sophisticated routing system that finds optimal swap routes across multiple decentralized exchanges on Solana. It uses advanced algorithms to minimize costs, reduce MEV risk, and maximize output amounts.

### Key Features

- 🚀 **Multi-DEX Support**: PumpFun, PumpFun Swap, Raydium, Raydium CPMM, Orca
- 🧠 **Smart Routing**: AI-powered route optimization
- 🛡️ **MEV Protection**: Advanced MEV risk assessment and mitigation
- ⚡ **High Performance**: Built in Rust for maximum speed
- 🔧 **Configurable**: Extensive configuration options
- 📊 **Analytics**: Comprehensive route analysis and reporting

## Architecture

### Core Components

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   DexAggregator │────│ SmartRoutingEngine│────│   DEX Modules   │
│                 │    │                  │    │                 │
│ • Configuration │    │ • Multi-hop      │    │ • PumpFun       │
│ • Route Finding │    │ • Split Trading  │    │ • Raydium       │
│ • Price Compare │    │ • MEV Protection │    │ • Orca          │
└─────────────────┘    │ • Arbitrage      │    └─────────────────┘
                       │ • Gas Optimization│
                       └──────────────────┘
```

### Smart Routing Engine

The `SmartRoutingEngine` is the core intelligence of the aggregator:

1. **Route Discovery**: Finds all possible routes across DEXs
2. **Optimization**: Applies advanced algorithms to find the best routes
3. **Risk Assessment**: Evaluates MEV risk and liquidity depth
4. **Cost Analysis**: Considers fees, gas costs, and price impact
5. **Route Selection**: Chooses optimal routes based on multiple criteria

## Smart Routing Features

### 1. Multi-Hop Routing

**What it does**: Finds complex routes like SOL → USDC → BONK instead of direct swaps.

**How it works**:
- Builds a graph of all token connections across DEXs
- Uses BFS algorithm to find all possible paths
- Evaluates each path for optimality
- Supports up to N hops (configurable)

**Example**:
```rust
// Direct route: SOL → BONK (if available)
// Multi-hop: SOL → USDC → BONK (often better)
```

### 2. Split Trading

**What it does**: Splits large trades across multiple DEXs to reduce price impact.

**How it works**:
- Identifies multiple viable routes for the same trade
- Calculates optimal split ratios
- Ensures minimum amounts per split
- Balances price impact across routes

**Example**:
```rust
// 1000 SOL swap split as:
// - 400 SOL on Raydium
// - 350 SOL on Orca  
// - 250 SOL on PumpFun Swap
```

### 3. MEV Protection

**What it does**: Assesses and mitigates MEV (Maximal Extractable Value) risks.

**Risk Levels**:
- **Low**: High liquidity, private mempool
- **Medium**: Moderate liquidity, standard mempool
- **High**: Low liquidity, public mempool
- **Critical**: Very low liquidity, high-value trades

**Protection Strategies**:
- Liquidity depth analysis
- Private mempool routing
- Flashloan protection
- Dynamic slippage adjustment

### 4. Gas Optimization

**What it does**: Optimizes gas costs while maintaining execution speed.

**Features**:
- Gas cost estimation for each route
- Priority fee configuration
- Speed vs cost optimization
- Gas limit management

### 5. Arbitrage Detection

**What it does**: Identifies cross-DEX arbitrage opportunities.

**How it works**:
- Compares prices across all DEXs
- Calculates potential profits
- Creates arbitrage routes
- Considers execution costs

## Configuration

### Environment Variables

The aggregator supports configuration via environment variables. Create a `.env` file:

```bash
# RPC Configuration
RPC_URL=https://api.mainnet-beta.solana.com
COMMITMENT_LEVEL=confirmed

# Basic Settings
MAX_SLIPPAGE=0.05
MAX_ROUTES=5

# Smart Routing
ENABLE_MULTI_HOP=true
ENABLE_SPLIT_TRADING=true
ENABLE_ARBITRAGE_DETECTION=true
MAX_HOPS=3
MIN_LIQUIDITY_THRESHOLD=1000000
PRICE_IMPACT_THRESHOLD=0.05

# Gas Configuration
MAX_GAS_PRICE=5000
PRIORITY_FEE=1000
GAS_LIMIT=200000
OPTIMIZE_FOR_SPEED=false

# MEV Protection
USE_PRIVATE_MEMPOOL=false
MAX_SLIPPAGE_TOLERANCE=0.01
MIN_LIQUIDITY_THRESHOLD_MEV=10000000
MAX_MEV_RISK_TOLERANCE=medium
USE_FLASHLOAN_PROTECTION=false

# Split Trading
MAX_SPLITS=3
MIN_SPLIT_AMOUNT=1000000
MAX_PRICE_IMPACT_PER_SPLIT=0.02
PREFER_LOW_MEV=true

# Enabled DEXs
ENABLE_PUMPFUN=true
ENABLE_PUMPFUN_SWAP=true
ENABLE_RAYDIUM=true
ENABLE_RAYDIUM_CPMM=true
ENABLE_ORCA=true
```

### Programmatic Configuration

```rust
use sol_agg_rust::{DexAggregator, AggregatorConfig, SmartRoutingConfig, GasConfig, MevProtectionConfig, SplitConfig, DexType, MevRisk};
use rust_decimal::Decimal;

let config = AggregatorConfig {
    rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
    commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
    max_slippage: Decimal::new(5, 2), // 5%
    max_routes: 5,
    enabled_dexs: vec![
        DexType::PumpFun,
        DexType::Raydium,
        DexType::Orca,
    ],
    smart_routing: SmartRoutingConfig {
        enable_multi_hop: true,
        enable_split_trading: true,
        enable_arbitrage_detection: true,
        max_hops: 3,
        min_liquidity_threshold: 1000000,
        price_impact_threshold: Decimal::new(5, 2),
        enable_route_simulation: true,
        enable_dynamic_slippage: true,
    },
    gas_config: GasConfig {
        max_gas_price: 5000,
        priority_fee: 1000,
        gas_limit: 200000,
        optimize_for_speed: false,
    },
    mev_protection: MevProtectionConfig {
        use_private_mempool: false,
        max_slippage_tolerance: Decimal::new(1, 2),
        min_liquidity_threshold: 10000000,
        max_mev_risk_tolerance: MevRisk::Medium,
        use_flashloan_protection: false,
    },
    split_config: SplitConfig {
        max_splits: 3,
        min_split_amount: 1000000,
        max_price_impact_per_split: Decimal::new(2, 2),
        prefer_low_mev: true,
    },
};
```

## API Reference

### Core Types

#### `DexAggregator`

Main aggregator class that coordinates between DEXs and smart routing.

```rust
pub struct DexAggregator {
    config: AggregatorConfig,
    dexs: HashMap<DexType, Box<dyn DexInterface + Send + Sync>>,
    smart_routing: SmartRoutingEngine,
}
```

#### `SwapParams`

Parameters for a swap operation.

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

#### `BestRoute`

Result of route finding with comprehensive analysis.

```rust
pub struct BestRoute {
    pub routes: Vec<SwapRoute>,
    pub total_input_amount: u64,
    pub total_output_amount: u64,
    pub total_fee: u64,
    pub total_price_impact: Decimal,
    pub execution_priority: ExecutionPriority,
    pub total_gas_cost: u64,
    pub estimated_execution_time_ms: u64,
    pub max_mev_risk: MevRisk,
    pub route_type: RouteType,
    pub split_ratio: Option<Vec<Decimal>>,
}
```

### Main Methods

#### `find_best_route(params: &SwapParams) -> Result<BestRoute>`

Finds the optimal route using smart routing algorithms.

```rust
let best_route = aggregator.find_best_route(&swap_params).await?;
```

#### `get_price_comparison(input_token, output_token, amount) -> Result<Vec<PriceInfo>>`

Gets price comparison across all DEXs.

```rust
let prices = aggregator.get_price_comparison(&sol_mint, &usdc_mint, 1000000000).await?;
```

#### `is_token_pair_supported(token_a, token_b) -> Result<bool>`

Checks if a token pair is supported by any DEX.

```rust
let supported = aggregator.is_token_pair_supported(&token_a, &token_b).await?;
```

## Examples

### Basic Usage

```rust
use sol_agg_rust::{DexAggregator, SwapParams, ExecutionPriority};
use solana_sdk::pubkey::Pubkey;
use rust_decimal::Decimal;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create aggregator with default configuration
    let aggregator = DexAggregator::new(AggregatorConfig::default());
    
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
    println!("Output: {} USDC", best_route.total_output_amount as f64 / 1e6);
    println!("Fee: {} lamports", best_route.total_fee);
    println!("Gas cost: {} lamports", best_route.total_gas_cost);
    println!("MEV risk: {:?}", best_route.max_mev_risk);
    
    Ok(())
}
```

### Advanced Configuration

```rust
use sol_agg_rust::*;

// Load configuration from environment
let config = AggregatorConfig::from_env()?;

// Create aggregator with custom configuration
let aggregator = DexAggregator::new(config);

// Use the aggregator...
```

## Advanced Usage

### Custom DEX Integration

To add a new DEX, implement the `DexInterface` trait:

```rust
use async_trait::async_trait;
use crate::dex::traits::DexInterface;

pub struct MyCustomDex {
    // Your DEX implementation
}

#[async_trait]
impl DexInterface for MyCustomDex {
    fn get_dex_type(&self) -> DexType {
        DexType::MyCustomDex
    }
    
    async fn get_best_route(&self, params: &SwapParams) -> Result<Option<SwapRoute>> {
        // Implement your DEX logic
    }
    
    // Implement other required methods...
}
```

### Custom Route Scoring

You can customize how routes are scored by modifying the `calculate_route_score` method in `SmartRoutingEngine`:

```rust
fn calculate_route_score(&self, route: &SwapRoute, params: &SwapParams) -> f64 {
    let output_score = route.output_amount as f64 / params.input_amount as f64;
    let fee_penalty = route.fee as f64 / params.input_amount as f64;
    let gas_penalty = route.gas_cost as f64 / 1000000.0;
    let mev_penalty = match route.mev_risk {
        MevRisk::Low => 0.0,
        MevRisk::Medium => 0.1,
        MevRisk::High => 0.3,
        MevRisk::Critical => 0.5,
    };
    let liquidity_bonus = (route.liquidity_depth as f64 / 1000000000.0).min(1.0);

    // Your custom scoring logic here
    output_score - fee_penalty - gas_penalty - mev_penalty + liquidity_bonus
}
```

## Performance

### Benchmarks

- **Route Discovery**: < 100ms for 5 DEXs
- **Multi-hop Analysis**: < 500ms for 3-hop paths
- **Price Comparison**: < 200ms across all DEXs
- **Memory Usage**: < 50MB for typical workloads

### Optimization Tips

1. **Enable only needed DEXs** to reduce query time
2. **Set appropriate max_hops** based on your needs
3. **Use private mempools** for MEV-sensitive trades
4. **Configure gas limits** based on network conditions

## Security

### MEV Protection

The aggregator provides several MEV protection mechanisms:

1. **Liquidity Analysis**: Avoids low-liquidity pools
2. **Private Mempools**: Routes through private channels
3. **Flashloan Protection**: Detects and avoids flashloan attacks
4. **Dynamic Slippage**: Adjusts slippage based on market conditions

### Best Practices

1. **Always verify routes** before execution
2. **Use appropriate slippage tolerance**
3. **Monitor MEV risk levels**
4. **Test with small amounts first**
5. **Keep configuration updated**

### Risk Assessment

The aggregator provides comprehensive risk assessment:

- **MEV Risk**: Low, Medium, High, Critical
- **Liquidity Risk**: Based on pool depth
- **Execution Risk**: Based on gas costs and timing
- **Price Impact**: Calculated for each route

## Troubleshooting

### Common Issues

1. **No routes found**: Check if token pairs are supported
2. **High MEV risk**: Increase liquidity thresholds
3. **Slow execution**: Enable speed optimization
4. **High gas costs**: Adjust gas configuration

### Debug Mode

Enable debug logging:

```rust
env_logger::Builder::from_default_env()
    .filter_level(log::LevelFilter::Debug)
    .init();
```

### Error Handling

The aggregator provides detailed error information:

```rust
match aggregator.find_best_route(&params).await {
    Ok(route) => println!("Route found: {:?}", route),
    Err(DexAggregatorError::RouteNotFound) => println!("No routes available"),
    Err(DexAggregatorError::InsufficientLiquidity) => println!("Not enough liquidity"),
    Err(e) => println!("Error: {}", e),
}
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Implement your changes
4. Add tests
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Support

For questions and support:
- Create an issue on GitHub
- Join our Discord community
- Check the documentation wiki
