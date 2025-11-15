# Real-Time Solana DEX Aggregator

A high-performance Solana DEX aggregator with real-time pool data updates, smart routing, and Jupiter-like functionality supporting Orca, Raydium, PumpFun, and other major DEXs.

## Features

- 🔄 **Real-time Pool Updates**: Live pool state management with WebSocket and Yellowstone gRPC support
- 🧠 **Smart Routing**: Intelligent route optimization across multiple DEXs
- ⚡ **High Performance**: In-memory pool state management for ultra-fast route calculations
- 🛡️ **MEV Protection**: Built-in protection against sandwich attacks and MEV exploitation
- 🎯 **Multi-DEX Support**: Orca, Raydium AMM, Raydium CPMM, PumpFun, and PumpFun Swap
- 📊 **Advanced Analytics**: Pool statistics, volume tracking, and performance metrics
- 🔀 **Split Trading**: Intelligent order splitting across multiple pools for better prices
- 🏗️ **Modular Architecture**: Clean, extensible design for easy integration

## Supported DEXs

| DEX | Status | Pool Types | Fee Structure |
|-----|---------|------------|---------------|
| **Orca** | ✅ Implemented | AMM, Whirlpools | 0.03% - 1% |
| **Raydium** | ✅ Implemented | AMM, CPMM | 0.01% - 0.25% |
| **PumpFun** | ✅ Implemented | Bonding Curves | 1% |
| **Jupiter** | 🔄 Planned | Aggregator | Variable |
| **Meteora** | 🔄 Planned | Dynamic Pools | Variable |

## Architecture

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Data Sources  │    │  Pool Manager    │    │   DEX Modules   │
│                 │    │                  │    │                 │
│ • RPC Nodes     │───▶│ • In-memory      │◀───│ • Orca          │
│ • WebSocket     │    │   pool state     │    │ • Raydium       │
│ • Yellowstone   │    │ • Real-time      │    │ • PumpFun       │
│   gRPC          │    │   updates        │    │ • Custom DEXs   │
└─────────────────┘    └──────────────────┘    └─────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Smart Routing Engine                         │
│                                                                 │
│ • Route optimization          • Price impact calculation        │
│ • Multi-hop routing          • Gas estimation                  │
│ • Split order execution      • MEV protection                  │
│ • Arbitrage detection        • Slippage management             │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        DEX Aggregator                           │
│                                                                 │
│ • Best route finding         • Transaction building            │
│ • Multi-DEX comparison       • Execution priority              │
│ • Risk assessment           • Performance monitoring           │
└─────────────────────────────────────────────────────────────────┘
```

## Quick Start

### 1. Basic Setup

```rust
use sol_agg_rust::{
    DexAggregator, PoolStateManager, RealTimeDataFetcher,
    AggregatorConfig, DataFetcherConfig, SwapParams, DexType,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create pool state manager
    let pool_manager = Arc::new(PoolStateManager::new());

    // Configure real-time data fetcher
    let data_config = DataFetcherConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        fetch_interval_ms: 5000,
        enable_websocket: true,
        ..Default::default()
    };

    // Start data fetcher
    let mut data_fetcher = RealTimeDataFetcher::new(data_config, pool_manager.clone());
    data_fetcher.start().await?;

    // Configure aggregator
    let config = AggregatorConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        enabled_dexs: vec![DexType::Orca, DexType::Raydium, DexType::PumpFun],
        max_slippage: rust_decimal::Decimal::new(1, 2), // 1%
        ..Default::default()
    };

    // Create aggregator
    let aggregator = DexAggregator::new_with_pool_manager(config, pool_manager);

    Ok(())
}
```

### 2. Finding Best Routes

```rust
use solana_sdk::pubkey::Pubkey;
use rust_decimal::Decimal;

