# Ethereum Uniswap Swap Listener

This module listens to Ethereum Uniswap (V2, V3, V4) swap events via WebSocket and computes token prices in real-time.

## Features

- **WebSocket Connection**: Connects to Ethereum network via WebSocket (Alchemy, Infura, etc.)
- **Multi-Version Support**: Listens to Uniswap V2, V3, and V4 swap events
- **Real-time Price Calculation**: Computes token prices from swap amounts
- **In-Memory Storage**: Uses DashMap for concurrent price storage
- **ETH/USD Pricing**: Calculates USD prices for tokens paired with WETH

## Setup

1. Copy `.env.example` to `.env`:
```bash
cp .env.example .env
```

2. Configure your Ethereum WebSocket URL:
```env
ETH_WEBSOCKET_URL=wss://eth-mainnet.g.alchemy.com/v2/your-api-key
```

**Note**: ETH price is now automatically fetched from Binance WebSocket in real-time. No need to manually configure `ETH_PRICE_USD`.

## Running

```bash
cargo run -p amm-eth
```

## Architecture

### Components

- **`listener.rs`**: Main WebSocket listener that subscribes to swap events
- **`price_store.rs`**: Concurrent in-memory storage for token prices
- **`types.rs`**: Data structures for swap events and prices
- **`main.rs`**: Entry point that starts the listener

### Price Calculation

**Uniswap V2** - Uses Sync Events:
1. Listens to `Sync(uint112 reserve0, uint112 reserve1)` events
2. These events are emitted after every swap with updated reserves
3. Calculates price: `price_token0 = reserve1 / reserve0`
4. Token pairs are cached in memory to avoid redundant contract calls

**Uniswap V3** - Uses sqrtPriceX96:
1. Listens to `Swap(...)` events that include `sqrtPriceX96`
2. Converts sqrtPriceX96 to actual price: `price = (sqrtPriceX96 / 2^96)^2`
3. This gives the precise price of token0 in terms of token1
4. Token pairs are fetched once and cached

**Price Storage**:
- If paired with WETH, calculates ETH price
- If ETH_PRICE_USD is set, calculates USD price
- Stores with timestamp and DEX version

### Event Handling

**Uniswap V2**:
- Event: `Sync(uint112,uint112)` - provides updated reserves
- Advantage: More accurate than calculating from swap amounts
- Price calculation: Direct ratio of reserves

**Uniswap V3**:
- Event: `Swap(address,address,int256,int256,uint160,uint128,int24)`
- Uses `sqrtPriceX96` field for precise price calculation
- Handles concentrated liquidity positions correctly

**Uniswap V4**:
- Future support (when deployed)

### Optimizations

- **Token Pair Caching**: Pool token addresses are cached in a `DashMap` to avoid repeated contract calls
- **Efficient Price Updates**: Prices updated directly from event data without additional RPC calls
- **Concurrent Processing**: Uses async/await for non-blocking event processing

## Usage Example

```rust
use amm_eth::{EthConfig, EthSwapListener, PriceStore};

#[tokio::main]
async fn main() -> Result<()> {
    let config = EthConfig::default();
    let price_store = PriceStore::new();

    let listener = EthSwapListener::new(config, price_store.clone()).await?;

    // Get prices
    let token_address = "0x...".parse()?;
    if let Some(price) = price_store.get_price(&token_address) {
        println!("Price: {} ETH", price.price_in_eth);
    }

    listener.start().await?;
    Ok(())
}
```

## API

### PriceStore

- `update_price(token, price)`: Store/update token price
- `get_price(token)`: Get price for a token
- `get_all_prices()`: Get all stored prices
- `len()`: Number of tokens tracked
- `log_stats()`: Log statistics by DEX version

### TokenPrice

```rust
pub struct TokenPrice {
    pub token_address: Address,
    pub price_in_eth: f64,
    pub price_in_usd: Option<f64>,
    pub last_updated: u64,
    pub pool_address: Address,
    pub dex_version: DexVersion,
}
```

## Dependencies

- **ethers**: Ethereum library for WebSocket and contract interactions
- **tokio**: Async runtime
- **dashmap**: Concurrent HashMap for price storage
- **log/env_logger**: Logging
