# arbitrade-dex-eth - DEX Arbitrage Service

This service monitors Ethereum DEX pools via amm-eth and detects profitable arbitrage opportunities across multiple liquidity pools.

## Architecture

```
amm-eth (Pool Price Feed)
    ↓ [WebSocket: ws://localhost:8080]
arbitrade-dex-eth
    ├── DexWsClient (Connects & listens to price stream)
    ├── PriceCache (Stores prices by token, indexed by pool)
    ├── ArbitrageDetector (Compares pools, detects opportunities)
    └── ArbitrageExecutor (Executes trades on-chain)
```

## How It Works

### 1. **Price Collection**
- Connects to amm-eth WebSocket server
- Receives real-time Uniswap pool price updates
- Stores prices in `PriceCache` indexed by token address
- Each token can have multiple pool prices (V2, V3, cross-DEX)

### 2. **Opportunity Detection**
- Continuously scans all tokens with 2+ pools
- For each token, compares all pool pairs
- Calculates profit: `sell_price - buy_price`
- Filters by minimum thresholds:
  - `MIN_PROFIT_PERCENT`: Minimum % gain required (default: 2%)
  - `MIN_PRICE_DIFF_ETH`: Minimum ETH difference (default: 0.001 ETH)

### 3. **Trade Execution** (Planned)
- Identifies best profit opportunity
- Executes buy on lowest-price pool
- Executes sell on highest-price pool
- Accounts for gas costs and slippage
- Calculates net profit

## Components

### `types.rs`
Data structures for pool prices, arbitrage opportunities, and execution results:
- `PoolPrice`: Single pool's price for a token
- `DexArbitrageOpportunity`: Detected arbitrage between two pools
- `ExecutionResult`: Trade execution outcome

### `price_cache.rs`
Thread-safe in-memory price storage:
```rust
// Store prices from WebSocket
cache.update_price(pool_price);

// Get best prices for arbitrage
let buy_price = cache.get_best_buy_price(&token);
let sell_price = cache.get_best_sell_price(&token);

// Monitor cache statistics
let stats = cache.get_stats();
// Stats: unique_tokens, total_pools, tokens_with_multiple_pools
```

### `dex_ws_client.rs`
Connects to amm-eth WebSocket:
```rust
let client = DexWsClient::new("ws://localhost:8080".to_string());

// Listen for price updates
client.start(|pool_price| {
    println!("Price update: {}", pool_price);
}).await;

// Auto-reconnect with exponential backoff
client.start_with_reconnect(callback).await;
```

### `arbitrage_detector.rs`
Detects profitable opportunities:
```rust
let detector = ArbitrageDetector::new(
    cache,
    min_profit_percent: 2.0,
    min_price_diff_eth: 0.001,
);

// Find opportunities for a token
let opps = detector.find_opportunities(&token_address);

// Find all profitable opportunities
let all_opps = detector.find_all_opportunities();
```

### `executor.rs`
Executes trades on-chain (simulated in dry-run mode):
```rust
let executor = ArbitrageExecutor::new(provider, private_key, slippage, dry_run);

// Execute trade
let result = executor.execute(&opportunity).await;
```

## Setup & Configuration

### Environment Variables

```bash
# amm-eth connection
AMM_ETH_WS_URL=ws://localhost:8080

# Arbitrage detection thresholds
MIN_PROFIT_PERCENT=2.0              # Minimum % profit to consider
MIN_PRICE_DIFF_ETH=0.001            # Minimum ETH price difference

# Execution parameters
CHECK_INTERVAL_SECS=5               # How often to scan for opportunities

# Optional: Trade execution
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/...
ETH_WEBSOCKET_URL=wss://eth-mainnet.g.alchemy.com/v2/...
PRIVATE_KEY=0x...                   # Wallet private key
DRY_RUN=true                        # Set to false to execute real trades
SLIPPAGE_TOLERANCE=100              # Basis points (100 = 1%)
```

### Create `.env` file

```bash
cat > arbitrade-dex-eth/.env << 'EOF'
# Price feed
AMM_ETH_WS_URL=ws://localhost:8080

# Thresholds
MIN_PROFIT_PERCENT=2.0
MIN_PRICE_DIFF_ETH=0.001
CHECK_INTERVAL_SECS=5

# Logging
RUST_LOG=info

# For trade execution
# ETH_RPC_URL=
# PRIVATE_KEY=
# DRY_RUN=true
EOF
```

