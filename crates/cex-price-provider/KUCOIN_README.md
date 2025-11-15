# KuCoin Integration for CEX Price Provider

This document explains how to use the KuCoin price provider service to fetch real-time cryptocurrency prices filtered by contract addresses.

## Key Feature: No Authentication Required!

Unlike Bybit, **KuCoin provides contract address information through PUBLIC API endpoints** - no API credentials needed! This makes it much easier to use for contract address filtering.

## Features

- ✅ **Real-time price streaming** via WebSocket
- ✅ **Contract address filtering** (Ethereum or Solana)
- ✅ **NO authentication required** for contract address access
- ✅ **Orderbook depth analysis** for sell estimation
- ✅ **Automatic reconnection** on connection failures
- ✅ **Multiple concurrent WebSocket connections** for scalability

## Architecture

```
KuCoin Public API (REST)
  ↓
Get all USDT trading pairs
  ↓
For each currency, fetch contract addresses (PUBLIC endpoint)
  ↓
Filter by valid Ethereum/Solana addresses
  ↓
Subscribe to WebSocket ticker streams
  ↓
Cache prices by contract address
```

## Usage

### Basic Usage (No Auth Needed!)

```rust
use cex_price_provider::kucoin::KucoinService;
use cex_price_provider::{FilterAddressType, PriceProvider};

#[tokio::main]
async fn main() -> Result<()> {
    // Create service - filters for Ethereum contracts
    let service = KucoinService::new(FilterAddressType::Ethereum);

    // Start the service (fetches contract addresses and connects WebSocket)
    tokio::spawn(async move {
        service.start().await
    });

    // Wait for prices to populate
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Query by contract address
    if let Some(price) = service.get_price("0x123...").await {
        println!("Token price: ${}", price.price);
    }

    Ok(())
}
```

### Running the Example

```bash
# From workspace root
cargo run --example kucoin -p cex-price-provider

# With debug logging
RUST_LOG=debug cargo run --example kucoin -p cex-price-provider
```

## How Contract Address Filtering Works

1. **Fetch Trading Pairs**: Get all USDT spot markets from KuCoin
2. **Fetch Contract Addresses**: For each base currency, call `/api/v3/currencies/{currency}` (PUBLIC endpoint)
3. **Filter by Chain**: Extract contract addresses for Ethereum or Solana chains
4. **Validate Addresses**: Check if addresses are valid (40-char hex for ETH, base58 for Solana)
5. **Build Mappings**:
   - `symbol → contract_address`
   - `contract_address → symbol`
6. **Subscribe**: Only subscribe to WebSocket feeds for tokens with valid contract addresses
7. **Cache by Contract**: Store prices using contract addresses as keys

## API Endpoints Used

### REST API (Public)

- **`GET /api/v1/symbols`** - Get all trading pairs
- **`GET /api/v3/currencies`** - Get list of all currencies (optional)
- **`GET /api/v3/currencies/{currency}`** - Get currency details with contract addresses per chain
- **`GET /api/v1/market/orderbook/level2_{depth}?symbol={symbol}`** - Get orderbook

### WebSocket API (Public)

- **`POST /api/v1/bullet-public`** - Get WebSocket connection token
- **`wss://ws-api-spot.kucoin.com/?token={token}`** - WebSocket endpoint
- **Topic: `/market/ticker:{symbol}`** - Real-time ticker updates

## Comparison: KuCoin vs Bybit vs MEXC

| Feature | KuCoin | Bybit | MEXC |
|---------|--------|-------|------|
| Contract addresses via API | ✅ Public | ⚠️ Requires Auth | ✅ Public |
| API Key needed | ❌ No | ✅ Yes | ❌ No |
| Ethereum support | ✅ Yes | ✅ Yes | ✅ Yes |
| Solana support | ✅ Yes | ✅ Yes | ✅ Yes |
| WebSocket reconnect | ✅ Auto | ✅ Auto | ✅ Auto |
| Orderbook access | ✅ Public | ✅ Public | ✅ Public |

## Response Structures

### Currency Detail Response

```json
{
  "code": "200000",
  "data": {
    "currency": "BTC",
    "name": "Bitcoin",
    "chains": [
      {
        "chainName": "BTC",
        "contractAddress": "",
        "isWithdrawEnabled": true,
        "isDepositEnabled": true
      },
      {
        "chainName": "ERC20",
        "contractAddress": "0x123...",
        "isWithdrawEnabled": true,
        "isDepositEnabled": true
      }
    ]
  }
}
```

### WebSocket Ticker Message

```json
{
  "type": "message",
  "topic": "/market/ticker:BTC-USDT",
  "subject": "trade.ticker",
  "data": {
    "sequence": "1234567890",
    "price": "45000.50",
    "size": "0.5",
    "bestAsk": "45001.00",
    "bestAskSize": "1.2",
    "bestBid": "45000.00",
    "bestBidSize": "0.8"
  }
}
```

## Rate Limiting

KuCoin implements rate limiting:
- **REST API**: ~200 requests per 10 seconds (public endpoints)
- **WebSocket**: Max 100 subscriptions per connection

The implementation includes:
- 100ms delay between currency detail requests
- Multiple WebSocket connections for large symbol lists
- Automatic retry with exponential backoff

## Error Handling

The service handles:
- ✅ Network disconnections (auto-reconnect)
- ✅ Invalid contract addresses (filtered out)
- ✅ API errors (logged with context)
- ✅ WebSocket timeouts (ping/pong)
- ✅ Rate limiting (delays between requests)

## Advanced Features

### Sell Order Estimation

```rust
// Estimate USDT output for selling tokens
let usdt_amount = service.estimate_sell_output(
    "0x123...", // Contract address
    1.5,        // Amount to sell
).await?;

println!("Would receive: ${:.2} USDT", usdt_amount);
```

### Statistics Monitoring

The service logs statistics every 60 seconds:
- Number of tokens with active prices
- Number of contract addresses mapped
- WebSocket connection status

## Debugging

Enable debug logging to see:
- Raw API responses from KuCoin
- Contract address validation results
- WebSocket connection details
- Price update events

```bash
RUST_LOG=cex_price_provider=debug cargo run --example kucoin -p cex-price-provider
```

## Notes

- **No API credentials required** - KuCoin's currency detail endpoint is public
- Contract addresses are fetched during service startup (may take a few seconds)
- 100ms delay between contract address requests to respect rate limits
- Only tokens with valid contract addresses are subscribed to
- Prices are cached using contract addresses as keys
- WebSocket automatically handles ping/pong for connection keepalive

## Future Improvements

- [ ] Cache contract address mappings to disk
- [ ] Parallel contract address fetching (with rate limit respect)
- [ ] Support for other quote currencies (BTC, ETH)
- [ ] Historical price data integration
- [ ] Trade execution support (requires authentication)
