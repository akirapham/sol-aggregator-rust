# Solana DEX Arbitrage Dashboard

## Overview

A real-time web dashboard for monitoring and managing Solana DEX arbitrage opportunities. Built with a Solana-specific focus on detecting price discrepancies between DEX pools.

## Architecture

### Dashboard Features

The dashboard is specifically designed for DEX arbitrage on Solana (not CEX like arbitrade-eth):

```
┌─────────────────────────────────────────────────────┐
│         SOLANA DEX ARBITRAGE DASHBOARD              │
├─────────────────────────────────────────────────────┤
│                                                      │
│  📊 Key Metrics (Real-time)                         │
│  ├─ Active Monitors: 25 tokens                      │
│  ├─ DEXes: 4 (Orca, Raydium, Phoenix, Marinade)   │
│  ├─ Opportunities Detected                         │
│  ├─ Estimated Profit                               │
│  ├─ Best Opportunity Margin                        │
│  └─ Monitor Latency                                │
│                                                      │
│  DEX Pool Status                                    │
│  ├─ Orca Whirlpool (2.3k pools, $15.2M liquidity) │
│  ├─ Raydium CLMM (1.8k pools, $12.8M liquidity)   │
│  ├─ Phoenix (1.2k pools, $8.5M liquidity)         │
│  └─ Marinade (856 pools, $6.2M liquidity)         │
│                                                      │
│  🎯 Monitored Tokens (25 active)                   │
│  ├─ Blue Chip: SOL, JUP, ETH, WBTC                │
│  ├─ LST: mSOL, stSOL, jitoSOL, bSOL               │
│  ├─ Meme: WIF, POPCAT, PONKE                      │
│  └─ Stablecoins: USDT, DAI, PYUSD                 │
│                                                      │
│  💰 Arbitrage Opportunities (Real-time)            │
│  ├─ Forward Route: Pool A → Pool B                │
│  ├─ Reverse Route: Pool B → Pool A                │
│  ├─ Price Comparison & Profit Calculation         │
│  ├─ Execution Status                              │
│  └─ Last 20 Detected Opportunities                │
│                                                      │
└─────────────────────────────────────────────────────┘
```

## Key Differences from arbitrade-eth

### arbitrade-eth (CEX Arbitrage)
- Compares DEX price vs CEX price (Binance, KuCoin, etc.)
- Strategy: Buy cheap on DEX → Sell expensive on CEX
- Single-direction arbitrage
- Requires CEX API integration

### arbitrage-sol (DEX Arbitrage)
- Compares prices between different DEX pools
- Strategy: Pool A (cheap) → Pool B (expensive) → back to USDC
- Round-trip arbitrage (atomic within one transaction)
- All on-chain, no CEX needed
- Event-driven detection based on pool updates

## Dashboard Sections

### 1. Key Metrics (Top Stats)

**Real-time indicators:**
- **Active Monitors**: Number of tokens currently being tracked
- **DEXes Supported**: Count of supported DEX protocols
- **Opportunities Detected**: Total found in current session
- **Estimated Profit**: Unrealized profit from detected opportunities
- **Best Opportunity**: Highest profit margin detected
- **Monitor Latency**: Average detection time from pool update to detection

### 2. DEX Pool Status

Shows statistics for each supported DEX:

```
🐋 Orca Whirlpool
├─ Total Liquidity: $15.2M
├─ 24h Volume: $287M
├─ Mispriced Pools: 3 (pools with price discrepancies)
└─ Average Spread: 0.12%

⚡ Raydium CLMM
├─ Total Liquidity: $12.8M
├─ 24h Volume: $456M
├─ Mispriced Pools: 5
└─ Average Spread: 0.08%

🔥 Phoenix
├─ Total Liquidity: $8.5M
├─ 24h Volume: $125M
├─ Mispriced Pools: 2
└─ Average Spread: 0.15%

🏦 Marinade
├─ Total Liquidity: $6.2M
├─ 24h Volume: $89M
├─ Mispriced Pools: 1
└─ Average Spread: 0.22%
```

**What "Mispriced Pools" means:**
- A pool where the price differs significantly from other DEXes
- Example: Orca has SOL at 100 USDC, Raydium has it at 101 USDC
- This creates an arbitrage opportunity

### 3. Monitored Tokens Table

Lists all 25 tokens being monitored:

| # | Token | Symbol | Address | Status | Category | Opportunities | Best Spread | Actions |
|---|-------|--------|---------|--------|----------|----------------|-------------|---------|
| 1 | Solana | SOL | `So11...112` | ✅ Enabled | Blue Chip | 12 | 0.18% | Remove |
| 2 | Jupiter | JUP | `JUPy...vCN` | ✅ Enabled | Blue Chip | 8 | 0.22% | Remove |
| 3 | Marinade SOL | mSOL | `mSoL...So` | ✅ Enabled | LST | 5 | 0.05% | Remove |

**Features:**
- Enable/disable tokens at runtime (API-backed)
- Filter by category (Blue Chip, LST, Meme, Stablecoin)
- View opportunities detected per token
- See best price discrepancy ("spread") for each token

### 4. Recent Arbitrage Opportunities

Real-time table of detected arbitrage opportunities:

**Example:**
```
Time: 2025-10-25 14:32:45
Token: SOL

Forward Route:  Orca (100 USDC) → Raydium (101 USDC)
Reverse Route:  Raydium (101.5 USDC) → Orca (102 USDC)

Forward Output: 9,900 SOL
Reverse Output: 9,803 SOL

Profit: ~$200
Margin: 0.18%

Status: ⏳ Pending
```

**What this shows:**
1. **Forward Route**: Buy cheap (Orca) → Sell at better price (Raydium)
2. **Reverse Route**: Take that output and swap back
3. **Net Profit**: Difference after all swaps and fees
4. **Margin**: Profit as percentage of input

## Technical Implementation

### Routes

```
GET  /                      → Dashboard page (HTML)
GET  /dashboard             → Dashboard page (HTML)

GET  /health                → Health check
POST /quote                 → Get swap quote
POST /arbitrage             → Check arbitrage opportunity

GET  /arbitrage/tokens      → List monitored tokens
POST /arbitrage/tokens      → Add token to monitoring
DELETE /arbitrage/tokens    → Remove token from monitoring

GET  /pools/:token0/:token1 → Get pools for pair
GET  /stats                 → Get pool statistics
```

### Data Updates

**Auto-refresh: Every 5 seconds**
- Fetches live opportunity data
- Updates statistics
- Refreshes last update timestamp

**Manual refresh buttons:**
- Refresh Tokens
- Refresh Opportunities
- Filter by category, profitability, margin

## Real-time Data Flow

```
Pool Updates (Geyser)
    ↓
Pool Manager (applies updates)
    ↓
Calculate prices (forward & reverse)
    ↓
Broadcast pool update (with prices)
    ↓
Arbitrage Monitor (subscribes to broadcast)
    ↓
Quick price check (forward * reverse > 1.0)
    ↓
Full routing (if promising)
    ↓
Detect Opportunity ✅
    ↓
Dashboard (WebSocket or polling)
    ↓
Display in Opportunities Table
```

## API Integration Points

### 1. Dashboard Page
```rust
GET /dashboard
Response: HTML page with embedded data
```

### 2. Token Management (API endpoints)
```rust
GET /arbitrage/tokens
{
  "base_token": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  "monitored_tokens": [
    {
      "address": "So11111111111111111111111111111111111111112",
      "symbol": "SOL",
      "enabled": true
    },
    ...
  ]
}

POST /arbitrage/tokens
{
  "address": "TOKEN_ADDRESS",
  "symbol": "TOKEN_SYMBOL"
}

DELETE /arbitrage/tokens
{
  "address": "TOKEN_ADDRESS"
}
```

### 3. Arbitrage Opportunities (TODO)
```rust
GET /arbitrage/opportunities?limit=20&sort=profit
{
  "opportunities": [
    {
      "timestamp": 1698243165,
      "token_a": "So11111111111111111111111111111111111111112",
      "token_b": "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
      "forward_pool": "orca_pool_address",
      "reverse_pool": "raydium_pool_address",
      "forward_price": 100.0,
      "reverse_price": 101.0,
      "profit_amount": 250000,
      "profit_percent": 0.25,
      "status": "pending"
    },
    ...
  ]
}
```

### 4. Pool Statistics (TODO)
```rust
GET /stats
{
  "total_pools": 5856,
  "monitored_tokens": 25,
  "active_opportunities": 28,
  "opportunities_today": 412,
  "best_opportunity_percent": 0.35,
  "average_spread_bps": 12,
  "dex_stats": {
    "orca": { "pools": 2300, "liquidity": 15200000, "volume_24h": 287000000 },
    "raydium_clmm": { "pools": 1800, "liquidity": 12800000, "volume_24h": 456000000 },
    ...
  }
}
```