// Define swap parameters
let swap_params = SwapParams {
    input_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse()?, // USDC
    output_token: "So11111111111111111111111111111111111111112".parse()?, // SOL
    input_amount: 1_000_000_000, // 1000 USDC
    slippage_tolerance: Decimal::new(1, 2), // 1%
    user_wallet: user_wallet_pubkey,
    priority: ExecutionPriority::Medium,
};

// Find best route
match aggregator.get_best_route(&swap_params).await? {
    Some(route) => {
        println!("Best route found!");
        println!("Output: {} tokens", route.total_output_amount);
        println!("Price impact: {:.2}%", route.total_price_impact * 100);
        println!("Fee: {} lamports", route.total_fee);
        
        for (i, dex_route) in route.routes.iter().enumerate() {
            println!("Route {}: {} - {} tokens", i + 1, dex_route.dex, dex_route.output_amount);
        }
    }
    None => println!("No route found"),
}
```

### 3. Real-time Pool Monitoring

```rust
// Get pool statistics
let stats = pool_manager.get_stats().await;
println!("Total pools: {}", stats.total_pools);
println!("Pools by DEX: {:?}", stats.pools_by_dex);

// Get best pools for a token pair
let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse()?;
let sol = "So11111111111111111111111111111111111111112".parse()?;

let best_pools = pool_manager.get_best_pools_for_pair(&usdc, &sol, 5).await;
for pool in best_pools {
    println!("Pool: {} - Liquidity: ${:.2}k", 
        pool.dex, 
        pool.liquidity_usd.unwrap_or(0.0) / 1000.0
    );
}
```

## Configuration

### Aggregator Configuration

```rust
let config = AggregatorConfig {
    rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
    commitment: CommitmentLevel::Confirmed,
    max_slippage: Decimal::new(1, 2), // 1%
    max_routes: 5,
    enabled_dexs: vec![
        DexType::Orca,
        DexType::Raydium,
        DexType::RaydiumCpmm,
        DexType::PumpFun,
    ],
    smart_routing: SmartRoutingConfig {
        enabled: true,
        max_hops: 3,
        min_liquidity_threshold: 100_000_000, // $100k
        price_impact_threshold: Decimal::new(5, 2), // 5%
        use_ai_optimization: false,
        prefer_low_mev: true,
    },
    gas_config: GasConfig {
        max_gas_price: 1_000_000,
        priority_fee: 10_000,
        gas_limit: 200_000,
        optimize_for_speed: false,
    },
    mev_protection: MevProtectionConfig {
        use_private_mempool: false,
        max_slippage_tolerance: Decimal::new(2, 2), // 2%
        min_liquidity_threshold: 50_000_000,
        max_mev_risk_tolerance: MevRisk::Medium,
        use_flashloan_protection: false,
    },
    split_config: SplitConfig {
        max_splits: 3,
        min_split_amount: 1_000_000,
        max_price_impact_per_split: Decimal::new(1, 2), // 1%
        prefer_low_mev: true,
    },
};
```

### Data Fetcher Configuration

```rust
let data_config = DataFetcherConfig {
    rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
    yellowstone_grpc_url: Some("https://grpc.mainnet.solana.com".to_string()),
    fetch_interval_ms: 3000, // 3 seconds
    max_pools_per_fetch: 1000,
    enable_websocket: true,
    commitment: "confirmed".to_string(),
};
```

## Performance Optimization

### 1. RPC Configuration

For optimal performance, use:
- **Dedicated RPC nodes** for reduced latency
- **GenesysGo** or **Triton** for high-performance RPC
- **Multiple RPC endpoints** for redundancy

### 2. Memory Management

The aggregator uses in-memory pool state management:
- **Pool data**: ~1KB per pool
- **1000 pools**: ~1MB memory usage
- **10000 pools**: ~10MB memory usage

### 3. Update Frequency

Recommended update frequencies:
- **High-frequency trading**: 500ms - 1s
- **Regular trading**: 3s - 5s
- **Portfolio management**: 10s - 30s

## Advanced Features

### Multi-hop Routing

```rust
// Enable multi-hop routing for better prices
let config = SmartRoutingConfig {
    max_hops: 3, // Allow up to 3 hops (A -> B -> C -> D)
    min_liquidity_threshold: 50_000_000,
    ..Default::default()
};
```

### Split Order Execution

```rust
// Configure split trading for large orders
let split_config = SplitConfig {
    max_splits: 4, // Split across up to 4 different pools
    min_split_amount: 5_000_000, // Minimum 5M base units per split
    max_price_impact_per_split: Decimal::new(15, 3), // 1.5% max impact per split
    prefer_low_mev: true,
};
```

### MEV Protection

```rust
let mev_config = MevProtectionConfig {
    use_private_mempool: true, // Use private mempool when available
    max_slippage_tolerance: Decimal::new(15, 3), // 1.5% max slippage
    min_liquidity_threshold: 100_000_000, // $100k minimum liquidity
    max_mev_risk_tolerance: MevRisk::Low, // Conservative MEV tolerance
    use_flashloan_protection: true,
};
```

## Production Deployment

### 1. Infrastructure Requirements

- **CPU**: 4+ cores for real-time processing
- **Memory**: 8GB+ for pool state management
- **Network**: Low-latency connection to Solana RPCs
- **Storage**: SSD for transaction logs and analytics

### 2. Monitoring and Alerting

```rust
// Monitor pool manager statistics
let stats = pool_manager.get_stats().await;
if stats.total_pools < expected_minimum {
    alert_system.send_alert("Low pool count detected").await;
}

