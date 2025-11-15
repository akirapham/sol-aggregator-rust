# Bitget CEX Price Provider

This module provides real-time cryptocurrency price data from Bitget exchange via WebSocket connections.

## Features

- **Public API Access**: No authentication required to access market data
- **Contract Address Filtering**: Automatically filters tokens by Ethereum or Solana contract addresses
- **WebSocket Real-time Updates**: Subscribe to live price updates via Bitget's WebSocket API
- **Multiple Connections**: Handles large numbers of symbols by splitting into multiple WebSocket connections
- **Orderbook Support**: Fetch orderbook depth to estimate sell outputs
- **Automatic Reconnection**: Handles connection failures with automatic retry

## Usage

### Basic Example

```rust
use cex_price_provider::bitget::BitgetService;
use cex_price_provider::{FilterAddressType, PriceProvider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create service filtered for Ethereum tokens
    let service = BitgetService::new(FilterAddressType::Ethereum);

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
cargo run --example bitget -p cex-price-provider

# With debug logging
RUST_LOG=cex_price_provider=debug cargo run --example bitget -p cex-price-provider
```

## API Endpoints Used

### REST API

1. **Get Symbols**: `/api/v2/spot/public/symbols`
   - Fetches all available spot trading pairs
   - Filters for USDT pairs with "online" status

2. **Get Coin Info**: `/api/v2/spot/public/coins?coin={SYMBOL}`
   - Retrieves contract addresses for each token
   - Provides chain information (Ethereum, Solana, etc.)
   - **No authentication required** (unlike some exchanges)

3. **Get Orderbook**: `/api/v2/spot/market/orderbook`
   - Fetches current orderbook for a symbol
   - Used for estimating sell outputs

### WebSocket API

- **URL**: `wss://ws.bitget.com/v2/ws/public`
- **Channels**: `ticker` channel for real-time price updates
- **Subscription Format**:
  ```json
  {
    "op": "subscribe",
    "args": [
      {
        "instType": "SPOT",
        "channel": "ticker",
        "instId": "BTCUSDT"
      }
    ]
  }
  ```

## How It Works

1. **Fetch Trading Pairs**: Gets all USDT spot pairs from Bitget
2. **Fetch Contract Addresses**: For each unique base currency, fetches chain information
   - Runs in parallel batches to avoid rate limits
   - 10 currencies per batch with 500ms delay between batches
3. **Filter by Contract Address**: Only subscribes to tokens with valid Ethereum/Solana addresses
4. **WebSocket Connections**: Creates multiple WebSocket connections (50 symbols per connection)
5. **Price Updates**: Receives and caches real-time price data
6. **Cache by Contract Address**: Uses contract addresses as cache keys for easy lookup

## Rate Limiting

Bitget has generous rate limits for public endpoints:
- **REST API**: 10 requests per batch with 500ms delay
- **WebSocket**: 50 symbols per connection to avoid overwhelming single connections

## Comparison with Other Exchanges

| Feature | Bitget | Bybit | KuCoin | MEXC |
|---------|--------|-------|--------|------|
| Contract Address API | ✅ Public | 🔒 Auth Required | ✅ Public | ✅ Public |
| WebSocket Protocol | JSON | JSON | JSON | Protobuf |
| Max Symbols/Connection | ~50 | ~100 | ~50 | ~15 |
| Ping Interval | 30s | Custom | Custom | 30s |
| Rate Limits | Moderate | Moderate | Strict | Moderate |

## Contract Address Validation

The service validates contract addresses based on the `FilterAddressType`:

- **Ethereum**: Validates ERC-20 addresses (0x + 40 hex characters)
- **Solana**: Validates using Solana SDK's Pubkey parser

Invalid addresses are filtered out automatically.

## Error Handling

- Graceful handling of rate limits with retry logic
- Automatic WebSocket reconnection on disconnection
- Detailed logging for debugging
- Continues operation even if some currencies fail to fetch

## Limitations

1. **Public API Only**: All data is from public endpoints (no private orders/balances)
2. **USDT Pairs Only**: Currently only subscribes to USDT trading pairs
3. **Spot Markets Only**: Does not include futures or perpetual markets
4. **Contract Address Required**: Only subscribes to tokens with valid contract addresses

## API Response Codes

- `00000`: Success
- Other codes indicate errors (see Bitget API documentation)

## Example Output

```
Starting Bitget Service...
Found 850 USDT trading pairs
Found 850 unique currencies to query
Fetching batch 1/85 (10 currencies)...
Mapped 245 trading pairs to contract addresses (12 filtered out as invalid)
Subscribing to 245 symbols
Starting 5 WebSocket connections
Total tokens with prices: 245
```
