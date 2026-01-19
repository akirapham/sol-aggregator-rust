# CEX Token Safety Verification Implementation

## Overview
This implementation adds critical safety checks to ensure tokens are BOTH tradeable AND depositable on the correct network (ERC20 for Ethereum, Solana chain for Solana) before attempting arbitrage.

## Problem Statement
The bug found with SXP on MEXC: The system showed the token as tradeable and available, but MEXC didn't actually support ERC20 deposits for that token. This could lead to failed arbitrage attempts.

## Solution Architecture

### 1. Token Status Structure (`TokenStatus`)
Located in: `crates/cex-price-provider/src/lib.rs`

```rust
pub struct TokenStatus {
    pub symbol: String,                // Trading pair symbol (e.g., "BTCUSDT")
    pub base_asset: String,             // Base currency (e.g., "BTC")
    pub contract_address: Option<String>, // Token contract address
    pub is_trading: bool,               // Whether trading is enabled
    pub is_deposit_enabled: bool,       // Whether deposits are enabled on the correct network
    pub network_verified: bool,         // Whether network matches filter (ERC20 for Ethereum, Solana for Solana)
    pub last_updated: u64,              // Unix timestamp of last update
}
```

### 2. New PriceProvider Trait Methods

```rust
async fn is_token_safe_for_arbitrage(&self, symbol: &str, contract_address: Option<&str>) -> bool;
async fn get_token_status(&self, symbol: &str, contract_address: Option<&str>) -> Option<TokenStatus>;
async fn refresh_token_status(&self) -> Result<()>;
```

### 3. Implementation Status

#### ✅ MEXC (FULLY IMPLEMENTED)
- ✅ Token status cache with HashMap
- ✅ Network verification (ERC20 vs BSC vs Polygon, etc.)
- ✅ Deposit enable status check
- ✅ Initial token status refresh on startup
- ✅ Background task to refresh every 12 hours
- ✅ Proper filtering during `start()` to only subscribe to safe tokens

**Key Network Checks for MEXC:**
- Ethereum: Only accepts `ERC20`, `ETH`, or `ETHEREUM` network names
- Solana: Only accepts `SOL` or `SOLANA` network names
- Verifies contract address matches the trading pair
- Checks `deposit_enable` flag is true

#### 🔄 Bybit (PARTIALLY IMPLEMENTED)
- ✅ Token status cache added
- ✅ `refresh_token_status()` implemented with network verification
- ✅ `is_token_safe_for_arbitrage()` and `get_token_status()` implemented
- ❌ NOT YET: Integrated into `start()` method
- ❌ NOT YET: Background refresh task (12 hours)

**Key Network Checks for Bybit:**
- Ethereum: Only `ETH` or `ETHEREUM` chain, excluding Arbitrum/Polygon variants
- Solana: Only `SOL` or `SOLANA` chain
- Checks `chain_deposit` == "1"

#### ⚠️ Gate.io (STUB ONLY)
- ⚠️ Only stub implementations (returns true/empty)
- TODO: Add token_status_cache
- TODO: Implement network verification
- TODO: Add periodic refresh

**API Support:**
- Has `/wallet/currency_chains/{currency}` endpoint
- Has `is_deposit_disabled` field (0 = enabled, 1 = disabled)
- Network identification: `chain` field (e.g., "ETH", "SOL")

#### ⚠️ KuCoin (STUB ONLY)
- ⚠️ Only stub implementations (returns true/empty)
- TODO: Add token_status_cache
- TODO: Implement network verification
- TODO: Add periodic refresh

**API Support:**
- Has `/api/v1/currencies/{currency}` endpoint
- Has `chains` array with `isDepositEnabled` boolean
- Network identification: `chainName` field

#### ⚠️ Bitget (STUB ONLY)
- ⚠️ Only stub implementations (returns true/empty)
- TODO: Add token_status_cache
- TODO: Implement network verification
- TODO: Add periodic refresh

**API Support:**
- Has `/api/v2/spot/public/coins` endpoint
- Has `chains` array with `rechargeable` field ("true"/"false")
- Network identification: `chain` field

## Network Verification Logic

### For Ethereum (FilterAddressType::Ethereum)
**CRITICAL**: Only accept tokens depositable on Ethereum mainnet via ERC20

**Acceptable Network Names:**
- `ETH`
- `ETHEREUM`
- `ERC20`

**REJECT:**
- `BSC` / `BEP20` (Binance Smart Chain)
- `POLYGON` / `MATIC`
- `ARBITRUM` / `ARB`
- `OPTIMISM` / `OP`
- `AVALANCHE` / `AVAX`
- Any other L2 or sidechain

### For Solana (FilterAddressType::Solana)
**Acceptable Network Names:**
- `SOL`
- `SOLANA`

## Testing

### Run the Safety Test Example:
```bash
# With MEXC credentials (recommended):
export MEXC_API_KEY="your_key"
export MEXC_API_SECRET="your_secret"

# With Bybit credentials (optional):
export BYBIT_API_KEY="your_key"
export BYBIT_API_SECRET="your_secret"

# Run the test:
RUST_LOG=info cargo run --example test_token_safety -p cex-price-provider
```

### What the Test Checks:
1. Can refresh token status from API
2. Verifies USDT (known good token)
3. Verifies WETH (known good token)
4. Tests potentially problematic tokens
5. Shows detailed status for each:
   - Trading enabled?
   - Deposits enabled?
   - Network verified?
   - Overall: Safe for arbitrage?

## TODO: Complete Implementation

### Priority 1: Complete Bybit
1. Update `start()` method to call `refresh_token_status()` initially
2. Add background task for 12-hour refresh
3. Filter pairs based on `is_token_safe_for_arbitrage()` before subscribing

### Priority 2: Implement Gate.io
1. Add `token_status_cache` field to `GateService`
2. Implement `refresh_token_status()` with API calls
3. Add network verification logic
4. Update `start()` method
5. Add background refresh task

### Priority 3: Implement KuCoin
1. Add `token_status_cache` field to `KucoinService`
2. Implement `refresh_token_status()` with API calls
3. Add network verification logic
4. Update `start()` method
5. Add background refresh task

### Priority 4: Implement Bitget
1. Add `token_status_cache` field to `BitgetService`
2. Implement `refresh_token_status()` with API calls
3. Add network verification logic
4. Update `start()` method
5. Add background refresh task

## Integration with Arbitrage System

When checking for arbitrage opportunities, ALWAYS call:
```rust
let is_safe = cex_service.is_token_safe_for_arbitrage(symbol, Some(contract_address)).await;
if !is_safe {
    log::warn!("Skipping {} - not safe for arbitrage on {}", symbol, cex_name);
    continue;
}
```

## Benefits

1. **Prevents Failed Arbitrage**: Won't attempt arbitrage if deposits are disabled
2. **Network Safety**: Ensures we're only using the correct network (no BSC when expecting ERC20)
3. **Automatic Updates**: Status refreshes every 12 hours to catch changes
4. **Performance**: Cached status means no API calls during arbitrage checks
5. **Transparency**: Can query status of any token at any time

## Example Output

```
📊 Testing SXP
  Symbol: SXPUSDT
  Contract: 0x8ce9137d39326ad0cd6491fb5cc0cba0e089b6a9
  ✓ Trading enabled: ✅
  ✓ Deposits enabled: ❌
  ✓ Network verified: ❌
  ⚠️  NOT SAFE FOR ARBITRAGE
     Reason: Deposits are disabled
     Reason: Wrong network or network not verified
```

This clearly shows why a token is not safe for arbitrage.