## Running the Service

### 1. **Start amm-eth first** (provides price feed)
```bash
# In one terminal
cd amm-eth
cargo run
# Should output: WebSocket server on 0.0.0.0:8080
```

### 2. **Start arbitrade-dex-eth** (listens to prices, detects arbitrage)
```bash
# In another terminal
cd arbitrade-dex-eth
cargo run
```

### Expected Output
```
🚀 Starting arbitrade-dex-eth service
📊 Configuration: min_profit=2%, min_diff=0.001 ETH, check_interval=5s
🔗 Connecting to amm-eth WebSocket: ws://localhost:8080
✅ WebSocket connected and listening for price updates
🎯 Starting arbitrage detection loop

[Waiting for prices...]

💰 Found 3 arbitrage opportunity(ies)
   🎯 ARB 0x1234...5678 - Buy@$1.50/ETH (UniswapV2), Sell@$1.53/ETH (UniswapV3) = 2.0% profit
   💵 USD Equivalent: $42.50
```

## Data Flow

```
1. amm-eth detects Uniswap swap events
   ↓
2. Updates pool prices, broadcasts via WebSocket
   ↓
3. arbitrade-dex-eth WebSocket client receives message
   ↓
4. PriceCache stores: token → [pool1_price, pool2_price, ...]
   ↓
5. ArbitrageDetector compares pools for same token
   ↓
6. If profit > threshold:
   ├─ Logs opportunity
   ├─ Stores in memory
   └─ Ready for execution
```

## Statistics

The service maintains statistics visible via logs:

```
Cache: 150 tokens, 420 pools
  - 47 tokens have multiple pools (arbitrage possible)
  - Avg 2.8 pools per token
```

## Performance

- **Price updates**: Received and cached in <1ms
- **Opportunity detection**: ~50-200ms for full scan
- **Memory usage**: ~50MB for 500+ pools
- **WebSocket latency**: Typically <100ms from event to detection

## Testing

### Unit Tests
```bash
cargo test -p arbitrade-dex-eth

# Test price cache
# Test arbitrage detection logic
# Test executor simulation
```

### Local Testing with Simulation
```bash
# Set DRY_RUN=true in .env
RUST_LOG=debug cargo run

# Will simulate opportunities without executing real trades
```

## Integration with arbitrade-eth

This service is complementary to `arbitrade-eth`:

| Feature | arbitrade-dex-eth | arbitrade-eth |
|---------|------------------|---------------|
| **Arbitrage Type** | DEX ↔ DEX | DEX ↔ CEX |
| **Price Source** | Uniswap pools | CEX APIs |
| **Capital Needed** | Deployed on-chain | Off-chain |
| **Speed** | <1s execution | Seconds (deposit) |
| **Complexity** | Single tx | Multi-step (buy/wait/sell) |

**Combined Strategy**: 
- arbitrade-dex-eth: Fast, on-chain profits
- arbitrade-eth: Larger, cross-venue opportunities

## Future Enhancements

- [ ] Smart contract for batch arbitrage execution
- [ ] Slippage estimation from pool reserves
- [ ] Gas optimization (batching trades)
- [ ] Multi-hop arbitrage (A→B→C→A)
- [ ] MEV protection
- [ ] Dashboard for monitoring opportunities
- [ ] API for external systems

## Troubleshooting

### "Failed to connect to WebSocket"
```bash
# Ensure amm-eth is running
ps aux | grep amm-eth

# Check if port 8080 is open
netstat -tlnp | grep 8080

# Start amm-eth
cd amm-eth && cargo run
```

### "WebSocket connection closed"
- amm-eth crashed or restarted
- Service will auto-reconnect with exponential backoff
- Check amm-eth logs

### No opportunities detected
- Pools don't have sufficient price difference
- Lower `MIN_PROFIT_PERCENT` threshold
- Add more pools to amm-eth configuration
- Check cache statistics: `Cache: X tokens, Y pools`

### High memory usage
- Adjust pool price history (currently keeps 100/token)
- Enable automatic pruning of old prices
- Monitor with: `cache.get_stats()`

## License

MIT
