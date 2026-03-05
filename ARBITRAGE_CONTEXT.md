# Ethereum Arbitrage Project Context

## Project Overview

This is an Ethereum/Arbitrum arbitrage detection and execution system. It monitors DEX pools for price inefficiencies and executes flashloan-based arbitrage trades.

## Architecture

### Services (Docker Compose)

- **amm-eth**: Price feed service
  - Listens to Uniswap V2/V3/V4 swap events on Ethereum/Arbitrum
  - Provides prices via WebSocket (port 8080) and REST API (port 2222)
  - Uses RocksDB for persistence

- **arbitrade-dex-eth**: Arbitrage detection and execution
  - Connects to amm-eth via WebSocket for real-time price updates
  - Detects arbitrage opportunities between pools
  - Computes on-chain quotes via QuoteRouter
  - Executes flashloan trades (when profitable)

### Key Components

- **crates/eth-dex-quote**: Core quote library with DEX configs
- **bins/amm-eth**: Binary for price feed
- **bins/arbitrade-dex-eth**: Binary for arbitrage engine

### Arbitrage Logic

1. **Detection**: Find price differences between pools for the same token
2. **Path Types**:
   - **2-hop**: X → Token → X (flashloan token same as return token)
   - **3-hop**: X → Token A → Token B → X (when sell pool goes to different token)
3. **Multicall**: Uses single RPC calls to batch quote multiple paths
   - 2-hop: `quote_arbitrage_2_hop`
   - 3-hop: `quote_multi_paths` (all B→X paths in one call)

## Recent Changes

### Bug Fixes

1. **Profit calculation bug** (FIXED): When swap output was 0, profit showed as -1000 USD (incorrect). Added zero-output check to treat as error instead.

2. **Code refactoring**: Removed the experimental "check all paths" approach that was building too many invalid paths (60k+ paths) causing RPC reverts. Reverted to original opportunity-based checking that works correctly.

### Current Behavior

- On price update, detects all pools with price differences > 0.1%
- For each opportunity, computes exact on-chain profit via QuoteRouter
- Shows realistic profit/loss (e.g., -$30 to -$800 losses typical)
- All opportunities currently unprofitable after gas + slippage

## Configuration

Key env vars in `.env.arbitrage`:
- `ARB_RPC_URL`: Ethereum RPC (default: arb1.arbitrum.io/rpc)
- `ARB_WEBSOCKET_URL`: WebSocket for events
- `QUOTE_ROUTER_ADDRESS`: 0x09e2f790bD344cF842E1c5D37BAffB39f5c09985
- `MIN_PRICE_DIFF_TRIGGER`: 0.1% (minimum price diff to check)
- `ETH_ARBITRADE_KEY`: Private key for execution

## Running

```bash
# Start services
docker-compose -f docker-compose.arb.yml up -d

# View logs
docker-compose -f docker-compose.arb.yml logs -f arbitrade-dex-eth

# Stop
docker-compose -f docker-compose.arb.yml down
```

## Notes for Server Deployment

- RPC calls from the server will be much faster than from Docker cloud
- May want to adjust `MIN_PRICE_DIFF_TRIGGER` to filter smaller opportunities
- Flashloan amount currently 1000 USD (can be increased for larger profits)
- The system is ready to execute when profitable opportunities are found
