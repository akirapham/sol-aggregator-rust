# Gate.io CEX Price Provider

This module provides real-time cryptocurrency price data from Gate.io exchange via WebSocket connections.

## Features

- **Public API Access**: No authentication required to access market data
- **Contract Address Filtering**: Automatically filters tokens by Ethereum or Solana contract addresses
- **WebSocket Real-time Updates**: Subscribe to live price updates via Gate.io's WebSocket API v4
- **Multiple Connections**: Handles large numbers of symbols by splitting into multiple WebSocket connections
- **Orderbook Support**: Fetch orderbook depth to estimate sell outputs
- **Automatic Reconnection**: Handles connection failures with automatic retry

## Usage

### Basic Example

```rust
use cex_price_provider::gate::GateService;
use cex_price_provider::{FilterAddressType, PriceProvider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create service filtered for Ethereum tokens
    let service = GateService::new(FilterAddressType::Ethereum);

    // Start WebSocket connections in background
    tokio::spawn(async move {
        service.start().await
    });

    // Wait for data to populate
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Get all prices
    let prices = service.get_all_prices().await;
    println!("Tracking {} tokens", prices.len());

    Ok(())
}
```

### Running the Example

```bash
# From workspace root
cargo run --example gate -p cex-price-provider

# With debug logging
RUST_LOG=cex_price_provider=debug cargo run --example gate -p cex-price-provider
```

## API Endpoints Used

### REST API

1. **Get Currency Pairs**: `/api/v4/spot/currency_pairs`
   - Fetches all available spot trading pairs
   - Filters for USDT pairs with "tradable" status
   - Returns pair ID (e.g., "BTC_USDT"), base, and quote currencies

2. **Get Currency Chains**: `/api/v4/wallet/currency_chains?currency={SYMBOL}`
   - Retrieves contract addresses for each token across different chains
   - Provides chain information (ETH, SOL, etc.)
   - Includes enabled/disabled status
   - **No authentication required** (public endpoint)

3. **Get Orderbook**: `/api/v4/spot/order_book`
   - Fetches current orderbook for a currency pair
   - Used for estimating sell outputs

### WebSocket API

- **URL**: `wss://api.gateio.ws/ws/v4/`
- **Channel**: `spot.tickers` for real-time price updates
- **Ping Interval**: 15 seconds
- **Subscription Format**:
  ```json
  {
    "time": 1633699200,
    "channel": "spot.tickers",
    "event": "subscribe",
    "payload": ["BTC_USDT", "ETH_USDT"]
  }
  ```

## How It Works

1. **Fetch Trading Pairs**: Gets all USDT spot pairs from Gate.io
2. **Fetch Currency Chains**: For each unique base currency, fetches chain information
   - Runs in parallel batches to avoid rate limits
   - 10 currencies per batch with 500ms delay between batches
3. **Filter by Contract Address**: Only subscribes to tokens with valid Ethereum/Solana addresses
4. **WebSocket Connections**: Creates multiple WebSocket connections (100 symbols per connection)
5. **Price Updates**: Receives and caches real-time price data
6. **Cache by Contract Address**: Uses contract addresses as cache keys for easy lookup

## Rate Limiting

Gate.io has moderate rate limits for public endpoints:
- **REST API**: 10 requests per batch with 500ms delay
- **WebSocket**: 100 symbols per connection (Gate.io can handle many symbols per connection)
- **Ping**: Required every 15 seconds to keep connection alive

## Comparison with Other Exchanges

| Feature | Gate.io | Bitget | Bybit | KuCoin | MEXC |
|---------|---------|--------|-------|--------|------|
| Contract Address API | ✅ Public | ✅ Public | 🔒 Auth Required | ✅ Public | ✅ Public |
| WebSocket Protocol | JSON v4 | JSON v2 | JSON | JSON | Protobuf |
| Max Symbols/Connection | ~100 | ~50 | ~100 | ~50 | ~15 |
| Ping Interval | 15s | 30s | Custom | Custom | 30s |
| Rate Limits | Moderate | Moderate | Moderate | Strict | Moderate |

## Contract Address Validation

The service validates contract addresses based on the `FilterAddressType`:

- **Ethereum**: Validates ERC-20 addresses (0x + 40 hex characters)
  - Matches chain names: "ETH", "ERC20"
- **Solana**: Validates using Solana SDK's Pubkey parser
  - Matches chain names: "SOL", "SOLANA"

Invalid addresses are filtered out automatically.

## WebSocket Message Types

### Subscribe Response
```json
{
  "time": 1633699200,
  "channel": "spot.tickers",
  "event": "subscribe",
  "result": {
    "status": "success"
  }
}
```

### Ticker Update
```json
{
  "time": 1633699201,
  "channel": "spot.tickers",
  "event": "update",
  "result": {
    "currency_pair": "BTC_USDT",
    "last": "50000.5",
    "change_percentage": "2.5"
  }
}
```

### Pong Response
```json
{
  "time": 1633699202,
  "channel": "spot.pong"
}
```

## Error Handling

- Graceful handling of rate limits with retry logic
- Automatic WebSocket reconnection on disconnection
- Detailed logging for debugging
- Continues operation even if some currencies fail to fetch
- Validates chain data and filters out disabled chains

## Limitations

1. **Public API Only**: All data is from public endpoints (no private orders/balances)
2. **USDT Pairs Only**: Currently only subscribes to USDT trading pairs
3. **Spot Markets Only**: Does not include futures or perpetual markets
4. **Contract Address Required**: Only subscribes to tokens with valid contract addresses
5. **Chain Format**: Chain names must match expected patterns (ETH, SOL, etc.)

## Performance Characteristics

- **Startup Time**: ~30-60 seconds (depends on number of currencies to query)
- **Memory Usage**: Moderate (caches all price data in memory)
- **CPU Usage**: Low (async WebSocket processing)
- **Network**: Multiple concurrent connections for optimal throughput

## Example Output

```
Starting Gate.io Service...
Found 1200 USDT trading pairs
Found 1200 unique currencies to query
Fetching batch 1/120 (10 currencies)...
Fetching batch 2/120 (10 currencies)...
...
Found contract addresses for 680 currencies
Mapped 320 trading pairs to contract addresses (45 filtered out as invalid)
Subscribing to 320 symbols
Starting 4 WebSocket connections
Connection 0: Subscribed to 100 symbols
Connection 1: Subscribed to 100 symbols
Connection 2: Subscribed to 100 symbols
Connection 3: Subscribed to 20 symbols
Total tokens with prices: 320
```

## Debugging

Enable debug logging to see detailed information:

```bash
RUST_LOG=cex_price_provider=debug cargo run --example gate -p cex-price-provider
```

Debug output includes:
- API request/response details
- WebSocket message content
- Contract address validation results
- Price update events
- Connection status
