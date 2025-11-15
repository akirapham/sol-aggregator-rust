# CEX Price Provider - Complete Guide

This crate provides real-time cryptocurrency price streaming from multiple centralized exchanges (CEX), with support for filtering by blockchain contract addresses.

## Supported Exchanges

| Exchange | Auth Required | Contract Addresses | Status |
|----------|---------------|-------------------|--------|
| **Bitget** | ❌ No | ✅ Public API | ✅ Ready |
| **Bybit** | ✅ Yes | ⚠️ Requires Auth | ✅ Ready |
| **Gate.io** | ❌ No | ✅ Public API | ✅ Ready |
| **KuCoin** | ❌ No | ✅ Public API | ✅ Ready |
| **MEXC** | ❌ No | ✅ Public API | ✅ Ready |

## Quick Start

### 1. MEXC (Easiest - No Auth)

```rust
use cex_price_provider::mexc::MexcService;
use cex_price_provider::FilterAddressType;

let service = MexcService::new(FilterAddressType::Ethereum);
tokio::spawn(async move { service.start().await });
```

**Run Example:**
```bash
cargo run --example mexc -p cex-price-provider
```

### 2. KuCoin (No Auth - Most Flexible)

```rust
use cex_price_provider::kucoin::KucoinService;
use cex_price_provider::FilterAddressType;

let service = KucoinService::new(FilterAddressType::Ethereum);
tokio::spawn(async move { service.start().await });
```

**Run Example:**
```bash
cargo run --example kucoin -p cex-price-provider
```

### 3. Bybit (Requires API Key)

```rust
use cex_price_provider::bybit::BybitService;
use cex_price_provider::FilterAddressType;

let service = BybitService::with_credentials(
    FilterAddressType::Ethereum,
    "your_api_key".to_string(),
    "your_api_secret".to_string(),
);
tokio::spawn(async move { service.start().await });
```

**Run Example:**
```bash
# Set environment variables
export BYBIT_API_KEY="your_key"
export BYBIT_API_SECRET="your_secret"

cargo run --example bybit -p cex-price-provider
```

## Architecture

```
┌─────────────┐
│  Your App   │
└──────┬──────┘
       │
       ├──────► MexcService ──► MEXC WebSocket ──► Price Cache (by contract)
       │
       ├──────► KucoinService ──► KuCoin WebSocket ──► Price Cache (by contract)
       │
       └──────► BybitService ──► Bybit WebSocket ──► Price Cache (by contract)
```

## Common Interface: PriceProvider Trait

All services implement the same `PriceProvider` trait:

```rust
#[async_trait]
pub trait PriceProvider {
    /// Get price for a single token (by contract address or symbol)
    async fn get_price(&self, key: &str) -> Option<TokenPrice>;

    /// Get all cached prices
    async fn get_all_prices(&self) -> Vec<TokenPrice>;

    /// Get prices for multiple tokens
    async fn get_prices(&self, keys: &Vec<String>) -> Vec<Option<TokenPrice>>;

    /// Start the service (connects WebSocket, fetches data)
    async fn start(&self) -> Result<()>;
}
```

## Filtering by Contract Address

### Ethereum Contracts

```rust
let service = MexcService::new(FilterAddressType::Ethereum);
```

This will:
1. Fetch all trading pairs
2. Get contract addresses for each token
3. Validate addresses (40-char hex)
4. Only subscribe to tokens with valid Ethereum addresses
5. Cache prices using contract addresses as keys

### Solana Contracts

```rust
let service = KucoinService::new(FilterAddressType::Solana);
```

This will:
1. Fetch all trading pairs
2. Get contract addresses for each token
3. Validate addresses (base58 format)
4. Only subscribe to tokens with valid Solana addresses
5. Cache prices using contract addresses as keys

## Price Query Examples

### By Contract Address (Recommended)

```rust
// Ethereum contract address
let price = service.get_price("0x1234...abcd").await;

// Solana contract address
let price = service.get_price("So11111...").await;
```

### By Symbol (Fallback)

```rust
// Works if contract address not available
let price = service.get_price("BTC").await;
```

### Batch Query

```rust
let addresses = vec![
    "0x1234...".to_string(),
    "0x5678...".to_string(),
];
let prices = service.get_prices(&addresses).await;
```

## Advanced Features

### Orderbook Depth Analysis

All services support estimating sell outputs using orderbook depth:

```rust
// Estimate USDT received for selling tokens
let usdt_output = service.estimate_sell_output(
    "0x1234...", // Contract address
    10.5,        // Amount to sell
).await?;

println!("Would receive: ${:.2} USDT", usdt_output);
```

### Live Monitoring

```rust
use tokio::time::{interval, Duration};

let mut ticker = interval(Duration::from_secs(30));
loop {
    ticker.tick().await;

    let all_prices = service.get_all_prices().await;
    println!("Tracking {} tokens", all_prices.len());

    if let Some(btc) = service.get_price("0x123...").await {
        println!("BTC: ${:.2}", btc.price);
    }
}
```

