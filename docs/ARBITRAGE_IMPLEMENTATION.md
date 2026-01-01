# Arbitrage Implementation Summary

## Overview

This Solana DEX aggregator now includes a sophisticated arbitrage detection system that monitors 25 popular tokens for profitable trading opportunities.

## Key Features

### 1. Event-Driven Architecture
- **Broadcast Channel**: Pool manager broadcasts price updates in real-time
- **No Polling**: Zero waste from periodic checks
- **Instant Reaction**: Detects opportunities milliseconds after pool updates
- **Concurrent Processing**: Handles multiple opportunities simultaneously (max 10)

### 2. Smart Detection Strategy

#### Price Pre-filtering
```rust
// Quick check before expensive routing
let price_round_trip = forward_price * reverse_price;
if price_round_trip < 1.0 + min_profit_threshold {
    return; // Skip expensive calculation
}
```

#### Direct Path Forward Route
- Forces direct paths only (no multi-hop)
- Allows splits (up to 2 paths) to detect pool mispricing
- Example: If Pool A has SOL cheaper than Pool B, route detects this

#### Best Route Return Trip
- Reverse route allows multi-hop for optimal price
- Maximizes profit on the return to base token
- Uses full aggregator routing capabilities

### 3. USDC-Based Strategy
```
USDC -> Token -> USDC (round-trip)
```

**Why USDC?**
- Most liquid token on Solana
- Stable value (no price risk during arb)
- Easy profit calculation
- Universal base pair

### 4. Token Selection (25 Tokens)

#### Categories

**Blue Chips** (10 tokens):
- SOL, USDT, ETH, WBTC - Core assets
- mSOL, stSOL, jitoSOL, bSOL - Liquid staking (LST arbitrage)
- RAY, JTO - Major DeFi protocols

**High Volume** (5 tokens):
- JUP - Jupiter aggregator token (🔥 must have!)
- ORCA - Orca DEX
- PYTH - Oracle network
- MEW, HNT - Popular alts

**Meme Coins** (3 tokens):
- WIF (dogwifhat) - Massive volume
- POPCAT, PONKE, MOTHER - Volatile, high opportunity

**Stablecoins** (2 tokens):
- DAI, PYUSD - Tight spread arbitrage

**Infrastructure** (2 tokens):
- RENDER - AI/compute token
- W - Wormhole bridge token

### 5. Configuration

```toml
[settings]
min_profit_bps = 50              # 0.5% minimum profit
base_token = "USDC_ADDRESS"      # Always start with USDC
base_amount = 100000000          # 100 USDC per trade
slippage_bps = 50                # 0.5% slippage tolerance
max_concurrent_checks = 10       # Process 10 opportunities in parallel
```

## Competitive Advantages

### vs Jupiter
- **Jupiter**: Best routing, but slower (full graph search)
- **Our Edge**: Event-driven = faster detection

### vs Periodic Scanners
- **Scanners**: Check every X seconds, miss opportunities
- **Our Edge**: React to every pool update instantly

### vs Simple Bots
- **Simple Bots**: Check all pairs constantly
- **Our Edge**: Pre-filter with quick math, only route when promising

## API Endpoints

### Get Monitored Tokens
```bash
GET /arbitrage/tokens
```

Response:
```json
{
  "base_token": "EPjFWdd5...",
  "monitored_tokens": [
    {
      "address": "So11111...",
      "symbol": "SOL",
      "enabled": true
    },
    ...
  ]
}
```

### Add Token
```bash
POST /arbitrage/tokens
Content-Type: application/json

{
  "address": "TOKEN_ADDRESS",
  "symbol": "TOKEN_SYMBOL"
}
```

### Remove Token
```bash
DELETE /arbitrage/tokens
Content-Type: application/json

{
  "address": "TOKEN_ADDRESS"
}
```

### Check Manual Arbitrage
```bash
POST /arbitrage/check
Content-Type: application/json

{
  "input_token": "USDC_ADDRESS",
  "output_token": "SOL_ADDRESS",
  "amount": 100000000,
  "slippage_bps": 50
}
```

## Data Flow