// Monitor route quality
if route.total_price_impact > Decimal::new(5, 2) {
    log::warn!("High price impact route: {:.2}%", route.total_price_impact * 100);
}
```

### 3. Error Handling

```rust
// Robust error handling for production
match aggregator.get_best_route(&params).await {
    Ok(Some(route)) => {
        // Execute route
    }
    Ok(None) => {
        // No route found - handle gracefully
        log::warn!("No route found for {:?}", params);
    }
    Err(DexAggregatorError::InsufficientLiquidity) => {
        // Handle insufficient liquidity
    }
    Err(DexAggregatorError::RpcError(e)) => {
        // Handle RPC errors with retry logic
        retry_with_backoff(|| aggregator.get_best_route(&params)).await?;
    }
    Err(e) => {
        // Handle other errors
        log::error!("Unexpected error: {:?}", e);
    }
}
```

## Examples

Run the provided examples to see the aggregator in action:

```bash
# Basic usage example
cargo run --example basic_usage

# Real-time aggregator with live data
cargo run --example realtime_aggregator

# Complete integration example
cargo run --example complete_integration

# Environment configuration example
cargo run --example env_config_usage
```

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Adding New DEX Support

To add support for a new DEX:

1. Implement the `DexInterface` trait
2. Add parsing logic for the DEX's pool accounts
3. Update the `DexType` enum
4. Add integration tests

Example:

```rust
pub struct MyCustomDex {
    rpc_client: Arc<RpcClient>,
    pool_manager: Arc<PoolStateManager>,
    program_id: Pubkey,
}

#[async_trait]
impl DexInterface for MyCustomDex {
    fn get_dex_type(&self) -> DexType {
        DexType::MyCustom
    }

    async fn get_pools(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<Vec<PoolInfo>> {
        // Implementation
    }

    // ... other required methods
}
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Disclaimer

This software is provided "as is" without warranty. Trading cryptocurrencies involves risk, and you should carefully consider your investment objectives and risk tolerance. The developers are not responsible for any financial losses incurred through the use of this software.

## Support

- 📖 [Documentation](docs/)
- 🐛 [Issue Tracker](https://github.com/yourusername/sol-agg-rust/issues)
- 💬 [Discussions](https://github.com/yourusername/sol-agg-rust/discussions)
- 📧 [Email Support](mailto:support@yourdomain.com)
