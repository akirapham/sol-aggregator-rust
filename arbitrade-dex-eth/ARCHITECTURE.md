# arbitrade-dex-eth Architecture Diagrams

## System Architecture

```
┌──────────────────────────────────────────────────────────┐
│                  Ethereum Network                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ UniswapV2    │  │ UniswapV3    │  │ Other DEXs   │  │
│  │ Contracts    │  │ Contracts    │  │              │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         │ Events          │ Events           │ Events   │
│         └────────┬────────┴──────────┬───────┘          │
│                  │                   │                  │
└──────────────────┼───────────────────┼──────────────────┘
                   │                   │ ethers-rs library
                   ▼                   ▼
           ┌───────────────┐
           │   amm-eth     │
           │   Service     │
           │               │
           │ ┌───────────┐ │
           │ │ Listener  │ │ Listens to swap events
           │ │ Contract  │ │ Calculates prices
           │ └───────────┘ │
           │               │
           │ ┌───────────┐ │
           │ │   Price   │ │ Stores current prices
           │ │   Store   │ │ per pool
           │ └───────────┘ │
           │               │
           │ ┌───────────┐ │
           │ │WebSocket  │ │ Broadcasts via WS
           │ │ Server    │ │
           │ └─────┬─────┘ │
           └───────┼───────┘
                   │ ws://localhost:8080
                   │ {"type": "price", "data": {...}}
                   ▼
           ┌──────────────────────────┐
           │  arbitrade-dex-eth       │
           │  Arbitrage Service       │
           ├──────────────────────────┤
           │ ┌────────────────────┐   │
           │ │ DexWsClient        │   │ Receives prices
           │ │ WebSocket handler  │   │
           │ └─────────┬──────────┘   │
           │           │              │
           │ ┌─────────▼──────────┐   │
           │ │ PriceCache         │   │ Stores all prices
           │ │ HashMap by token   │   │ indexed by address
           │ │ token -> []prices  │   │
           │ └────────┬───────────┘   │
           │          │               │
           │ ┌────────▼────────────┐  │
           │ │ArbitrageDetector    │  │ Compares pools
           │ │Compare all pairs    │  │ calculates profit
           │ │Find opportunities   │  │
           │ └─────────┬──────────┘   │
           │           │              │
           │ ┌─────────▼──────────┐   │
           │ │ArbitrageExecutor   │   │ Simulates/executes
           │ │Trades (dry-run)    │   │ on-chain swaps
           │ └────────────────────┘   │
           └──────────────────────────┘
                   │
                   │ Results logged
                   │ Opportunities stored
                   ▼
           Profitability tracked
```

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│ amm-eth WebSocket Broadcast                              │
│ {"type": "price",                                       │
│  "data": {                                              │
│    "token_address": "0x6B17...",                       │
│    "price_in_eth": 0.0005,                             │
│    "price_in_usd": 1000.0,                             │
│    "pool_address": "0x1234...",                        │
│    "dex_version": "UniswapV2",                         │
│    "decimals": 18,                                     │
│    "last_updated": 1700000000                          │
│  }                                                      │
│ }                                                       │
└────────────────┬────────────────────────────────────────┘
                 │
                 ▼
         ┌────────────────────┐
         │ DexWsClient        │ Parses message
         │ (dex_ws_client.rs) │
         └────────┬───────────┘
                  │
                  ▼
         ┌────────────────────┐
         │ PriceCache         │ Stores:
         │ (price_cache.rs)   │ token → [price1, price2, ...]
         │                    │
         │ token_address:     │ Each price tagged with:
         │  0x6B17... → {     │  - pool_address
         │    price1:         │  - dex_version (V2, V3, etc.)
         │      pool: 0x1234..│  - price_in_eth
         │      dex: V2       │  - liquidity_eth
         │      price: 0.0005 │
         │    price2:         │ Multiple pools for same token
         │      pool: 0x5678..│
         │      dex: V3       │
         │      price: 0.000502│
         │  }                 │
         └────────┬───────────┘
                  │
                  ▼
         ┌──────────────────────┐
         │ArbitrageDetector     │ For each token:
         │(arbitrage_detector)  │ 1. Get all prices
         │                      │ 2. Compare pools
         │ find_opportunities() │ 3. Calculate profit %
         │                      │ 4. Filter by threshold
         └────────┬─────────────┘
                  │
                  ▼
         ┌──────────────────────────┐
         │DexArbitrageOpportunity   │ Result:
         │{                         │ - token_address
         │  token: 0x6B17...,      │ - buy_pool (V2 @ 0.0005)
         │  buy_pool: {            │ - sell_pool (V3 @ 0.000502)
         │    dex: V2,             │ - price_diff: 0.000002
         │    price: 0.0005        │ - profit_percent: 0.4%
         │  },                     │ - potential_profit_eth
         │  sell_pool: {           │ - potential_profit_usd
         │    dex: V3,             │
         │    price: 0.000502      │
         │  },                     │
         │  profit_percent: 0.4%   │
         │}                        │
         └──────────┬──────────────┘
                    │
                    ▼
           ┌─────────────────────────┐
           │ ArbitrageExecutor       │ Execute trade:
           │ (executor.rs)           │ 1. Buy on V2
           │                         │ 2. Sell on V3
           │ execute()               │ 3. Calculate profit
           │                         │ 4. Account for gas
           │ [DRY RUN / REAL]        │ 5. Return result
           └────────┬────────────────┘
                    │
                    ▼
           ┌─────────────────────────┐
           │ ExecutionResult         │
           │ {                       │
           │   tx_hash: "0x...",     │
           │   actual_profit_eth,    │
           │   status: "pending"     │
           │ }                       │
           └─────────────────────────┘
