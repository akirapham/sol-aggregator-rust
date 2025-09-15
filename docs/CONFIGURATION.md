# Configuration Guide

This guide explains how to configure the Solana DEX Aggregator using environment variables.

## Quick Start

1. Copy the example configuration file:
   ```bash
   cp .env.example .env
   ```

2. Edit the `.env` file with your preferred settings:
   ```bash
   nano .env
   ```

3. Run the aggregator:
   ```bash
   cargo run --example env_config_usage
   ```

## Configuration Categories

### 1. RPC Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `RPC_URL` | `https://api.mainnet-beta.solana.com` | Solana RPC endpoint |
| `COMMITMENT_LEVEL` | `confirmed` | Commitment level: `processed`, `confirmed`, or `finalized` |

**Examples:**
```bash
# Mainnet
RPC_URL=https://api.mainnet-beta.solana.com

# Devnet
RPC_URL=https://api.devnet.solana.com

# Custom RPC (Helius, QuickNode, etc.)
RPC_URL=https://your-custom-rpc-endpoint.com
```

### 2. Basic Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `MAX_SLIPPAGE` | `0.05` | Maximum slippage tolerance (5%) |
| `MAX_ROUTES` | `5` | Maximum number of routes to return |

**Examples:**
```bash
# Conservative slippage (1%)
MAX_SLIPPAGE=0.01

# Aggressive slippage (10%)
MAX_SLIPPAGE=0.10

# Return more routes
MAX_ROUTES=10
```

### 3. Smart Routing Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ENABLE_MULTI_HOP` | `true` | Enable multi-hop routing (A→B→C) |
| `ENABLE_SPLIT_TRADING` | `true` | Enable split trading across DEXs |
| `ENABLE_ARBITRAGE_DETECTION` | `true` | Enable arbitrage detection |
| `MAX_HOPS` | `3` | Maximum number of hops |
| `MIN_LIQUIDITY_THRESHOLD` | `1000000` | Minimum liquidity (lamports) |
| `PRICE_IMPACT_THRESHOLD` | `0.05` | Price impact threshold (5%) |
| `ENABLE_ROUTE_SIMULATION` | `true` | Enable route simulation |
| `ENABLE_DYNAMIC_SLIPPAGE` | `true` | Enable dynamic slippage |

**Examples:**
```bash
# Disable multi-hop for faster execution
ENABLE_MULTI_HOP=false

# Allow more hops for complex routes
MAX_HOPS=5

# Require higher liquidity
MIN_LIQUIDITY_THRESHOLD=10000000

# Stricter price impact
PRICE_IMPACT_THRESHOLD=0.02
```

### 4. Gas Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `MAX_GAS_PRICE` | `5000` | Maximum gas price (lamports) |
| `PRIORITY_FEE` | `1000` | Priority fee (lamports) |
| `GAS_LIMIT` | `200000` | Gas limit (compute units) |
| `OPTIMIZE_FOR_SPEED` | `false` | Optimize for speed vs cost |

**Examples:**
```bash
# Higher gas for faster execution
MAX_GAS_PRICE=10000
PRIORITY_FEE=2000

# Optimize for speed
OPTIMIZE_FOR_SPEED=true

# Lower gas for cost savings
MAX_GAS_PRICE=2000
PRIORITY_FEE=500
```

### 5. MEV Protection Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `USE_PRIVATE_MEMPOOL` | `false` | Use private mempool |
| `MAX_SLIPPAGE_TOLERANCE` | `0.01` | Max slippage for MEV protection |
| `MIN_LIQUIDITY_THRESHOLD_MEV` | `10000000` | Min liquidity for MEV protection |
| `MAX_MEV_RISK_TOLERANCE` | `medium` | Max MEV risk: `low`, `medium`, `high`, `critical` |
| `USE_FLASHLOAN_PROTECTION` | `false` | Use flashloan protection |

**Examples:**
```bash
# Conservative MEV protection
MAX_MEV_RISK_TOLERANCE=low
MIN_LIQUIDITY_THRESHOLD_MEV=50000000

# Aggressive MEV protection
USE_PRIVATE_MEMPOOL=true
USE_FLASHLOAN_PROTECTION=true

# Allow higher MEV risk
MAX_MEV_RISK_TOLERANCE=high
```

### 6. Split Trading Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `MAX_SPLITS` | `3` | Maximum number of splits |
| `MIN_SPLIT_AMOUNT` | `1000000` | Minimum amount per split |
| `MAX_PRICE_IMPACT_PER_SPLIT` | `0.02` | Max price impact per split |
| `PREFER_LOW_MEV` | `true` | Prefer low MEV routes |