## Error Handling & Resilience

All services include:
- ✅ **Automatic reconnection** on WebSocket failures
- ✅ **Exponential backoff** for retries
- ✅ **Rate limit handling** with delays
- ✅ **Invalid address filtering** (logged but not fatal)
- ✅ **Graceful degradation** (works without contract addresses)

## Logging & Debugging

### Info Level (Default)

```bash
cargo run --example mexc -p cex-price-provider
```

Shows:
- Service start/stop
- Number of pairs found
- Contract addresses mapped
- Periodic statistics

### Debug Level

```bash
RUST_LOG=debug cargo run --example mexc -p cex-price-provider
```

Shows additionally:
- Raw API responses
- Address validation results
- WebSocket message details
- Price update events

### Specific Module

```bash
RUST_LOG=cex_price_provider::mexc=debug cargo run --example mexc -p cex-price-provider
```

## Exchange Comparison

### Feature Matrix

| Feature | MEXC | KuCoin | Bybit |
|---------|------|--------|-------|
| **Setup Complexity** | ⭐ Easy | ⭐ Easy | ⭐⭐⭐ Medium |
| **Auth Required** | ❌ No | ❌ No | ✅ Yes |
| **Contract Addresses** | ✅ Public | ✅ Public | ⚠️ Auth only |
| **WebSocket Stability** | ⭐⭐⭐ Good | ⭐⭐⭐⭐ Excellent | ⭐⭐⭐⭐ Excellent |
| **API Rate Limits** | Moderate | Generous | Generous |
| **Trading Pairs** | ~1500 | ~800 | ~600 |
| **Orderbook Depth** | 20 levels | 100 levels | 200 levels |

### When to Use Each

**Use MEXC when:**
- ✅ You want the simplest setup
- ✅ You need the most trading pairs
- ✅ You don't want to manage API keys

**Use KuCoin when:**
- ✅ You want deep orderbook data
- ✅ You need reliable WebSocket connections
- ✅ You prefer well-documented APIs

**Use Bybit when:**
- ✅ You already have API credentials
- ✅ You need advanced trading features
- ✅ You want institutional-grade reliability

## Performance Considerations

### Memory Usage

Each service caches prices in memory using `DashMap`:
- Small footprint: ~100 bytes per token
- Thread-safe concurrent access
- Automatic cleanup of stale prices

### Network Usage

- **REST API**: Called once at startup (~1-5 MB)
- **WebSocket**: Continuous stream (~10 KB/s)
- **Multiple connections**: For large symbol lists (>100)

### CPU Usage

- Minimal: JSON parsing + HashMap lookups
- No heavy computations
- Async/await for I/O efficiency

## Testing

### Unit Tests

```bash
cargo test -p cex-price-provider
```

### Integration Tests

```bash
# Test MEXC (no setup needed)
cargo run --example mexc -p cex-price-provider

# Test KuCoin (no setup needed)
cargo run --example kucoin -p cex-price-provider

# Test Bybit (requires API key)
export BYBIT_API_KEY="..."
export BYBIT_API_SECRET="..."
cargo run --example bybit -p cex-price-provider
```

## Production Deployment

### Recommended Setup

```rust
use cex_price_provider::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    // Start multiple CEX services
    let mexc = Arc::new(MexcService::new(FilterAddressType::Ethereum));
    let kucoin = Arc::new(KucoinService::new(FilterAddressType::Ethereum));

    // Start services in background
    let mexc_clone = mexc.clone();
    tokio::spawn(async move { mexc_clone.start().await });

    let kucoin_clone = kucoin.clone();
    tokio::spawn(async move { kucoin_clone.start().await });

    // Wait for initialization
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Use services
    loop {
        let price = mexc.get_price("0x123...").await
            .or_else(|| kucoin.get_price("0x123...").await);

        if let Some(p) = price {
            println!("Price: ${}", p.price);
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
```

### Health Monitoring

```rust
// Check if services are healthy
if mexc.get_all_prices().await.len() < 10 {
    log::warn!("MEXC service may be unhealthy");
}
```

## Troubleshooting

### No Prices Showing Up

1. Check logs for connection errors
2. Verify network connectivity
3. Ensure contract addresses are valid
4. Wait longer (can take 10-30 seconds to populate)

### Bybit Authentication Errors

1. Verify API key has read permissions
2. Check API secret is correct
3. Ensure timestamp is synchronized
4. Review signature generation logs

### Rate Limiting Issues

1. Reduce number of symbols
2. Increase delays between requests
3. Use multiple API keys (if supported)
4. Contact exchange for limit increases

## API Documentation

- [MEXC API Docs](https://mexcdevelop.github.io/apidocs/)
- [KuCoin API Docs](https://www.kucoin.com/docs)
- [Bybit API Docs](https://bybit-exchange.github.io/docs/)

## Support

For issues or questions:
1. Check the exchange-specific README files
2. Review example code
3. Enable debug logging
4. Open an issue on GitHub

## License

MIT