```

## Price Cache Structure

```
┌───────────────────────────────────────────────────┐
│  PriceCache (DashMap)                             │
├───────────────────────────────────────────────────┤
│                                                   │
│ Key: "0x6b175474e89094c44da98b954eedeac495271d0f" │
│ Val: [PoolPrice, PoolPrice, PoolPrice]           │
│      │            │            │                  │
│      ▼            ▼            ▼                  │
│   Pool 1:     Pool 2:     Pool 3:                │
│   UniV2       UniV3       SushiSwap              │
│   0.0005ETH   0.000502    0.000498               │
│   Liq:1000    Liq:500     Liq:300                │
│                                                   │
│ Key: "0xdac17f958d2ee523a2206206994597c13d831ec7" │
│ Val: [PoolPrice, PoolPrice]                      │
│      │            │                               │
│      ▼            ▼                               │
│   Pool 1:     Pool 2:                            │
│   UniV2       UniV3                              │
│   1.0 ETH     1.003 ETH                          │
│   Liq:2000    Liq:1500                           │
│                                                   │
│ ... more tokens ...                              │
│                                                   │
│ Total: 150 unique tokens                         │
│        420 pool prices across all tokens         │
│        ~47 tokens have 2+ pools                  │
└───────────────────────────────────────────────────┘
```

## Opportunity Detection Logic

```
┌─────────────────────────────────────┐
│ For each token in cache:            │
│  prices = cache.get_all_prices()    │
│  if prices.len() < 2: continue      │
└──────────────┬──────────────────────┘
               │
               ▼
    ┌──────────────────────────┐
    │ Compare all pool pairs:  │
    │ (i=0, j=1), (i=0, j=2)  │
    │ (i=1, j=2), etc.        │
    └──────────┬───────────────┘
               │
               ▼
    ┌──────────────────────────────┐
    │ For pair (buy, sell):        │
    │                              │
    │ if sell.price <= buy.price:  │
    │   skip (no profit)           │
    │                              │
    │ diff = sell.price - buy.price│
    │ diff% = (diff/buy)*100       │
    │                              │
    │ if diff% < MIN_PROFIT%:      │
    │   skip                       │
    │                              │
    │ if diff < MIN_DIFF_ETH:      │
    │   skip                       │
    └──────────┬───────────────────┘
               │
               ▼
    ┌──────────────────────────────┐
    │ ✅ OPPORTUNITY FOUND!       │
    │                              │
    │ token: 0x6B17...            │
    │ buy_pool: price=0.0005       │
    │ sell_pool: price=0.000502    │
    │ diff%: 0.4%                  │
    │ profit_eth: 0.000002 per ETH │
    └──────────┬───────────────────┘
               │
               ▼
    ┌──────────────────────────────┐
    │ Sort opportunities by        │
    │ profit percentage (desc)     │
    │                              │
    │ Return top 10                │
    └──────────────────────────────┘