## Usage Guide

### Accessing the Dashboard
```bash
http://localhost:3000/dashboard
```

### Monitoring Tokens
1. **View**: Table shows all 25 monitored tokens with stats
2. **Add**: Click "Add Token" (via API endpoint)
3. **Remove**: Click "Remove" to stop monitoring
4. **Enable/Disable**: Toggle monitoring state without removing

### Finding Opportunities
1. **Recent**: View last 20 detected opportunities
2. **Filter**: By profitability (all, positive only, >50bps, >100bps)
3. **Sort**: By profit amount or time
4. **Details**: See price comparison between pools

### Metrics to Watch
- **Monitor Latency**: Should be <50ms for fast detection
- **Mispriced Pools**: Higher = more opportunities
- **Average Spread**: Target opportunities with spread > 50bps (0.5%)
- **DEX Volume**: Higher volume = better execution confidence

## Implementation TODO

### Phase 1: Dashboard Display (Current)
- ✅ Create dashboard HTML/CSS
- ✅ Add static example data
- ⏳ Connect to arbitrage monitor for live data
- ⏳ WebSocket for real-time updates

### Phase 2: Data Backend
- ⏳ Create opportunity tracking struct
- ⏳ Store recent opportunities (in-memory or RocksDB)
- ⏳ Implement /arbitrage/opportunities API
- ⏳ Implement /stats API with live pool data

### Phase 3: Advanced Features
- ⏳ WebSocket for real-time updates (no polling)
- ⏳ Historical charts (profit over time)
- ⏳ DEX performance comparison
- ⏳ Token performance metrics
- ⏳ Alerts for high-profit opportunities

### Phase 4: Execution UI
- ⏳ "Execute" button for opportunities (when execution ready)
- ⏳ Execution confirmation modal
- ⏳ Transaction tracking
- ⏳ Profitability analytics

## Design Highlights

### Solana-Specific Design
- 🟣 Solana brand colors (purple/green gradient)
- 🚀 DEX-focused terminology (pools, liquidity, spreads)
- ⚡ Fast/real-time emphasis (latency metrics)
- 🔄 Round-trip arbitrage visualization

### Key Metrics Displayed
- **Pool Status**: What's happening on each DEX
- **Token Health**: Opportunities per token
- **Arbitrage Details**: Route visualization with prices
- **Performance**: Latency and throughput metrics

### User Actions
- Add/remove tokens from monitoring
- Filter opportunities by profitability
- Refresh data on demand
- Auto-update every 5 seconds

## Example Data

### Live Opportunity Example:
```
Time: 14:32:45 UTC
Token: SOL (So11111111111111111111111111111111111111112)

Detect Arbitrage:
- Orca has SOL cheaper: 100 USDC per SOL
- Raydium has SOL expensive: 101 USDC per SOL

Trade:
1. Swap 10,000 USDC → SOL on Orca (get 100 SOL for 100 USDC each)
2. Swap 100 SOL → USDC on Raydium (get 10,100 USDC at 101 USDC each)

Result:
- Input: 10,000 USDC
- Output: 10,100 USDC
- Profit: 100 USDC
- Margin: 1.0%

Notes: After fees (~$50), net profit ≈ $50 (0.5%)
```

### Token Categories:

**Blue Chip** (8 tokens):
- SOL, JUP, ETH, WBTC, USDT, USDC, DAI, PYUSD

**LST - Liquid Staking** (4 tokens):
- mSOL (Marinade), stSOL (Lido), jitoSOL (Jito), bSOL (BlazeStake)
- Often trade at different rates to underlying SOL

**DEX Governance** (3 tokens):
- RAY (Raydium), ORCA (Orca), JTO (Jito)

**Meme Coins** (5 tokens):
- WIF, POPCAT, PONKE, MOTHER, MEW
- High volatility = frequent price discrepancies

**Others** (5 tokens):
- RENDER, W, PYTH, HNT, BONK

## Next Steps

1. **Connect to Live Data**: Feed arbitrage monitor data into dashboard
2. **Add WebSocket**: Real-time updates without polling
3. **Implement Execution**: Add "Execute" button for opportunities
4. **Track Results**: Store executed trades and profitability
5. **Historical Analytics**: Charts and trends over time