**Examples:**
```bash
# More splits for large trades
MAX_SPLITS=5
MIN_SPLIT_AMOUNT=500000

# Stricter price impact per split
MAX_PRICE_IMPACT_PER_SPLIT=0.01

# Don't prefer low MEV (faster execution)
PREFER_LOW_MEV=false
```

### 7. DEX Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ENABLE_PUMPFUN` | `true` | Enable PumpFun DEX |
| `ENABLE_PUMPFUN_SWAP` | `true` | Enable PumpFun Swap DEX |
| `ENABLE_RAYDIUM` | `true` | Enable Raydium DEX |
| `ENABLE_RAYDIUM_CPMM` | `true` | Enable Raydium CPMM DEX |
| `ENABLE_ORCA` | `true` | Enable Orca DEX |

**Examples:**
```bash
# Only use major DEXs
ENABLE_PUMPFUN=false
ENABLE_PUMPFUN_SWAP=false

# Use only concentrated liquidity DEXs
ENABLE_RAYDIUM=false
ENABLE_ORCA=false
ENABLE_RAYDIUM_CPMM=true
```

## Configuration Profiles

### Conservative Profile
For users who prioritize safety and low risk:

```bash
# Conservative settings
MAX_SLIPPAGE=0.01
MAX_MEV_RISK_TOLERANCE=low
MIN_LIQUIDITY_THRESHOLD=50000000
PRICE_IMPACT_THRESHOLD=0.02
USE_PRIVATE_MEMPOOL=true
```

### Aggressive Profile
For users who prioritize speed and maximum output:

```bash
# Aggressive settings
MAX_SLIPPAGE=0.10
MAX_MEV_RISK_TOLERANCE=high
MIN_LIQUIDITY_THRESHOLD=1000000
PRICE_IMPACT_THRESHOLD=0.10
OPTIMIZE_FOR_SPEED=true
```

### Balanced Profile
For users who want a balance of safety and performance:

```bash
# Balanced settings (default)
MAX_SLIPPAGE=0.05
MAX_MEV_RISK_TOLERANCE=medium
MIN_LIQUIDITY_THRESHOLD=10000000
PRICE_IMPACT_THRESHOLD=0.05
```

## Environment-Specific Configurations

### Development
```bash
RPC_URL=https://api.devnet.solana.com
COMMITMENT_LEVEL=processed
MAX_ROUTES=3
ENABLE_ARBITRAGE_DETECTION=false
```

### Staging
```bash
RPC_URL=https://api.mainnet-beta.solana.com
COMMITMENT_LEVEL=confirmed
MAX_ROUTES=5
ENABLE_ARBITRAGE_DETECTION=true
```

### Production
```bash
RPC_URL=https://your-production-rpc.com
COMMITMENT_LEVEL=finalized
MAX_ROUTES=10
USE_PRIVATE_MEMPOOL=true
ENABLE_METRICS=true
```

## Validation

The configuration loader validates all values and provides helpful error messages:

```rust
// Example error handling
match AggregatorConfig::from_env() {
    Ok(config) => println!("Configuration loaded successfully"),
    Err(e) => {
        eprintln!("Configuration error: {}", e);
        eprintln!("Check your .env file for invalid values");
    }
}
```

## Override with Environment Variables

You can override any configuration value by setting environment variables:

```bash
# Override specific values
export MAX_SLIPPAGE=0.02
export ENABLE_MULTI_HOP=false
export MAX_MEV_RISK_TOLERANCE=low

# Run the aggregator
cargo run --example env_config_usage
```

## Best Practices

1. **Start with defaults**: Use the default configuration first
2. **Test changes**: Test configuration changes with small amounts
3. **Monitor performance**: Watch for changes in execution time and success rate
4. **Use profiles**: Create different profiles for different use cases
5. **Document changes**: Keep track of configuration changes and their effects
6. **Validate settings**: Always validate configuration before production use

## Troubleshooting

### Common Issues

1. **Invalid decimal values**: Use proper decimal format (e.g., `0.05` not `5%`)
2. **Invalid boolean values**: Use `true`/`false`, `1`/`0`, `yes`/`no`, or `on`/`off`
3. **Invalid MEV risk**: Use `low`, `medium`, `high`, or `critical`
4. **Invalid commitment level**: Use `processed`, `confirmed`, or `finalized`

### Debug Mode

Enable debug logging to see configuration loading:

```bash
RUST_LOG=debug cargo run --example env_config_usage
```

### Configuration Validation

The aggregator validates all configuration values and provides detailed error messages for invalid settings.