```

## State Transitions

```
START
  │
  ├─► Connect to amm-eth WebSocket
  │      │
  │      ├─► Success: go to LISTENING
  │      └─► Failed: Retry with backoff
  │
  ▼
LISTENING
  │
  ├─► Receive price updates
  │      └─► Update PriceCache
  │
  ├─► [Every CHECK_INTERVAL_SECS]
  │      └─► Scan for opportunities
  │             ├─► Found: Log + Store
  │             └─► Not found: Continue listening
  │
  ├─► WebSocket error/close
  │      └─► Go to RECONNECTING
  │
  ▼
RECONNECTING
  │
  ├─► Wait (exponential backoff)
  │      └─► Try to reconnect
  │             ├─► Success: go to LISTENING
  │             └─► Failed: increase backoff, retry
  │
  └─► Max retries reached
         └─► Exit or fallback

EXECUTING (Future)
  │
  ├─► Opportunity detected
  │      └─► Send transaction
  │             ├─► Pending: Monitor
  │             ├─► Confirmed: Calculate profit
  │             └─► Failed: Log error
  │
  └─► Resume LISTENING
```

## Component Interaction

```
DexWsClient
  │
  ├─► Calls: callback(PoolPrice)
  │          │
  │          └──► PriceCache::update_price()
  │                 │
  │                 └──► DashMap insert/update
  │
  └─► Handles: WebSocket events
                ├─► Text message: parse JSON
                ├─► Ping/Pong: respond
                ├─► Close: graceful shutdown
                └─► Error: reconnect

main.rs (every CHECK_INTERVAL)
  │
  └─► ArbitrageDetector::find_all_opportunities()
         │
         ├─► For each token:
         │    └─► PriceCache::get_all_prices(token)
         │         │
         │         └─► Compare all pairs
         │              │
         │              └─► Calculate profit
         │                   │
         │                   └─► Filter by threshold
         │
         └─► Sort and return opportunities
              │
              └─► Log top 5
                  │
                  └─► Store in DashMap
                      │
                      └─► [Future] ArbitrageExecutor::execute()
```

## Performance Profile

```
┌──────────────────────────────────────┐
│ Price Update (per message)           │
├──────────────────────────────────────┤
│ WebSocket receive:      < 1ms        │
│ JSON parse:             < 0.1ms      │
│ DashMap insert:         < 0.1ms      │
│ Total:                  ~< 1ms ✅     │
└──────────────────────────────────────┘

┌──────────────────────────────────────┐
│ Opportunity Detection (full scan)    │
├──────────────────────────────────────┤
│ For 150 tokens:                      │
│   Get all prices:       5ms          │
│   Compare pairs:        45ms         │
│   Calculate profit:     50ms         │
│   Filter/sort:          10ms         │
│ Total:                  ~110ms ⚡    │
└──────────────────────────────────────┘

┌──────────────────────────────────────┐
│ Memory Usage (idle)                  │
├──────────────────────────────────────┤
│ Base application:       5MB          │
│ Price cache (150 tok):  35MB         │
│ Dashmap overhead:       5MB          │
│ Tokio runtime:          5MB          │
│ Total:                  ~50MB ✨      │
└──────────────────────────────────────┘
```

---

See README.md for detailed documentation.