```
Pool Update (Geyser)
    ↓
Pool Manager applies update
    ↓
Calculate token prices (forward & reverse)
    ↓
Check if pool has monitored tokens
    ↓
Broadcast to arbitrage monitor
    ↓
Quick price check (forward * reverse > 1.0 + threshold)
    ↓
Full routing calculation (if promising)
    ↓
Send opportunity to channel
    ↓
(Future: Execute via Jito bundle)
```

## Persistence

### RocksDB Storage
- **Monitored Token Addresses**: Saved on add/remove
- **Load on Startup**: Merges TOML config + DB state
- **Periodic Sync**: Updates saved automatically

### Merge Strategy
```
1. Load tokens from TOML (default config)
2. Load tokens from RocksDB (runtime additions)
3. Merge: DB tokens override TOML for conflicts
4. Result: Union of both sources
```

## Performance Metrics

### Expected Latency
- **Pool Update to Broadcast**: <1ms
- **Pre-filter Check**: <0.1ms
- **Full Route Calculation**: 5-50ms (depending on complexity)
- **Total Detection Time**: 5-50ms from pool update

### Throughput
- **Max Concurrent Checks**: 10
- **Broadcast Channel Capacity**: 1000 events
- **Expected Opportunities**: 10-100 per hour (varies by market)

## Future Enhancements

### Priority 1: Execution
- [ ] Build transactions from routes
- [ ] Jito bundle integration (MEV protection)
- [ ] Simulation before sending
- [ ] Retry logic on failure

### Priority 2: Safety
- [ ] Oracle price validation
- [ ] Pool liquidity checks
- [ ] Circuit breakers (max deviation)
- [ ] Position limits per token

### Priority 3: Advanced Strategies
- [ ] Triangular arbitrage (3+ token cycles)
- [ ] Cross-DEX arbitrage
- [ ] LST arbitrage specialization
- [ ] Stablecoin peg arbitrage

### Priority 4: Monitoring
- [ ] Prometheus metrics
- [ ] Opportunity tracking
- [ ] Profitability analytics
- [ ] Alert system for large opportunities

## Risk Considerations

### Current Risks
1. **No Execution Yet**: Only detection, manual trading required
2. **No MEV Protection**: Vulnerable to frontrunning without Jito
3. **Gas Costs**: 50bps profit might be eaten by fees on small amounts
4. **Slippage**: 0.5% tolerance might be tight in volatile markets

### Mitigations
- Start with larger amounts (100+ USDC)
- Only trade high-liquidity pairs
- Monitor actual fill prices
- Add Jito bundles before production

## Testing

### Manual Testing
```bash
# Check if SOL arbitrage is profitable
curl -X POST http://localhost:3000/arbitrage/check \
  -H "Content-Type: application/json" \
  -d '{
    "input_token": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "output_token": "So11111111111111111111111111111111111111112",
    "amount": 100000000,
    "slippage_bps": 50
  }'
```

### Expected Response
```json
{
  "profitable": true,
  "profit_amount": 250000,
  "profit_percent": 0.25,
  "forward_route": [...],
  "reverse_route": [...],
  "forward_output": 520000,
  "reverse_output": 100250000,
  "time_taken_ms": 45,
  "context_slot": 123456789
}
```

## Monitoring Opportunities

The arbitrage monitor sends detected opportunities to a channel. In `main.rs`:

```rust
let (monitor, mut opportunity_rx) = ArbitrageMonitor::new(aggregator.clone(), arb_config);

// Listen for opportunities
tokio::spawn(async move {
    while let Some(opp) = opportunity_rx.recv().await {
        log::info!(
            "💰 ARBITRAGE: {} | {:.4}% profit | {} base units",
            opp.pair_name,
            opp.profit_percent,
            opp.profit_amount
        );

        // TODO: Execute the trade here
        // build_and_send_transaction(&opp).await;
    }
});
```

## Summary

This arbitrage implementation provides a **solid foundation** for automated trading on Solana. The event-driven architecture and smart pre-filtering give us a competitive edge in detection speed.

**Current State**: Detection only, production-ready once execution is added.

**Next Steps**:
1. Add Jito bundle support
2. Implement transaction building
3. Add safety checks (oracle, liquidity)
4. Deploy and test with small amounts
5. Scale up after proving profitability
