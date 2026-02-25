# Callback-Chained Cross-DEX Arbitrage — Implementation Guide

> **Purpose**: This document provides everything needed to implement the cross-DEX
> arbitrage strategy observed on Arbitrum (deployer `0x33eabd63`).
> All data is sourced from on-chain trace analysis of live, profitable bot contracts.
>
> **CORRECTION (Feb 2026)**: Deep trace analysis revealed the bots do NOT use Balancer
> flash loans. They use **swap callback chaining** — executing the second swap inside
> the callback of the first, achieving atomic execution without any flash loan overhead.
> No special priority mechanism (Timeboost/bribes) is used — just Legacy type 0 txs
> at base fee. Profit is split via `WETH.withdrawTo()` between two operator-owned
> wallets (deployer `0x33eabd63...` and secondary `0x743be0db...`) with variable ratios.

---

## Table of Contents

1. [Strategy Overview](#1-strategy-overview)
2. [On-Chain Infrastructure](#2-on-chain-infrastructure)
3. [Price Discovery System](#3-price-discovery-system)
4. [Execution Flow](#4-execution-flow)
5. [Observed Trade Routes](#5-observed-trade-routes)
6. [Profitability Data](#6-profitability-data)
7. [Function Selectors & ABIs](#7-function-selectors--abis)
8. [Contract Architecture](#8-contract-architecture)
9. [Gas & Economics](#9-gas--economics)
10. [Risk Management](#10-risk-management)

---

## 1. Strategy Overview

### What the bot does

The bot reads atomic spot prices from Camelot (Algebra v1.9) pools via `globalState()`,
compares them against Uniswap V3, V2, PancakeSwap V3, and other Algebra pools, and
executes cross-DEX arbitrage when a price discrepancy is found.

### Capital mechanism: Callback Chaining (NOT flash loans)

The bot does **not** use Balancer flash loans. Zero `FlashLoan` events were found
for either bot contract across all historical transactions. Instead, it uses **swap
callback chaining**:

1. Initiate a swap on Pool A (e.g., Algebra `swap()`)
2. Pool A calls `algebraSwapCallback()` on the bot — requesting tokens
3. Inside that callback, the bot initiates a swap on Pool B, which provides the tokens
4. Pool B calls its callback (e.g., `uniswapV3SwapCallback()`) — the bot sends tokens from Pool A's output
5. Both swaps settle atomically

This is more gas-efficient than flash loans (~30k gas saved) and requires no
pre-existing capital — the swap callbacks implicitly provide "just-in-time" liquidity.

### Core loop (pseudocode)

```
every_block:
    camelot_price = camelot_pool.globalState().sqrtPriceX96
    uni_v3_price  = uni_v3_pool.slot0().sqrtPriceX96
    other_prices  = read_other_pools()

    for route in candidate_routes:
        expected_output = simulate_route(route, prices)
        if expected_output - expected_input > gas_cost:
            submit_tx(route)
            break
    else:
        revert()  // no opportunity — tx fails, costs ~35-78k gas ($0.001-0.005)
```

### Key design decisions observed

| Decision | Choice | Why |
|----------|--------|-----|
| Price source | Camelot `globalState()` (STATICCALL) | Free (~2,600 gas), atomic, no oracle delay |
| Capital source | **Swap callback chaining** | Zero fee, zero overhead, no external dependency |
| Failure mode | Revert on no opportunity | Simple; ~35-78k gas wasted ($0.001-0.005 on Arbitrum) |
| Execution | Nested callbacks across pool types | Direct pool access, no router overhead |
| Profit extraction | WETH.withdrawTo() → 2 operator wallets | 50/50 split (typical), variable on larger trades |
| Priority mechanism | None (Legacy type 0 tx, base fee only) | No Timeboost or bribes — relies on speed |
| Architecture | Multi-contract (reader + executor + router) | Separation of concerns, upgradability |
| Calldata | Packed custom selectors (`0x00000020`, `0x00000021`) | Minimal calldata = lower L1 data fee |

---

## 2. On-Chain Infrastructure

### Network

| Property | Value |
|----------|-------|
| Chain | Arbitrum One (chain ID: 42161) |
| Block time | ~0.25 seconds |
| Gas price | ~0.01-0.1 gwei (extremely cheap) |
| Gas per failed tx | ~35,000-78,000 (~$0.001-0.005) |
| Gas per successful arb | ~260,000-584,000 (~$0.01-0.10) |

### Token Addresses (Arbitrum)

| Token | Address | Decimals |
|-------|---------|----------|
| WETH | `0x82aF49447D8a07e3bd95BD0d56f35241523fBab1` | 18 |
| USDC (native) | `0xaf88d065e77c8cC2239327C5EDb3A432268e5831` | 6 |
| USDC.e (bridged) | `0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8` | 6 |
| USDT | `0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9` | 6 |
| ARB | `0x912CE59144191C1204E64559FE8253a0e49E6548` | 18 |
| WBTC | `0x2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f` | 8 |
| DAI | `0xDA10009cBd5D07dd0CeCc66161FC93D7c9000da1` | 18 |
| GMX | `0xfc5A1A6EB076a2C7aD06eD22C90d7E710E35ad0a` | 18 |
| MAGIC | `0x539bdE0d7Dbd336b79148AA742883198BBF60342` | 18 |
| PENDLE | `0x0c880f6761F1af8d9Aa9C466984b80DAb9a8c9e8` | 18 |
| GRAIL | `0x3d9907F9a368ad0a51Be60f7Da3b97cf940982D8` | 18 |

### Key Protocol Contracts

| Contract | Address | Notes |
|----------|---------|-------|
| Camelot AlgebraFactory | `0x1a3c9B1d2F0529D97f2afC5136Cc23e58f1FD35B` | Pool registry |
| Uniswap V3 Factory | `0x1F98431c8aD98523631AE4a59f267346ea31F984` | V3 pool registry |
| PancakeSwap V3 Factory | `0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865` | PCS V3 pool registry |
| Uniswap V4 PoolManager | `0x360E68faCcca8cA495c1B759Fd9EEe466db9FB32` | V4 swap execution (verified) |
| Balancer V2 Vault | `0xBA12222222228d8Ba445958a75a0704d566BF2C8` | Available but NOT used by this bot |

### Bot Infrastructure Contracts (Observed from traces)

| Contract | Address | Role | Code Size |
|----------|---------|------|-----------|
| Bot 1 (reader) | `0x4ad74bc56f70cae4ae1308f62a18d15a1a556aaf` | Price reading + state check | Unverified |
| Bot 2 (reader) | `0xea4c5299b308fa6a220a1184e94c36f60efd397d` | Price reading + state check | 22,997 bytes |
| Bot 1 Executor | `0x1b61a41fcd...` | Orchestrates read → execute flow | Unverified |
| Bot 1 Router | `0xa1ff0ea658...` | Executes actual swap legs | Unverified |
| Bot 2 Executor | `0xfc7dc4f6...` | Delegates swap execution (sel: `0x92ead328`) | Unverified |
| Shared component | `0x8b194bea...` | Used by both bots (pool or utility) | 8,588 bytes |
| Proxy | `0x3b4557fe...` | Small proxy in Bot 1 path | 387 bytes |

### Camelot (Algebra v1.9) Pools — Price Sources

These are the pools the bot reads `globalState()` from for price discovery:

| Pool | Pair | Address | Liquidity |
|------|------|---------|-----------|
| Primary | WETH/USDC.e | `0xb1026b8e7276e7ac75410f1fcbbe21796e8f7526` | 21.8T |
| Primary | WETH/ARB | `0xe51635ae8136abac44906a8f230c2d235e9c195f` | 11.2T |
| Secondary | USDC/USDC.e | `0xc86eb7b85807020b4548ee05b54bfc956eebbfcd` | 11.7T |
| Secondary | ARB/USDC.e | `0x45fae8d0d2ace73544baab452f9020925afccc75` | 431B |
| Secondary | WETH/USDT | `0xfae2ae0a9f87fd35b5b0e24b47bac796a7eefea1` | 49.4T |
| Secondary | MAGIC/WETH | `0x1106db7165a8d4a8559b441ecdee14a5d5070dbc` | 6.7T |
| Secondary | WETH/GMX | `0xc99be44383bc8d82357f5a1d9ae9976ee9d75bee` | 542B |
| Secondary | GRAIL/USDC | `0x8cc8093218bcac8b1896a1eed4d925f6f6ab289f` | 5.2T |
| Secondary | PENDLE/WETH | `0xe461f84c3fe6bcdd1162eb0ef4284f3bb6e4cad3` | 7.0T |

### Pool Activity Snapshot

Based on a ~20 minute window (5,000 blocks) of the WETH/USDC.e pool:

| Metric | Value |
|--------|-------|
| Swap events | 43 |
| Total WETH volume | 1.43 WETH |
| Total USDC.e volume | $2,787 |
| Average swap size | 0.033 WETH (~$65) |
| WETH/ARB pool swaps (same window) | 9 |

### Live Price Snapshot (Feb 2026)

| Pool | Price |
|------|-------|
| WETH/USDC.e | 1 WETH = ~$1,942 USDC.e |
| WETH/ARB | 1 WETH = ~20,649 ARB |
| USDC/USDC.e | 1:1 (peg) |
| ARB implied | ~$0.094 |

---

## 3. Price Discovery System

### How `globalState()` works

Algebra v1.9 pools expose `globalState()` as a view function returning the pool's
current state in a single STATICCALL:

```solidity
function globalState() external view returns (
    uint160 sqrtPriceX96,   // Current sqrt(price) * 2^96
    int24   tick,            // Current tick
    uint16  fee,             // Current fee in hundredths of a bip
    uint16  timepointIndex,  // Index of last written timepoint
    uint16  communityFeeToken0,
    uint16  communityFeeToken1,
    bool    unlocked         // Reentrancy lock status
);
```

### sqrtPriceX96 → Human Price Conversion

```python
def sqrt_price_to_price(sqrtPriceX96: int, decimals0: int, decimals1: int) -> float:
    """Convert sqrtPriceX96 to price of token0 in terms of token1."""
    price = (sqrtPriceX96 / (2**96))**2 * (10**(decimals0 - decimals1))
    return price

# Example: WETH/USDC.e pool
# sqrtPriceX96 = 3495163748697822588028897
# price = (3495163748697822588028897 / 2^96)^2 * 10^(18-6)
# price ≈ 0.000515  (WETH per USDC.e)
# Inverted: 1/0.000515 ≈ 1942 USDC.e per WETH ✓
```

### Pre-flight State Reads (Observed from traces)

Before each trade, the bot reads from pools using these selectors:

| Selector | Function | Purpose |
|----------|----------|---------|
| `0xe76c01e4` | `globalState()` | Algebra pool price + fee |
| `0x3850c7bd` | `slot0()` | Uniswap V3 pool price + fee |
| `0x0dfe1681` | `token0()` | Verify pool token ordering |
| `0xd21220a7` | `token1()` | Verify pool token ordering |
| `0x1a686502` | Unknown (state read) | Additional state on Algebra pools |
| `0xd0c93a7c` | Unknown (state read) | Additional pool data |

### Price Comparison Logic

```python
def check_opportunity(camelot_price, other_prices):
    """
    Check if any cross-venue route is profitable.

    Example: WETH on Algebra vs WETH on UniV3

    1. Read Algebra WETH/USDC.e price via globalState()
    2. Read UniV3 WETH/USDC price via slot0()
    3. If buying WETH on venue A and selling on venue B yields profit
       after gas + bribe, execute.
    """
    for route in generate_routes(camelot_price, other_prices):
        input_amount = route.start_amount
        output_amount = simulate_swaps(route)
        gas_cost_usd = estimate_gas(route) * eth_price

        profit = output_amount - input_amount - gas_cost_usd
        if profit > MIN_PROFIT_THRESHOLD:
            return route
    return None
```

### What prices to read

The observed bot reads these in every transaction:

1. **Always**: Camelot pool `globalState()` — primary price reference
2. **Always**: Counter-pool `slot0()` or `globalState()` — the other side of the trade
3. **Always**: `token0()` and `token1()` on pools — verify token ordering
4. **Sometimes**: Additional pool state reads for multi-hop routes

---

## 4. Execution Flow

### How Callback Chaining Works

Instead of borrowing funds via flash loan, the bot exploits the fact that AMM swap
callbacks are called **before** the pool checks that it received the correct tokens.
This creates a window where the bot can use the output of one swap to fund the input
of another:

```
Pool A calls swap → sends tokens to bot → calls algebraSwapCallback()
  └─ Inside callback: bot calls Pool B swap → receives tokens → sends them to Pool A
       └─ Pool B calls uniswapV3SwapCallback()
            └─ Bot sends Pool A's output tokens to Pool B

Both pools verify they received correct tokens. Transaction succeeds.
No flash loan needed. No capital needed.
```

### Complete Transaction Lifecycle — Bot 1

Bot 1 uses a **multi-contract architecture** with separate reader, executor, and router:

```
┌──────────────────────────────────────────────────────────────┐
│                    EOA SENDS TRANSACTION                      │
│                                                              │
│  tx.to = Executor Contract (0x1b61a41fcd...)                 │
│  tx.data = 0x67d8fe79 + packed route params                  │
│  tx.gasLimit = ~600,000                                      │
│  tx.gasPrice = ~0.03 gwei                                    │
└──────────────────────┬───────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────┐
│      PHASE 1: STATE READING (via Bot Reader contract)        │
│                                                              │
│  Executor calls Bot 1 (0x4ad74bc5) with selector 0x359ecc85 │
│    ├─ STATICCALL → Camelot pool.globalState()                │
│    ├─ STATICCALL → UniV3 pool.slot0()                        │
│    ├─ STATICCALL → pool.token0() + pool.token1()             │
│    └─ Returns: price data + opportunity assessment           │
│                                                              │
│  IF no opportunity → REVERT (cost: ~78,000 gas, ~$0.005)    │
└──────────────────────┬───────────────────────────────────────┘
                       │ opportunity found
                       ▼
┌──────────────────────────────────────────────────────────────┐
│      PHASE 2: EXECUTE via Router (callback-chained)          │
│                                                              │
│  Executor calls Router (0xa1ff0ea658...)                     │
│    │                                                         │
│    ├─ Leg 1: AlgebraPool.swap(router, zeroToOne, amount,...) │
│    │   Pool sends 0.9492 WETH to router                      │
│    │   Pool calls algebraSwapCallback on router              │
│    │     └─ Router sends 1,868.86 USDC to Algebra pool      │
│    │                                                         │
│    ├─ Leg 2: UniV3Pool.swap(router, zeroToOne, amount,...)   │
│    │   Pool sends 1,872.54 USDC to router                    │
│    │   Pool calls uniswapV3SwapCallback on router            │
│    │     └─ Router sends 0.9491 WETH to UniV3 pool          │
│    │                                                         │
│    │  Net: +3.68 USDC + 0.0001 WETH                         │
│    │                                                         │
│    └─ Remaining profit flows to executor                     │
└──────────────────────┬───────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────┐
│      PHASE 3: PROFIT EXTRACTION (2-way split)                 │
│                                                              │
│  Profit flows through WETH.withdrawTo() which:               │
│    1. Burns WETH tokens (emits Transfer to 0x0000...)        │
│    2. Sends native ETH to specified recipient                │
│                                                              │
│  EVERY successful tx has exactly 2 withdrawTo() calls:       │
│    withdrawTo(0x743be0db..., amount_A)  ← operator wallet A  │
│    withdrawTo(0x33eabd63..., amount_B)  ← deployer wallet    │
│                                                              │
│  Both wallets are owned by the same operator (0x743be0db     │
│  is active across multiple chains). This is internal profit  │
│  distribution, NOT a bribe or external fee.                  │
│                                                              │
│  Split ratios (from 15 traced txs):                          │
│    50/50  — 10 of 15 txs (most common, smaller trades)       │
│    46.5/53.5 — 3 of 15 txs (deployer gets more)             │
│    ~98/2  — 2 of 15 txs (wallet A gets nearly all)           │
│                                                              │
│  The variable split may depend on trade size or route type.  │
│                                                              │
│  Gas cost: 0.0000099 ETH (~$0.02)                            │
│  No priority fees, no bribes, no Timeboost — Legacy tx at   │
│  base fee only (maxPriorityFeePerGas = 0).                   │
└──────────────────────────────────────────────────────────────┘
```

### Complete Transaction Lifecycle — Bot 2

Bot 2 is simpler — single executor contract with direct pool interaction:

```
┌──────────────────────────────────────────────────────────────┐
│                    EOA SENDS TRANSACTION                      │
│                                                              │
│  tx.to = Bot 2 Contract (0xea4c5299...)                      │
│  tx.data = 0x00000020 + packed route params (93% of txs)     │
│         or 0x00000021 + packed route params (7% of txs)      │
│  tx.gasLimit = ~600,000                                      │
└──────────────────────┬───────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────┐
│      PHASE 1: STATE READING (inline)                         │
│                                                              │
│  STATICCALL → Camelot pool.globalState()                     │
│  STATICCALL → Other pool.slot0() or globalState()            │
│  STATICCALL → pool.token0() + pool.token1()                  │
│                                                              │
│  IF no opportunity → REVERT (cost: ~35,000 gas, ~$0.001)    │
└──────────────────────┬───────────────────────────────────────┘
                       │ opportunity found
                       ▼
┌──────────────────────────────────────────────────────────────┐
│      PHASE 2: DELEGATE to Executor                           │
│                                                              │
│  CALL → Executor (0xfc7dc4f6...) with selector 0x92ead328   │
│    │                                                         │
│    ├─ Route Type: V3-to-V2                                   │
│    │   UniV3Pool.swap() → uniswapV3SwapCallback              │
│    │     └─ Inside callback: UniV2Pool.swap(0x022c0d9f)      │
│    │                                                         │
│    ├─ Route Type: V3-to-V3                                   │
│    │   UniV3Pool.swap() → uniswapV3SwapCallback              │
│    │     └─ Inside callback: another UniV3Pool.swap()        │
│    │                                                         │
│    ├─ Route Type: Algebra-to-PancakeV3                       │
│    │   AlgebraPool.swap() → algebraSwapCallback              │
│    │     └─ Inside callback: PancakeV3Pool.swap()            │
│    │                                                         │
│    └─ Route Type: 4-pool cyclic                              │
│        Pool1.swap → callback → Pool2.swap → callback → ...  │
│                                                              │
└──────────────────────┬───────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────┐
│      PHASE 3: PROFIT EXTRACTION                              │
│                                                              │
│  TRANSFER profit → Deployer EOA (0x33eabd63...)              │
│                                                              │
│  Profit token varies by route:                               │
│    WETH, USDC, USDC.e, USDT, DAI, ARB, WBTC                │
└──────────────────────────────────────────────────────────────┘
```

### Decoded Real Transactions

#### Bot 1: Algebra ↔ UniV3 WETH/USDC Arb

**TX**: `0x4dc9980d305cb63435d75d915220ab0641c6d54690dd9e0d0a9ad21d29502aa2`

| Step | Action | Amount | Venue |
|------|--------|--------|-------|
| 1 | Read Camelot WETH/USDC.e `globalState()` | — | STATICCALL |
| 2 | Read UniV3 WETH/USDC `slot0()` | — | STATICCALL |
| 3 | Swap on Algebra pool (buy WETH) | 1,868.86 USDC → 0.9492 WETH | Camelot `0xb1026b8e...` |
| 4 | Swap on UniV3 pool (sell WETH) | 0.9491 WETH → 1,872.54 USDC | UniV3 `0xc6962004...` |
| 5 | Algebra pool fee | 0.028 USDC | Fee recipient |
| 6 | **Net profit** | **3.68 USDC + 0.0019 WETH** | |
| 7 | Convert profit to WETH | 3.68 USDC → WETH | UniV3 |
| 8 | WETH.withdrawTo() | Profit ETH → deployer wallet | `0x33eabd63...` |

Gas: 495,626 (0.0000167 ETH = $0.03). Net profit: ~$3.65.
Note: WETH Transfer→0x0000 events in logs are withdrawTo() burns, not actual burns.
The ETH flows to DEX pools (swap legs) and to the deployer (profit).

#### Bot 2: V3-to-V2 WETH Arb

**TX**: `0x2316a320...`

| Step | Action | Amount | Venue |
|------|--------|--------|-------|
| 1 | Read pool states | — | STATICCALL |
| 2 | Swap on V2 pool | WETH in/out | UniV2 `0xdb07...` (via `0x022c0d9f`) |
| 3 | Swap on V3/Algebra pool | WETH in/out | Pool `0x8f5c...` |
| 4 | **Profit** | **0.000031 WETH (~$0.06)** | → deployer |

Gas: 260,642 (0.0000391 ETH = $0.08).

#### Bot 2: V3-to-V3 WETH Arb

**TX**: `0x375f4dea...`

| Step | Action | Amount | Venue |
|------|--------|--------|-------|
| 1 | Read pool states | — | STATICCALL |
| 2 | Swap on V3/Algebra pool A | WETH in/out | Pool `0x8f5c...` |
| 3 | Swap on V3/Algebra pool B | WETH in/out | Pool `0xb7cc...` |
| 4 | **Profit** | **0.000113 WETH (~$0.22)** | → deployer |

Gas: 548,715 (0.0000823 ETH = $0.16).

#### Bot 2: 4-Pool Cyclic USDT/WBTC/WETH Arb

**TX**: `0x9c0d0cdb...`

| Step | Action | Amount | Venue |
|------|--------|--------|-------|
| 1 | Swap USDT → WBTC | via V3 pool | UniV3 |
| 2 | Swap WBTC → WETH | via V3 pool | UniV3 |
| 3 | Swap WETH → intermediate | via V3/PCS | PancakeV3 (`pancakeV3SwapCallback`) |
| 4 | Swap intermediate → USDT | via V3 pool | UniV3 |
| 5 | **Profit** | **0.000002 WBTC (~$0.19)** | → deployer |

Gas: 583,467 (0.0000159 ETH = $0.03).

#### Bot 2: ARB/USDC.e Cross-V3 Arb

**TX**: `0x8d8510d8...`

| Step | Action | Amount | Venue |
|------|--------|--------|-------|
| 1 | Swap on V3 Pool A | ARB ↔ USDC.e | `0xa832...` |
| 2 | Swap on V3 Pool B | USDC.e ↔ ARB | `0xcda5...` |
| 3 | **Profit** | **1.025 USDC.e (~$1.03)** | → deployer |

Gas: 325,684 (0.0000131 ETH = $0.03).

---

## 5. Observed Trade Routes

### Route Categories

The bots use multiple route configurations across 5 DEX protocols. These patterns
were decoded from tracing actual successful transactions:

#### Route Type A: Algebra ↔ UniV3 (Most Common for Bot 1)
**2-leg cross-venue price arbitrage**

```
    Buy WETH cheap              Sell WETH expensive
USDC ──[Camelot/Algebra]──→ WETH ──[Uniswap V3]──→ USDC
                        (or reverse direction)
```

- Callback chained: Algebra `swap()` → `algebraSwapCallback()` → UniV3 `swap()`
- **Observed profit**: $3.68 per trade (decoded example)
- **Observed amounts**: ~0.95 WETH ($1,870)
- Primary pool pair: Camelot WETH/USDC.e vs UniV3 WETH/USDC

#### Route Type B: V3-to-V2 (Bot 2 Specialty)
**Cross-AMM-type arbitrage**

```
WETH ──[Uniswap V3]──→ Token ──[Uniswap V2]──→ WETH
```

- Callback chained: V3 `swap()` → `uniswapV3SwapCallback()` → V2 `swap(0x022c0d9f)`
- Exploits V2/V3 price lag — V2 pools update slower
- **Observed profit**: 0.000031-0.000113 WETH per trade

#### Route Type C: V3-to-V3 Cross-Pool (Bot 2)
**Same-protocol, different-pool arbitrage**

```
Token ──[V3 Pool A]──→ Token ──[V3 Pool B]──→ Token
```

- Same token pair but different fee tiers or different V3 deployments
- Nested `uniswapV3SwapCallback` calls
- **Observed profit**: 0.000113 WETH per trade

#### Route Type D: Multi-Hop Cyclic (Bot 2, 3-4 pools)
**Triangle/quad arbitrage across multiple tokens**

```
USDT ──[V3]──→ WBTC ──[V3]──→ WETH ──[PCS V3]──→ Token ──[V3]──→ USDT
```

- Chains 3-4 swaps across UniV3 + PancakeSwap V3
- Callback nesting 3-4 levels deep
- Rarest but can capture larger mispricing
- **Observed profit**: 0.000002 WBTC (~$0.19)

#### Route Type E: Stablecoin Cross-Pool (Bot 2)
**ARB/stablecoin arbitrage**

```
ARB ──[V3 Pool A]──→ USDC.e ──[V3 Pool B]──→ ARB
```

- Exploits ARB pricing differences between V3 pool pairs
- **Observed profit**: 1.025 USDC.e per trade

### Known Execution Venues (from traces)

| Venue | Type | Selector | Callback Selector | Pairs Observed |
|-------|------|----------|-------------------|----------------|
| Camelot (Algebra v1.9) | Concentrated liquidity | `0x128acb08` | `0x2c8958f6` | WETH/USDC.e, WETH/ARB |
| Uniswap V3 | Concentrated liquidity | `0x128acb08` | `0xfa461e33` | WETH/USDC, ARB/USDC.e, various |
| PancakeSwap V3 | Concentrated liquidity | `0x128acb08` | `0x23a69e75` | WETH/various |
| Uniswap V2 | Constant product | `0x022c0d9f` | N/A (pull-based) | WETH/various |
| Uniswap V4 | Singleton pool | `0x48c89491` | `0x91dd7346` | Various |

### Specific Pool Addresses (from successful txs)

| Pool Address | Pair | Protocol | Used By |
|-------------|------|----------|---------|
| `0xb1026b8e7276e7ac75410f1fcbbe21796e8f7526` | WETH/USDC.e | Camelot | Bot 1 |
| `0xe51635ae8136abac44906a8f230c2d235e9c195f` | WETH/ARB | Camelot | Bot 2 |
| `0xc6962004f452be9203591991d15f6b388e09e8d0` | WETH/USDC | UniV3 | Bot 1 |
| `0x8f5cd460d57ac54e111646fc569179144c7f0c28` | WETH/? | V3/Algebra | Bot 2 |
| `0xb7cc7bf2d593e301d7a1cecf2834a1e8d03c8ab1` | WETH/? | V3/Algebra | Bot 2 |
| `0xdb07...` | WETH/? | UniV2 | Bot 2 |
| `0xa832...` | ARB/USDC.e | V3 | Bot 2 |
| `0xcda5...` | USDC.e/ARB | V3 | Bot 2 |
| `0x5867...` | USDC/? | V2 | Bot 2 |
| `0x536f...` | USDC/? | V3 | Bot 2 |

---

## 6. Profitability Data

### Bot Cluster Overview (Deployer `0x33eabd63`)

| Bot Contract | Reads Pool | Sample Period | Total Txs | Success Rate |
|-------------|------------|---------------|-----------|--------------|
| `0x4ad74bc5...` | WETH/USDC.e | 1.4 days (Jan 2026) | 10,000 | 4.6% (458 hits) |
| `0xea4c5299...` | WETH/ARB | 1.4 days (Jan 2026) | 10,000 | ~4-5% |

### Operator Wallets

**NOTE**: The original operator EOAs (`0x30959c64...`, `0xb70cfc15...`, `0x2a9e1ba9...`) from
earlier analysis had **zero nonce and zero balance** on Arbitrum — they never sent a transaction
on this chain. The actual operators calling the executor contract are:

| EOA Address | Role | Tx Share |
|-------------|------|----------|
| `0x757210e76f4bb56be8fccf7152c456c929442ddf` | Primary operator | ~50% (64/127 successful in sample) |
| `0xc8409740c59e6ad2a87cb03b28c892fe904a1d57` | Secondary operator | ~50% (63/127 successful in sample) |

Both send transactions directly to the executor contract `0x1b61a41fcd...`.
Success rate: ~32% (127 successful out of 400 sampled).

### Profit Distribution Wallets

| Address | Role | Aggregate Share |
|---------|------|-----------------|
| `0x33eabd63853e74ff70d8b89982dfb5bc3eb0a189` | Deployer | 51.7% |
| `0x743be0db30148336a3db479f19d4e1828b293869` | Operator wallet (multi-chain active) | 48.3% |

### Bot 1 Detailed P&L (`0x4ad74bc5` — WETH/USDC reader)

Over a 1.4-day sample (10,000 transactions):

| Metric | Value |
|--------|-------|
| Total transactions | 10,000 |
| Successful (profitable) | 458 (4.6%) |
| Failed (reverted) | 9,542 (95.4%) |
| Total gas spent | 0.0297 ETH (~$57.70) |
| Gas per failed tx | ~0.0000025 ETH (~$0.005) |
| Gas per successful tx | ~0.000050 ETH (~$0.10) |

**Note**: Bot 1 profit extraction uses `WETH.withdrawTo()` which emits `Transfer(from, 0x0000..., amount)`
(looks like a burn in logs) but actually converts WETH→ETH and sends native ETH to a recipient.
Every successful tx has exactly **2 `withdrawTo()` calls** splitting profit between:
- `0x33eabd63...` (deployer) — ~51.7% aggregate
- `0x743be0db...` (operator's second wallet) — ~48.3% aggregate

Both wallets are owned by the same operator. To track P&L accurately, sum both `withdrawTo()`
amounts in the call trace, not just Transfer events.

### Bot 2 Detailed P&L (`0xea4c5299` — WETH/ARB reader)

Over a 1.4-day sample:

| Token | Profit Amount | USD Value (approx) |
|-------|--------------|---------------------|
| USDC | 2,536.24 | $2,536 |
| USDT | 794.76 | $795 |
| USDC.e | 518.90 | $519 |
| DAI | 200.46 | $200 |
| ARB | 93.50 | $8.80 |
| WETH | 0.43 | $835 |
| WBTC | 0.015 | $1,425 |
| **Total** | | **~$6,320** |
| Gas cost | 0.044 ETH | ~$85 |
| **Net profit** | | **~$6,235 / 1.4 days** |
| **Annualized** | | **~$1.6M/year** |

### Combined Cluster Estimate (Jan 2026 — Bot 2 token transfers)

| Metric | Value |
|--------|-------|
| Daily revenue (both bots) | ~$4,000-6,000+ |
| Daily gas cost | ~$100-150 |
| Daily net profit | ~$4,000-5,500 |
| Success rate | ~4-5% |
| Avg profit per hit | ~$10-15 |
| Tx frequency | ~7,000-15,000 txs/day per bot |

### Bot 1 Profit Trends (Feb 4-22 2026 — 65 traced txs via withdrawTo)

**Sample**: 2,938 successful txs over 18 days (~163 hits/day), with 65 evenly-sampled
transactions traced via `debug_traceTransaction` for exact profit amounts.

#### Profit Distribution

| Metric | ETH | USD (@ $1,950) |
|--------|-----|----------------|
| Min | 0.0000352 | $0.07 |
| 25th percentile | 0.0000804 | $0.16 |
| **Median** | **0.000160** | **$0.31** |
| **Mean** | **0.01887** | **$36.80** (skewed by outlier) |
| 75th percentile | 0.000830 | $1.62 |
| 90th percentile | 0.003812 | $7.43 |
| Max | 1.1452 | **$2,233** |

The median of **$0.31** per trade is the true typical profit. The mean is pulled up
by one massive 1.145 ETH ($2,233) outlier on Feb 5.

#### Profit Decline Trend

**Profits declined ~96% over the 18-day sample period:**

```
Feb 5:     0.129  ETH avg/trade  ████████████████████████████████ ($251)
Feb 6-8:   0.0006 ETH avg/trade  █                               ($1.17)
Feb 9-12:  0.0009 ETH avg/trade  █                               ($1.75)
Feb 13-14: 0.005  ETH avg/trade  ████                            ($9.75)
Feb 15-18: 0.0003 ETH avg/trade  ▏                               ($0.58)
Feb 19-22: 0.0005 ETH avg/trade  ▏                               ($0.97)
```

This steep decline suggests increasing competition and/or narrowing price dislocations.
The brief spike on Feb 13-14 shows opportunities are episodic — large dislocations
still appear but are increasingly rare.

#### Profit by Route Complexity

| Swap Legs | Frequency | Avg Profit (ETH) | Avg Profit (USD) |
|-----------|-----------|-------------------|------------------|
| 1 | 19% | 0.000692 | $1.35 |
| 2 | 25% | 0.000412 | $0.80 |
| 3 | 29% (most common) | 0.001403 | $2.74 |
| 4 | 17% | 0.000723 | $1.41 |
| 5 | 8% | 0.230050 | $448.60 (outlier) |
| 6 | 3% | 0.013389 | $26.11 |

**3-leg routes are the sweet spot** — most frequent AND above-average profit.

#### Top Pools (from 65 traced successful txs)

| Pool Address | Appearances | Share |
|-------------|-------------|-------|
| `0x641c00a822e8b671738d32a431a4fb6074e5c79d` | 28 | **43%** |
| `0xc6962004f452be9203591991d15f6b388e09e8d0` (UniV3 WETH/USDC) | 26 | **40%** |
| `0xdd65ead5c92f22b357b1ae516362e4a98b1291ce` | 10 | 15% |
| `0x9804ba22f87728bcb99ffa7041e659768df4dd4f` | 7 | 11% |
| `0xc6f780497a95e246eb9449f5e4770916dcd6396a` | 7 | 11% |
| `0xb1026b8e...` (Camelot WETH/USDC.e) | 4 | 6% |

Two pools dominate: `0x641c00a8...` and the UniV3 WETH/USDC pool.

#### Callback Distribution

| Callback | Frequency | Share |
|----------|-----------|-------|
| `uniswapV3SwapCallback` | 59 of 65 | **91%** |
| `pancakeV3SwapCallback` | 14 of 65 | 22% |
| `algebraSwapCallback` | 12 of 65 | 19% |

(Percentages >100% because a single tx can use multiple callback types.)

### Timing Data (Feb 2026 — 2,938 successful txs)

| Metric | Value |
|--------|-------|
| Successful trades per day | ~163 |
| Median time between hits | **88 seconds** |
| Mean time between hits | 9.1 minutes (skewed by gaps) |
| Same-block clusters | 467 blocks with 2+ trades (up to 6 per block) |
| Peak hour (UTC) | 16:00 (205 trades, 7.0% of total) |
| Quietest hours (UTC) | 02:00-03:00 (41-64 trades) |
| Activity spread | Fairly even across 24h, slight peaks at 11, 15-16, 20 UTC |

### Profit Distribution by Token (Bot 2, Jan 2026)

Based on observed output tokens, the bot does NOT always end in the same token.
The profit depends on which route was most profitable:

```
USDC   ████████████████████████████████████████   40%
WETH   ██████████████████                         13%
WBTC   ██████████████████████████                 23%
USDT   ████████████████                           13%
USDC.e ██████████                                  8%
DAI    ████                                        3%
ARB    ██                                          1%
```

---

## 7. Function Selectors & ABIs

### Core Selectors Used by the Bot

#### Price Reading

| Selector | Function | Protocol | Purpose |
|----------|----------|----------|---------|
| `0xe76c01e4` | `globalState()` | Algebra/Camelot | Read Algebra pool price |
| `0x3850c7bd` | `slot0()` | Uniswap V3 | Read V3 pool price |
| `0x0dfe1681` | `token0()` | All pools | Verify token ordering |
| `0xd21220a7` | `token1()` | All pools | Verify token ordering |
| `0x70a08231` | `balanceOf(address)` | ERC-20 | Check token balance |
| `0x1a686502` | Unknown | Algebra | Additional state read |
| `0xd0c93a7c` | Unknown | Algebra | Additional state read |

#### Swap Execution

| Selector | Function | Protocol | Direction |
|----------|----------|----------|-----------|
| `0x128acb08` | `swap(address,bool,int256,uint160,bytes)` | Algebra/UniV3/PCS | Execute swap |
| `0x022c0d9f` | `swap(uint,uint,address,bytes)` | Uniswap V2 | Execute V2 swap |
| `0x48c89491` | `unlock(bytes)` | Uniswap V4 | Begin V4 swap |

#### Callbacks (your contract must implement these)

| Selector | Function | Protocol | When Called |
|----------|----------|----------|------------|
| `0x2c8958f6` | `algebraSwapCallback(int256,int256,bytes)` | Algebra v1.9 | After Algebra swap |
| `0xfa461e33` | `uniswapV3SwapCallback(int256,int256,bytes)` | Uniswap V3 | After V3 swap |
| `0x23a69e75` | `pancakeV3SwapCallback(int256,int256,bytes)` | PancakeSwap V3 | After PCS swap |
| `0x91dd7346` | `unlockCallback(bytes)` | Uniswap V4 | After V4 unlock |

#### Token Operations

| Selector | Function | Protocol | Purpose |
|----------|----------|----------|---------|
| `0xa9059cbb` | `transfer(address,uint256)` | ERC-20 | Move tokens / pay pool |
| `0x23b872dd` | `transferFrom(address,address,uint256)` | ERC-20 | Move tokens |
| `0x095ea7b3` | `approve(address,uint256)` | ERC-20 | Approve spend |

#### Bot-Specific (custom packed selectors)

| Selector | Used By | Meaning |
|----------|---------|---------|
| `0x00000020` | Bot 2 | Primary route execution (93% of txs) |
| `0x00000021` | Bot 2 | Secondary route execution (7% of txs) |
| `0x359ecc85` | Bot 1 reader | State reading function (called by executor) |
| `0x67d8fe79` | Bot 1 executor | Main entry point |
| `0x92ead328` | Bot 2 executor | Swap delegation entry point |

#### TWAP (NOT used — for reference only)

| Selector | Function | Protocol | Notes |
|----------|----------|----------|-------|
| `0x1749209f` | `getTimepoints(uint32[])` | Algebra v1.9 | Zero external callers found |

### Algebra v1.9 `globalState()` Return Decoding

```
Response: 32 bytes sqrtPriceX96 | 32 bytes tick | 32 bytes fee | ...

Offset 0x00 (bytes 0-31):   uint160 sqrtPriceX96
Offset 0x20 (bytes 32-63):  int24   tick
Offset 0x40 (bytes 64-95):  uint16  fee
Offset 0x60 (bytes 96-127): uint16  timepointIndex
Offset 0x80 (bytes 128-159): uint16 communityFeeToken0
Offset 0xa0 (bytes 160-191): uint16 communityFeeToken1
Offset 0xc0 (bytes 192-223): bool   unlocked
```

### Swap Interfaces

```solidity
// Algebra v1.9 / Camelot
interface IAlgebraPool {
    function globalState() external view returns (
        uint160 sqrtPriceX96, int24 tick, uint16 fee,
        uint16 timepointIndex, uint16 communityFeeToken0,
        uint16 communityFeeToken1, bool unlocked
    );
    function swap(
        address recipient,
        bool zeroToOne,        // true = sell token0, false = sell token1
        int256 amountRequired, // positive = exact input, negative = exact output
        uint160 limitSqrtPrice,
        bytes calldata data
    ) external returns (int256 amount0, int256 amount1);
}

interface IAlgebraSwapCallback {
    function algebraSwapCallback(
        int256 amount0Delta, int256 amount1Delta, bytes calldata data
    ) external;
}

// Uniswap V3 (same swap signature, different callback)
interface IUniswapV3Pool {
    function slot0() external view returns (
        uint160 sqrtPriceX96, int24 tick, uint16 observationIndex,
        uint16 observationCardinality, uint16 observationCardinalityNext,
        uint8 feeProtocol, bool unlocked
    );
    function swap(
        address recipient, bool zeroForOne, int256 amountSpecified,
        uint160 sqrtPriceLimitX96, bytes calldata data
    ) external returns (int256 amount0, int256 amount1);
}

interface IUniswapV3SwapCallback {
    function uniswapV3SwapCallback(
        int256 amount0Delta, int256 amount1Delta, bytes calldata data
    ) external;
}

// PancakeSwap V3 (same swap signature, different callback)
interface IPancakeV3SwapCallback {
    function pancakeV3SwapCallback(
        int256 amount0Delta, int256 amount1Delta, bytes calldata data
    ) external;
}

// Uniswap V2
interface IUniswapV2Pair {
    function swap(
        uint amount0Out, uint amount1Out, address to, bytes calldata data
    ) external;
    function getReserves() external view returns (
        uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast
    );
}

// Uniswap V4
interface IPoolManager {
    function unlock(bytes calldata data) external returns (bytes memory);
}
interface IUnlockCallback {
    function unlockCallback(bytes calldata data) external returns (bytes memory);
}
```

---

## 8. Contract Architecture

### Observed Architecture: Multi-Contract System

The live bot uses a **3-contract architecture** (Bot 1) or **2-contract architecture**
(Bot 2), NOT a single monolithic contract:

```
Bot 1 Architecture (3 contracts):

  ┌─────────────┐     ┌──────────────┐     ┌──────────────┐
  │   Reader     │     │   Executor   │     │    Router    │
  │ 0x4ad74bc5   │◄────│ 0x1b61a41f   │────►│ 0xa1ff0ea6   │
  │              │     │              │     │              │
  │ - globalState│     │ - Orchestrate│     │ - swap()     │
  │ - slot0()    │     │ - Route sel. │     │ - callbacks  │
  │ - Assess opp │     │ - Profit mgmt│     │ - Token xfer │
  └──────────────┘     └──────────────┘     └──────────────┘
        ▲                     ▲                     │
        │ STATICCALL          │ tx.to               │ CALL
        │                     │                     ▼
     [Pools]              [EOA]               [DEX Pools]


Bot 2 Architecture (2 contracts):

  ┌─────────────────┐     ┌──────────────┐
  │   Bot + Reader   │     │   Executor   │
  │  0xea4c5299      │────►│ 0xfc7dc4f6   │
  │                  │     │              │
  │ - globalState    │     │ - swap()     │
  │ - slot0()        │     │ - callbacks  │
  │ - Assess opp     │     │ - Token xfer │
  │ - Route dispatch │     │              │
  └──────────────────┘     └──────────────┘
```

### Why separate contracts?

1. **Upgradeability**: Deploy new router without redeploying the reader
2. **Gas optimization**: Reader can be tiny (only STATICCALLs); router has the heavy logic
3. **Code reuse**: Same executor can work with different readers for different pool pairs
4. **Obfuscation**: Harder to reverse-engineer the full strategy from a single contract

### Recommended Contract Structure

```solidity
// Contract 1: Reader (deployed once, rarely changed)
contract PriceReader {
    function checkOpportunity(bytes calldata params)
        external view returns (bool profitable, bytes memory routeData)
    {
        // Read globalState() from Camelot pools
        // Read slot0() from UniV3 pools
        // Compare prices
        // Return whether profitable + encoded route
    }
}

// Contract 2: Executor (main entry point)
contract Executor {
    address public reader;
    address public router;
    address public owner;

    function execute(bytes calldata params) external {
        require(msg.sender == owner);

        // Phase 1: Check opportunity
        (bool profitable, bytes memory routeData) = reader.staticcall(
            abi.encodeCall(PriceReader.checkOpportunity, params)
        );
        require(profitable, "no opportunity");

        // Phase 2: Execute via router
        router.call(abi.encodeWithSelector(0x92ead328, routeData));

        // Phase 3: Extract profit
        // Split via WETH.withdrawTo() to operator wallets
    }
}

// Contract 3: Router (implements all swap callbacks)
contract SwapRouter {
    function executeRoute(bytes calldata routeData) external {
        // Decode route: which pools, directions, amounts
        // Initiate first swap (callback-chained)
    }

    function algebraSwapCallback(int256 a0, int256 a1, bytes calldata data) external {
        // Validate msg.sender is expected pool
        // Either: send tokens to pool (funded by previous swap output)
        // Or: initiate next swap in the chain
    }

    function uniswapV3SwapCallback(int256 a0, int256 a1, bytes calldata data) external {
        // Same pattern as algebraSwapCallback
    }

    function pancakeV3SwapCallback(int256 a0, int256 a1, bytes calldata data) external {
        // Same pattern
    }
}
```

### Callback Chaining Implementation Pattern

```solidity
// Inside algebraSwapCallback:
function algebraSwapCallback(
    int256 amount0Delta,
    int256 amount1Delta,
    bytes calldata data
) external {
    // Validate caller is the expected Algebra pool
    require(msg.sender == expectedPool);

    // Decode which token the pool wants
    if (amount0Delta > 0) {
        // Pool wants token0 from us
        // Fund it by swapping on the next venue
        IUniswapV3Pool(nextPool).swap(
            msg.sender,           // Send output directly to Algebra pool
            zeroForOne,
            int256(amount0Delta), // Exact output needed
            sqrtPriceLimit,
            abi.encode(nextCallbackData)
        );
    }
    if (amount1Delta > 0) {
        // Pool wants token1 — same pattern
    }
}
```

### Security Patterns Observed

1. **Caller restriction**: Only specific EOAs can call `execute()`
2. **Callback validation**: Each callback checks `msg.sender` is the expected pool
3. **Atomic execution**: Everything happens in one tx — no state between calls
4. **Profit extraction**: Two patterns observed:
   - **Bot 1**: Convert profit to ETH via 2x `WETH.withdrawTo()` → split between deployer + operator wallet
   - **Bot 2**: Transfer profit tokens directly to deployer EOA
5. **No token storage**: Contract holds no tokens between transactions
6. **Packed calldata**: Using `0x00000020`/`0x00000021` instead of proper function selectors minimizes calldata size (saves L1 data posting fee on Arbitrum)

---

## 9. Gas & Economics

### Cost Breakdown

| Component | Gas | USD (at 0.03 gwei, ETH=$1,942) |
|-----------|-----|--------------------------------|
| Failed tx (revert after price check) | ~35,000-78,000 | $0.001-0.005 |
| globalState() STATICCALL (per pool) | ~2,600 | $0.00005 |
| slot0() STATICCALL (per pool) | ~2,600 | $0.00005 |
| Algebra swap leg | ~80,000-100,000 | $0.005 |
| UniV3 swap leg | ~80,000-120,000 | $0.006 |
| UniV2 swap leg | ~60,000-80,000 | $0.004 |
| ERC-20 transfers (2-3) | ~50,000-75,000 | $0.003 |
| **Total successful 2-leg tx** | **~260,000-550,000** | **$0.01-0.03** |
| **Total successful 4-leg tx** | **~550,000-600,000** | **$0.03-0.05** |

### Profit Extraction Economics (Bot 1)

Bot 1 converts arb profit to ETH via `WETH.withdrawTo()` and splits it between
two operator-owned wallets. No external fees, bribes, or priority mechanisms.

| Metric | Value |
|--------|-------|
| Gross profit per hit (example) | ~$3.65 |
| Gas cost per hit | ~$0.02-0.03 |
| Net profit per hit | ~$3.62 |
| Priority mechanism | None — Legacy type 0 tx, base fee only |
| maxPriorityFeePerGas | 0 |
| Tx position in block | 4-8 (not first) |
| Profit split | 2 `withdrawTo()` calls per tx (see below) |

**Profit split details** (from 15 traced successful txs):

| Split Pattern | Frequency | Deployer Share | Wallet A Share |
|--------------|-----------|----------------|----------------|
| 50/50 | 10 of 15 (67%) | 50.0% | 50.0% |
| ~46.5/53.5 | 3 of 15 (20%) | 53.5% | 46.5% |
| ~2/98 | 2 of 15 (13%) | 1.4% | 98.6% |

Aggregate across all 15 txs: deployer 51.7% (0.00846 ETH), wallet A 48.3% (0.00792 ETH).
Both wallets are owned by the same operator (`0x743be0db...` is active across multiple chains).
The variable split may depend on trade size, route type, or an internal accounting rule.

**No Timeboost**: Timeboost express lane (type 105 txs) does not appear to be active
on Arbitrum One as of this analysis. Zero type 105 transactions were found in recent
blocks. The bot uses standard Legacy (type 0) transactions at base fee with no priority tip.

**Why does it work without priority?** On Arbitrum's sequencer, transactions are
ordered by arrival time (FCFS). The bot relies on:
1. Fast off-chain detection + submission speed
2. Two rotating operator EOAs (`0x757210e7...` and `0xc8409740...`)
   for nonce management and redundancy
3. ~50/50 tx distribution across both EOAs
4. High submission rate (~4.1 tx/min) to catch fleeting opportunities

### Economic Model

```
Bot 1 (Feb 2026 — from 65 traced withdrawTo txs):
  Median profit per hit: $0.31
  Mean profit per hit:   $36.80 (skewed by $2,233 outlier)
  Avg gas per hit:       $0.025
  Median profit/gas:     15.7x
  Min profit/gas:        4x (every trade profitable after gas)
  Hits per day:          ~163
  Gas cost per day:      ~$4 (163 × $0.025)

  Revenue model (conservative, using median):
    163 hits × $0.31 median = ~$50/day
  Revenue model (mean, includes outliers):
    163 hits × $36.80 mean = ~$6,000/day

  Reality is in between — profits are lumpy. A few big hits
  per week drive most revenue. Most trades are micro-profits.

Bot 2 model (Jan 2026 — from token transfer analysis):
  Net profit per hit: ~$0.06-1.03 (wide range)
  Gas per hit: ~$0.03-0.16
  Hits per day: ~300-450
  Net per day: ~$4,000-5,500
```

### Gas Efficiency (Feb 2026 — 65 traced txs)

| Metric | Value |
|--------|-------|
| Mean gas used per successful tx | 550,860 |
| Median gas used | 491,503 |
| Mean gas cost | 0.000013 ETH (~$0.025) |
| Median profit-to-gas ratio | **15.7x** |
| Mean profit-to-gas ratio | 403.5x (skewed by outlier) |
| Min profit-to-gas ratio | 4x |
| Gas-profit correlation (Pearson r) | 0.50 (moderate positive) |

More complex (gas-heavy) trades tend to extract more profit, but even the
cheapest single-leg trades return 4x gas cost minimum.

### Timing Economics

| Metric | Value |
|--------|-------|
| Successful hits per day | ~163 |
| Median gap between hits | 88 seconds |
| Same-block clusters | 467 blocks with multiple hits |
| Gas cost per day | ~$4 |
| Revenue per day (median model) | ~$50 |
| Revenue per day (mean model) | ~$6,000 (outlier-driven) |

### Method ID Distribution

| Selector | Frequency | Meaning |
|----------|-----------|---------|
| `0x00000020` | 93% (466/500) | Primary route type |
| `0x00000021` | 7% (34/500) | Secondary route type |

### Why the revert-on-failure pattern works on Arbitrum

On Ethereum mainnet, failed transactions cost $5-50+ each, making the "spray and pray"
approach unprofitable. On Arbitrum:

- Failed tx gas: ~35,000-78,000
- At 0.01-0.1 gwei: **$0.001-0.005 per failure**
- Even with 95% failure rate, 10,000 failed txs cost only ~$10-50
- A single $1+ hit covers hundreds of failures

### Optimal Parameters (Derived from Observation)

| Parameter | Observed Value | Notes |
|-----------|---------------|-------|
| Min profit threshold | ~$0.05-0.50 | Very low threshold viable due to cheap gas |
| Trade size range | $65-$1,870 | Varies by route |
| Tx submission rate | ~4.1 per minute | Per bot contract |
| Gas price | 0.01-0.1 gwei | Use current L2 gas price |
| Gas limit | 600,000 | Sufficient for 4-leg arb |
| Priority mechanism | None (Legacy type 0, base fee) | FCFS ordering — speed is everything |

---

## 10. Risk Management

### Known Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Pool liquidity drain | Medium | Check liquidity before sizing trades |
| Frontrunning by other bots | Medium | Speed (FCFS ordering) + multi-EOA redundancy |
| Smart contract bug | High | Extensive testing on forked mainnet |
| Callback reentrancy | High | Validate `msg.sender` in every callback |
| Token depegging | Low | Use tight slippage limits |
| Gas price spike | Low | Arbitrum gas is stable and cheap |
| Pool contract upgrade | Low | Monitor factory events |
| Sequencer downtime | Medium | Bot automatically stops (all txs would fail) |

### Key Invariants

1. **Swap callbacks must be fully satisfied** or the whole tx reverts
2. **No partial execution** — either all legs succeed or all revert
3. **No capital at risk** between transactions (callback chaining pattern)
4. **Callback sender validation** — always verify `msg.sender` is the expected pool
5. **No token approvals needed** — tokens flow through callbacks, not approvals

### Monitoring Checklist

- [ ] Track success rate — should be 3-6%. Below 2% = market too efficient
- [ ] Track avg profit per hit — declining = more competition
- [ ] Monitor new bot deployments to same pools (watch deployer addresses)
- [ ] Watch for pool parameter changes (fee tier changes)
- [ ] Monitor for Timeboost activation (type 105 txs appearing = new priority mechanism)
- [ ] Check for new Camelot/Algebra pool deployments (new trading pairs = new opportunities)
- [ ] Track WETH/USDC.e pool swap volume (~43 swaps per 20 min as baseline)
- [ ] Monitor `0x00000020` vs `0x00000021` selector ratio for route health

---

## Appendix A: Reference Bot Addresses

### Deployer & Operators

| Role | Address |
|------|---------|
| Deployer (profit wallet A) | `0x33eabd63853e74ff70d8b89982dfb5bc3eb0a189` |
| Operator (profit wallet B) | `0x743be0db30148336a3db479f19d4e1828b293869` |
| Operator EOA 1 (tx sender) | `0x757210e76f4bb56be8fccf7152c456c929442ddf` |
| Operator EOA 2 (tx sender) | `0xc8409740c59e6ad2a87cb03b28c892fe904a1d57` |

Note: Earlier analysis identified `0x30959c64...`, `0xb70cfc15...`, `0x2a9e1ba9...` as
operators, but these have zero nonce on Arbitrum and never sent transactions on-chain.
They may be operators for Bot 2 (reader `0xea4c5299...`) or for a different chain.

### Bot Contracts

| Contract | Role | Address | Deployed Block |
|----------|------|---------|----------------|
| Bot 1 Reader | Price reading | `0x4ad74bc56f70cae4ae1308f62a18d15a1a556aaf` | 428,450,061 |
| Bot 2 Reader+Entry | Price reading + dispatch | `0xea4c5299b308fa6a220a1184e94c36f60efd397d` | 407,629,267 |
| Bot 1 Executor | Orchestration | `0x1b61a41fcd...` | Unknown |
| Bot 1 Router | Swap execution | `0xa1ff0ea658...` | Unknown |
| Bot 2 Executor | Swap execution | `0xfc7dc4f6...` | Unknown |
| Shared component | Common utility | `0x8b194bea...` | Unknown |

### Sample Profitable Transactions (for reference tracing)

Trace these with `debug_traceTransaction` + `callTracer` to study exact call flows:

```
# Bot 1: Algebra ↔ UniV3 arb ($3.68 profit, split via 2x withdrawTo)
0x4dc9980d305cb63435d75d915220ab0641c6d54690dd9e0d0a9ad21d29502aa2

# Bot 1: Reverted (no opportunity) — shows price reading pattern
0x05093c2f25779588e423e6dc35c869e5bf2c7c3ee6fcee35ba42c57cd95066c3
0x32be1cf6d82014078408f3243a704b4e693a44650a4f15d9b4cc22aacb5f9f83

# Bot 2: V3-to-V2 arb (0.000031 WETH profit)
0x2316a320...

# Bot 2: V3-to-V3 arb (0.000113 WETH profit)
0x375f4dea...

# Bot 2: V3-to-V2 USDC arb (0.166 USDC profit)
0x52e71bd6...

# Bot 2: 4-pool cyclic USDT/WBTC/WETH (0.000002 WBTC profit)
0x9c0d0cdb...

# Bot 2: ARB/USDC.e cross-V3 arb (1.025 USDC.e profit)
0x8d8510d8...
```

### All 65 Traced Transactions (Bot 1, Feb 4-22 2026)

Sorted by profit (descending). Full data in `data/profit_trends.json`.

| TX Hash | Legs | Profit (ETH) | Profit (USD) | Gas | Pools Used |
|---------|------|-------------|-------------|-----|------------|
| `0x65d72b52769af83f1461f1bef8210fa7620cee84...` | 5 | 1.145229 | $2,233.20 | 1,262,067 | WETH/USDT(V3), WETH/USDT(V3-0.3%) |
| `0xd8ddb8a8aacc87982c9f179e4f9411ae99c5ac12...` | 6 | 0.025496 | $49.72 | 865,175 | HERMES/WETH, MAIA/HERMES, MAIA/WETH |
| `0x243b706f6f710f23f25e17b216e02032593a421a...` | 3 | 0.008982 | $17.52 | 1,068,978 | WETH/USDC(V3), WBTC/WETH(V3) |
| `0x901447c67961dbf36bd02f6e2cf41fc72a5c75fc...` | 3 | 0.006992 | $13.64 | 649,410 | WETH/USDC(V3), WETH/USDC(V3-0.01%) |
| `0xcd8fa661343a5dd06a464c66ada4fad894d8295e...` | 3 | 0.005234 | $10.21 | 491,503 | WETH/USDC(V3), ?, ZRO/WETH |
| `0x6a67d25b96d99633fd5ec6f109c708ce7134f4f8...` | 2 | 0.004167 | $8.13 | 436,750 | RAIN/WETH(V3), ? |
| `0x3b72908c2917943550a9c3a19d90e224aba248f8...` | 4 | 0.003967 | $7.74 | 708,351 | USDC/USDC.e(Camelot), ?, ? |
| `0xf0f8607cc5defea734bad5c339642955e3671226...` | 1 | 0.003579 | $6.98 | 636,370 | ?(single pool) |
| `0x56bec897cb707cf0cc1bb3c015aaf5c348ac059b...` | 4 | 0.002821 | $5.50 | 630,213 | WETH/USDT(V3), WETH/USDC(V3), ? |
| `0xf4943774fc4d8a0ff0e9d62c2096a6092e5ac27a...` | 5 | 0.002801 | $5.46 | 642,958 | WETH/USDC.e(Camelot), WETH/USDC(V3) |
| `0x414f889d2bb3f942cfa12b39895752cc2b8acd49...` | 1 | 0.002031 | $3.96 | 427,284 | ?(single pool) |
| `0xdf4a537c9e67b94f53717d3d649e5065afe8d80f...` | 3 | 0.001550 | $3.02 | 435,260 | Algebra pools |
| `0x39f0cf5dc228d1fc75b112dab33d0e93b217f2c5...` | 5 | 0.001468 | $2.86 | 980,588 | #BTB/USDC(PCS), #BTB/LYK(PCS) |
| `0xf06216743e16814002bb595ed7abdd35210539f8...` | 6 | 0.001283 | $2.50 | 659,436 | WETH/USDT(V3), ZRO/USDT, ZRO/WETH |
| `0x5efe46b862115766320b9d98e1b86b5a6f9424ba...` | 1 | 0.001157 | $2.26 | 445,397 | ZRO/WETH(V3) |
| `0x2bf88b239fd14fbb483ab21ba07dfa9ab2bd05ef...` | 2 | 0.000867 | $1.69 | 775,522 | ?(Algebra+V3) |
| `0x016f4fb419a10e49027ff12e09030e017533effd...` | 3 | 0.000830 | $1.62 | 546,409 | ?, WBTC/USDT(V3), WETH/USDT(V3) |
| `0x703b643bd4dd1c640fb29bc9e04a9b01f9453ff0...` | 3 | 0.000758 | $1.48 | 547,500 | WETH/USDT(V3) x3 |
| `0x0ccc87477d0f912b6386c468a48147add7f5da61...` | 5 | 0.000680 | $1.33 | 785,515 | WETH/USDC(V3), WETH/ARB(V3) |
| `0x6ddbcac2ab71ec54810b052739ed8973eab5fa37...` | 1 | 0.000485 | $0.95 | 452,856 | WETH/LINK(PCS) |
| `0x149503f32419bb5cff27741f053b14f0a2bfbe4c...` | 3 | 0.000443 | $0.86 | 465,988 | WETH/USDC(V3), WETH/USDC(V3-0.01%) |
| `0xf81c4e1be24fc301c8f08df36fad4dcdf42ddc94...` | 3 | 0.000428 | $0.83 | 800,253 | ?, USDC/USDC.e(Camelot), WETH/USDC.e(V3) |
| `0x0c0ab4ed2a87965f25754d27f7f3a952b2cd3329...` | 2 | 0.000353 | $0.69 | 767,042 | ?, WETH/USDT(V3) |
| `0xc6290cace97d5f78c2a5cab0b66e0c58e111163f...` | 4 | 0.000312 | $0.61 | 481,186 | WETH/ARB(V3), ?, ? |
| `0xbed286bd382214f3d475b1228770b7a167aa873b...` | 1 | 0.000280 | $0.55 | 460,459 | WETH/LINK(PCS) |
| `0xe21d62743ca47e1d22ae1a5e896ccb28434ce36b...` | 3 | 0.000263 | $0.51 | 634,011 | WETH/USDC(V3), ?, WETH/USDC(V3) |
| `0x027ca125286b21896741592bc557ba2de8526b24...` | 2 | 0.000257 | $0.50 | 359,391 | ?, RAIN/WETH(V3) |
| `0x216024d9a2221712f6bb38debc2f22d50f716ee9...` | 1 | 0.000242 | $0.47 | 384,703 | ?(single pool) |
| `0xe1339b3faf9821ca5e09ddc0343e278a5abf079a...` | 1 | 0.000196 | $0.38 | 713,898 | WETH/USDC(V3) |
| `0x8e74ca90a16f5ddab9d0af533e1cf1f09066eead...` | 3 | 0.000188 | $0.37 | 604,487 | WBTC/USDT(V3), ?, WBTC/WETH(V3) |
| `0x19f148fcfbda8f61a04cb4ebec1a382c2f9742a8...` | 3 | 0.000176 | $0.34 | 630,596 | SOL/WETH(V3), ?, WETH/USDC(V3) |
| `0x942a5d88e92ae2fc52571db7eb451931041ec75d...` | 4 | 0.000175 | $0.34 | 508,022 | WETH/USDT(V3), WETH/ARB(V3), ? |
| `0xef16e7af2cb75557d80ba2a213f6e8afec3cab19...` | 3 | 0.000160 | $0.31 | 362,669 | OTM/USDT(V3), OTM/USDT(?), WETH/USDT(V3) |
| `0xa4fba4f88534d680e7dca2219aa028858ef1d831...` | 1 | 0.000151 | $0.29 | 664,642 | WETH/USDC(V3) |
| `0x608b28e1bc5ec0dc069786665558927eeb7d8f6a...` | 4 | 0.000140 | $0.27 | 441,362 | OTM/USDT(V3) x2, OTM/USDT(?) |
| `0xd92e4a9c852ba268e25ffeb7292ac017b8c1d7cf...` | 3 | 0.000139 | $0.27 | 361,718 | OTM/USDT(V3), OTM/USDT(?), WETH/USDT(V3) |
| `0x99fa1cdee5c105d101bbe3605e608e374204fec3...` | 2 | 0.000134 | $0.26 | 342,524 | ?(Algebra), MAGIC/WETH(Camelot) |
| `0x6ba69b5b4b9bfcc030afdfa9cfd469b1b24979a5...` | 4 | 0.000131 | $0.26 | 554,817 | SOL/WETH(PCS), SOL/WETH(V3) |
| `0xe3d19a35d378f290600f71ccd7ac4ff941fbdbff...` | 2 | 0.000126 | $0.25 | 552,124 | ?, WETH/USDT(V3) |
| `0x719e955edc2cb54bec9d3036ad2cac2456e0c2f6...` | 3 | 0.000126 | $0.24 | 447,016 | WETH/USDC(V3), WETH/USDC.e(Camelot), WETH/USDC(V3) |
| `0x034092543c96c05e819cd07bb4469097494faf47...` | 4 | 0.000120 | $0.23 | 726,017 | ?(V3+Algebra) |
| `0x5dcb3206e08b00a3e0b3fe84b0d16f005cf90e52...` | 3 | 0.000118 | $0.23 | 421,122 | ?, ?, WETH/USDC.e(V3) |
| `0x12961e20b822d5c8968138442f9253f28370741c...` | 3 | 0.000116 | $0.23 | 560,599 | WBTC/WETH(V3), ?, WBTC/WETH(V3) |
| `0x287fb06ecd915ab7c9f8c88783fb9b33dc24aabe...` | 2 | 0.000110 | $0.21 | 611,165 | ?(PCS), WETH/USDC(V3) |
| `0x815f3e4c9c204d4af753d40a8abb489440541a3a...` | 4 | 0.000102 | $0.20 | 442,313 | OTM/USDT(V3) x2, OTM/USDT(?) |
| `0xd0e4a3cde43c8103d3b25081016da275d0b89fa4...` | 4 | 0.000100 | $0.19 | 441,954 | OTM/USDT(V3) x2, OTM/USDT(?) |
| `0xc0afbc1cbaa9c33cedf38c9b34e14a04e7b0855a...` | 2 | 0.000099 | $0.19 | 393,383 | ?(Algebra+V3) |
| `0x094ae977747226b419fd5445e005d5cbe1030ab4...` | 2 | 0.000096 | $0.19 | 293,256 | ?(V3) |
| `0xea4eed9b30b7692a26e391d1aeb366fd16891989...` | 2 | 0.000080 | $0.16 | 400,151 | WETH/AAVE(PCS), WETH/AAVE(V3) |
| `0x638853bf5dc07d8d7d864a3f164b635ecd09f51d...` | 5 | 0.000073 | $0.14 | 547,730 | @G/WETH(V3), WETH/USDT(V3) |
| `0x764c0e34ce20344dc16cc79a857b3271d3169e58...` | 2 | 0.000064 | $0.12 | 677,217 | WETH/ARB(V3), WBTC/WETH(V3) |
| `0x5c71e3e43833601f17b6884ebc25f217d6e11657...` | 2 | 0.000063 | $0.12 | 483,900 | ?, WETH/USDC(V3) |
| `0xd9e2ab62f604e0bd6cc1b473641326decee4c66b...` | 3 | 0.000059 | $0.12 | 470,556 | ?(PCS), ?, WETH/USDT(V3) |
| `0x84251dd2cdc26bc2c11f788f6c8cc14b5d69af5f...` | 1 | 0.000055 | $0.11 | 442,476 | ?(Algebra) |
| `0x4665cc60b2dbcc0ee71d1f902a22d9b64b4334f7...` | 3 | 0.000054 | $0.11 | 614,585 | LYK/ARB(PCS) x2, WETH/ARB(V3) |
| `0x98a81427d38dd9aca5d0ffa64ed97e88bbfa1a2e...` | 2 | 0.000051 | $0.10 | 383,666 | WETH/AAVE(PCS), WETH/AAVE(V3) |
| `0xfbebb2f4a75e24d9e02ba7c07828d31f547d6e26...` | 2 | 0.000049 | $0.10 | 506,606 | ?, WETH/USDT(V3) |
| `0x863a9f261746a9da143889a9b7bd20416c2b3482...` | 1 | 0.000048 | $0.09 | 595,908 | WETH/USDT(V3) |
| `0x762f5fa8aa11799f72d593e8a81c82e61d874d55...` | 1 | 0.000048 | $0.09 | 415,428 | WETH/SETH(V3) |
| `0xb0e23e689e205a2b2586ebdf19abe665983ad0d4...` | 4 | 0.000047 | $0.09 | 462,252 | ?, ?, OTM/USDT(?), WETH/USDT(V3) |
| `0x1b5d97562ceae91c401b1ceef22eeb7ca00c935a...` | 2 | 0.000043 | $0.08 | 285,043 | WETH/SETH(V3), ? |
| `0xe995bb69379d0754f3698df9d96080ec279b3d9f...` | 4 | 0.000043 | $0.08 | 441,351 | OTM/USDT(V3) x2, OTM/USDT(?) |
| `0xf35393fcb9ae5272a0c0c0462ff71757d4cc3258...` | 3 | 0.000042 | $0.08 | 401,898 | WETH/USDC.e(Camelot), WETH/USDC(V3) x2 |
| `0xf0ab32d3dbd76649c2dc4d52672f02e2d7930122...` | 2 | 0.000039 | $0.08 | 385,756 | SOL/WETH(V3), SOL/WETH(PCS) |
| `0x9dae7d01129a8ff47bd5d2ea76c75ed1fb97acd1...` | 1 | 0.000035 | $0.07 | 417,101 | ?(V3) |

### Complete Pool Catalog (28 identified pools from 65 traced txs)

| Pool Address | Pair | Protocol | Fee | Uses |
|-------------|------|----------|-----|------|
| `0x641c00a822e8b671738d32a431a4fb6074e5c79d` | **WETH/USDT** | Uniswap V3 | 0.05% | 28 |
| `0xc6962004f452be9203591991d15f6b388e09e8d0` | **WETH/USDC** | Uniswap V3 | 0.05% | 26 |
| `0xdd65ead5c92f22b357b1ae516362e4a98b1291ce` | OTM/USDT | Uniswap V3 | 1% | 10 |
| `0x9804ba22f87728bcb99ffa7041e659768df4dd4f` | OTM/USDT | Unknown (non-standard) | N/A | 7 |
| `0xc6f780497a95e246eb9449f5e4770916dcd6396a` | WETH/ARB | Uniswap V3 | 0.05% | 7 |
| `0x2f5e87c9312fa29aed5c179e456625d79015299c` | WBTC/WETH | Uniswap V3 | 0.05% | 5 |
| `0xdaf544bcab17e2dcd293c3af28e67c7e8b5a49ee` | SOL/WETH | Uniswap V3 | 0.3% | 4 |
| `0x4cef551255ec96d89fec975446301b5c4e164c59` | ZRO/WETH | Uniswap V3 | 0.3% | 4 |
| `0xb1026b8e7276e7ac75410f1fcbbe21796e8f7526` | WETH/USDC.e | Camelot (Algebra) | dynamic | 4 |
| `0x1e59fa2f0f4e34649fae55222eaf4d730ed35d95` | SOL/WETH | PancakeSwap V3 | 0.05% | 3 |
| `0xc31e54c7a869b9fcbecc14363cf510d1c41fa443` | WETH/USDC.e | Uniswap V3 | 0.05% | 3 |
| `0x6f38e884725a116c9c7fbf208e79fe8828a2595f` | WETH/USDC | Uniswap V3 | 0.01% | 2 |
| `0x977f5d9a39049c73bc26edb3fa15d5f7c0ac82e9` | WETH/LINK | PancakeSwap V3 | 0.05% | 2 |
| `0x36c46b34b306010136dd28bb3ba34f921dab53ba` | LYK/ARB | PancakeSwap V3 | 0.25% | 2 |
| `0x770b4493fbed2584c47caeb8c8f7de74d810c49f` | #BTB/USDC | PancakeSwap V3 | 0.25% | 2 |
| `0x0e65f47449920c4bc2127e5082d755286e07a01a` | #BTB/LYK | PancakeSwap V3 | 0.25% | 2 |
| `0xc86eb7b85807020b4548ee05b54bfc956eebbfcd` | USDC/USDC.e | Camelot (Algebra) | dynamic | 2 |
| `0x04d1e97733131f8f9711d30aed1a7055832033cd` | HERMES/WETH | Uniswap V3 | 1% | 2 |
| `0x72e68515fc898624930b0eafa502b4320b1ede46` | MAIA/HERMES | Uniswap V3 | 1% | 2 |
| `0x5067384e6ad48de6f14732eabe749dc0f02f662f` | MAIA/WETH | Uniswap V3 | 1% | 2 |
| `0xc1bf07800063efb46231029864cd22325ef8efe8` | @G/WETH | Uniswap V3 | 0.3% | 2 |
| `0x80ceb98632409080924dce50c26acc25458dde17` | WETH/AAVE | PancakeSwap V3 | 0.05% | 2 |
| `0x263f7b865de80355f91c00dfb975a821effbea24` | WETH/AAVE | Uniswap V3 | 0.05% | 2 |
| `0xc868f85196facfbbc08b44975f83788dd922e482` | ZRO/USDT | Uniswap V3 | 1% | 2 |
| `0xd13040d4fe917ee704158cfcb3338dcd2838b245` | RAIN/WETH | Uniswap V3 | 0.01% | 2 |
| `0x2ad24e6cb77c2c7f09a5fa3fa5f23f3278046909` | WETH/SETH | Uniswap V3 | 0.3% | 2 |
| `0xc82819f72a9e77e2c0c3a69b3196478f44303cf4` | WETH/USDT | Uniswap V3 | 0.3% | 2 |
| `0x5969efdde3cf5c0d9a88ae51e47d721096a97203` | WBTC/USDT | Uniswap V3 | 0.05% | 2 |

**Protocol distribution**: 19 UniV3, 6 PancakeSwap V3, 2 Camelot/Algebra, 1 unknown.
**Top pair by total uses**: WETH/USDT (30), WETH/USDC (32), OTM/USDT (17).

---

## Appendix B: Alternative Strategies Observed

For reference, other operators use different approaches on the same pools:

### Check-and-Return Pattern (Deployer `0xd79d421b`)

Instead of reverting, this bot silently returns on no opportunity.
Costs only ~35k gas per check ($0.0007) with 100% success rate.
Uses 80+ EOA wallets to avoid detection.

**Trade-off**: Lower per-trade profit (~$0.001) but zero wasted gas on failures.

### Multi-DEX 2-3 Hop (Deployer `0x14cd16fb`)

Routes across Algebra, PancakeSwap V3, and Uniswap V3 in the same transaction.
Uses vanity addresses (ground hex suffixes like `...666666`, `...555555`).
48% success rate — more aggressive than the callback-chaining bot.

---

## Appendix C: Quick Start Checklist

1. **Deploy 2-3 contracts** on Arbitrum:
   - Reader (STATICCALLs only — reads `globalState()`, `slot0()`, `token0()`, `token1()`)
   - Executor (entry point, profit management)
   - Router (swap execution + all callback implementations)
2. **Fund operator EOA** with ~0.01 ETH for gas (~10,000 failed txs worth)
3. **Off-chain component**: Monitor `globalState()` + `slot0()` on target pools every block
4. **Price comparison**: Compare Camelot prices vs UniV3, V2, PancakeSwap V3
5. **Route simulation**: For each candidate route, simulate swap outputs using on-chain `quote` or off-chain math
6. **Profit check**: If output - input - gasCost > threshold, submit tx
7. **Submit tx**: Call executor contract with packed route data
8. **Monitor**: Track success rate, profit per hit, gas costs
9. **Iterate**: Add new pools/routes as market conditions change

### Minimum Viable Implementation

The simplest version needs:
- 2 Solidity contracts (reader + executor/router with all callbacks)
- 1 off-chain script reading `globalState()` + `slot0()` + simulating routes
- 1 EOA wallet with ~0.01 ETH
- **No capital required** — callback chaining provides atomic funding

### Key Differences from Flash Loan Approach

| Aspect | Flash Loan | Callback Chaining (actual) |
|--------|-----------|---------------------------|
| Capital needed | Zero (borrowed) | Zero (swaps fund each other) |
| Fee | 0% (Balancer) / 0.05% (Aave) | **None** |
| Gas overhead | +30k (flash loan call) | **None** |
| External dependency | Balancer/Aave must have liquidity | **None** |
| Complexity | Medium | Medium (nested callbacks) |
| Max leverage | Limited by flash loan pool | Limited by AMM liquidity |

The observed bot has been profitable since at least block 407,629,267
(Bot 2 deployment) through the present.