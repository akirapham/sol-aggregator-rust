# Jupiter API Status & Custom Route Options

## Current State (Jan 2025)

### ✅ APIs Available:

1. **Ultra Swap API** (Recommended - Latest)
   - Endpoint: `https://lite-api.jup.ag/ultra/v1/`
   - Powers jup.ag frontend
   - Uses Juno liquidity engine (Metis + JupiterZ + external sources)
   - Best execution + highest success rates

2. **Legacy Swap API** (v6 - Still Available!)
   - Endpoint: `https://lite-api.jup.ag/swap/v1/`
   - Metis v1 routing engine only
   - Stable and battle-tested
   - Still receives tens of thousands of requests/second

**Answer: v6 IS still available as "Legacy Swap API"!**

## Can You Specify Custom Routes/Pools?

### ❌ No Direct Pool Specification

**Short Answer:** No, you **cannot** tell Jupiter "use these exact pools in this order"

**Reason:** Jupiter's APIs are designed to:
- Find optimal routes automatically
- Handle routing complexity
- Provide best execution

They don't expose a "marketInfos" or custom route parameter.

### ✅ What You CAN Control:

#### 1. DEX Filtering (Legacy API)
```rust
// Include ONLY specific DEXes
let quote = client.quote(&QuoteRequest {
    input_mint: USDC,
    output_mint: SOL,
    amount: 100_000_000,
    dexes: Some(vec!["Orca Whirlpool", "Raydium CLMM"]),
    ..Default::default()
}).await?;

// Exclude specific DEXes
let quote = client.quote(&QuoteRequest {
    input_mint: USDC,
    output_mint: SOL,
    amount: 100_000_000,
    exclude_dexes: Some(vec!["Raydium", "Orca V2"]),
    ..Default::default()
}).await?;
```

#### 2. Direct Routes Only (Legacy API)
```rust
// Force single-hop routes only (no intermediary tokens)
let quote = client.quote(&QuoteRequest {
    input_mint: USDC,
    output_mint: SOL,
    amount: 100_000_000,
    only_direct_routes: true,  // ← Only direct USDC → SOL pools
    ..Default::default()
}).await?;
```

#### 3. Router Selection (Ultra API)
```rust
// Choose which routing engine to use
let order = client.order(&OrderRequest {
    input_mint: USDC,
    output_mint: SOL,
    amount: 100_000_000,
    exclude_routers: Some(vec!["jupiterz", "dflow"]),  // Use Metis only
    ..Default::default()
}).await?;
```

### Why This Matters for Arbitrage

**Your Detection Already Found the Mispriced Pool!**

If your arbitrage detector found:
- Pool A: 1 SOL = 100 USDC (cheap)
- Pool B: 1 SOL = 101 USDC (expensive)

**Problem:** Jupiter might route through Pool C instead!

## Solutions for Arbitrage

### ✅ Option 1: Build Your Own Swap Instructions (Recommended!)

**Why:** You already know the exact pools to use from your detection

```rust
use anchor_lang::prelude::*;

pub struct ArbitrageExecutor {
    payer: Keypair,
    jito: JitoClient,
}

impl ArbitrageExecutor {
    /// Execute arbitrage with YOUR detected pools
    pub async fn execute_detected_arbitrage(
        &self,
        forward_pool: Pubkey,      // Pool you detected as cheap
        forward_dex: DexType,       // Which DEX it's on
        reverse_pool: Pubkey,       // Pool you detected as expensive
        reverse_dex: DexType,       // Which DEX it's on
        amount: u64,
    ) -> Result<Signature> {
        let mut instructions = vec![];

        // Build swap instruction for forward pool
        let forward_ix = match forward_dex {
            DexType::OrcaWhirlpool => {
                self.build_orca_whirlpool_swap(
                    forward_pool,
                    USDC_MINT,
                    SOL_MINT,
                    amount,
                )
            }
            DexType::RaydiumCLMM => {
                self.build_raydium_clmm_swap(
                    forward_pool,
                    USDC_MINT,
                    SOL_MINT,
                    amount,
                )
            }
            _ => return Err("Unsupported DEX"),
        }?;
        instructions.push(forward_ix);

        // Build swap instruction for reverse pool
        let reverse_ix = match reverse_dex {
            DexType::OrcaWhirlpool => {
                self.build_orca_whirlpool_swap(
                    reverse_pool,
                    SOL_MINT,
                    USDC_MINT,
                    expected_sol_amount,
                )
            }
            DexType::RaydiumCLMM => {
                self.build_raydium_clmm_swap(
                    reverse_pool,
                    SOL_MINT,
                    USDC_MINT,
                    expected_sol_amount,
                )
            }
            _ => return Err("Unsupported DEX"),
        }?;
        instructions.push(reverse_ix);

        // Create atomic transaction
        let recent_blockhash = self.rpc.get_latest_blockhash().await?;
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.payer.pubkey()),
            &[&self.payer],
            recent_blockhash,
        );

        // Send via Jito for MEV protection
        self.jito.send_bundle(vec![tx], tip_lamports).await
    }
}
```

**Pros:**
- ✅ Use EXACT pools you detected
- ✅ No Jupiter dependency
- ✅ Full control over route
- ✅ Atomic execution (Solana TX guarantee)
- ✅ Lower latency (no API call)

**Cons:**
- ❌ Need to implement swap instructions for each DEX
- ❌ More code to maintain
- ❌ Transaction size limits

### ✅ Option 2: Use Jupiter with DEX Constraints

**Approach:** Guide Jupiter towards your pools via DEX filtering

```rust
pub async fn execute_via_jupiter_constrained(
    &self,
    detected_opportunity: ArbitrageOpportunity,
) -> Result<Signature> {
    let jupiter = JupiterSwapApiClient::new("https://lite-api.jup.ag");

    // If you detected Orca pool is cheap, force Jupiter to use Orca
    let dex_filter = match detected_opportunity.forward_dex {
        DexType::OrcaWhirlpool => vec!["Orca Whirlpool"],
        DexType::RaydiumCLMM => vec!["Raydium CLMM"],
        _ => vec![],
    };

    // Get quote with DEX constraint
    let quote = jupiter.quote(&QuoteRequest {
        input_mint: detected_opportunity.input_token,
        output_mint: detected_opportunity.output_token,
        amount: detected_opportunity.amount,
        dexes: Some(dex_filter),           // ← Force specific DEX
        only_direct_routes: true,           // ← No multi-hop
        slippage_bps: 50,
        ..Default::default()
    }).await?;

    // Verify it's using the pool you detected
    if !self.verify_route_uses_pool(&quote.route_plan, detected_opportunity.forward_pool) {
        return Err("Jupiter didn't route through detected pool");
    }

    // Build and execute
    let swap = jupiter.swap(&SwapRequest {
        user_public_key: self.payer.pubkey(),
        quote_response: quote,
        ..Default::default()
    }).await?;

    self.jito.send_bundle(swap.transaction).await
}

fn verify_route_uses_pool(
    &self,
    route_plan: &[RoutePlan],
    expected_pool: Pubkey,
) -> bool {
    route_plan.iter().any(|step| {
        step.swap_info.amm_key == expected_pool.to_string()
    })
}
```

**Pros:**
- ✅ Easier than building instructions
- ✅ Jupiter handles transaction building
- ✅ Some control via DEX filtering

**Cons:**
- ⚠️ Not guaranteed to use exact pool
- ⚠️ Verification needed
- ⚠️ Extra API latency

### ✅ Option 3: Hybrid Approach (Best for Production)

```rust
pub enum ExecutionStrategy {
    /// Build custom instructions for known DEXes
    CustomInstructions {
        forward_pool: Pubkey,
        forward_dex: DexType,
        reverse_pool: Pubkey,
        reverse_dex: DexType,
    },

    /// Use Jupiter with constraints
    JupiterConstrained {
        dex_filter: Vec<String>,
        verify_pool: Pubkey,
    },

    /// Use Jupiter full auto (for complex routes)
    JupiterAuto,
}

impl ArbitrageExecutor {
    pub async fn execute(&self, opp: ArbitrageOpportunity) -> Result<Signature> {
        let strategy = self.choose_strategy(&opp);

        match strategy {
            // Simple 2-pool arb on known DEXes = custom instructions
            ExecutionStrategy::CustomInstructions { .. }
                if self.can_build_instructions(&opp) => {
                self.execute_custom_instructions(opp).await
            }

            // Can constrain Jupiter enough = use it
            ExecutionStrategy::JupiterConstrained { .. } => {
                self.execute_jupiter_constrained(opp).await
            }

            // Complex route = let Jupiter figure it out
            ExecutionStrategy::JupiterAuto => {
                self.execute_jupiter_auto(opp).await
            }
        }
    }

    fn choose_strategy(&self, opp: &ArbitrageOpportunity) -> ExecutionStrategy {
        let forward_steps = opp.forward_route.paths.iter()
            .flat_map(|p| &p.steps)
            .count();
        let reverse_steps = opp.reverse_route.paths.iter()
            .flat_map(|p| &p.steps)
            .count();

        // Simple 2-step on supported DEXes = build ourselves
        if forward_steps == 1 && reverse_steps == 1 {
            let forward_dex = opp.forward_route.paths[0].steps[0].dex;
            let reverse_dex = opp.reverse_route.paths[0].steps[0].dex;

            if self.supports_custom_ix(forward_dex)
                && self.supports_custom_ix(reverse_dex) {
                return ExecutionStrategy::CustomInstructions {
                    forward_pool: opp.forward_route.paths[0].steps[0].pool_address,
                    forward_dex,
                    reverse_pool: opp.reverse_route.paths[0].steps[0].pool_address,
                    reverse_dex,
                };
            }
        }

        // Default to Jupiter
        ExecutionStrategy::JupiterAuto
    }

    fn supports_custom_ix(&self, dex: DexType) -> bool {
        matches!(dex,
            DexType::OrcaWhirlpool |
            DexType::RaydiumCLMM |
            DexType::RaydiumCPMM |
            DexType::Phoenix
        )
    }
}
```

## Recommended Implementation Plan

### Phase 1: Custom Instructions for Major DEXes (Week 1-2)

**Focus:** Build swap instructions for top 3 DEXes that cover 80% of your opportunities

```rust
// Priority DEXes to implement:
1. Orca Whirlpool (CLMM) - largest liquidity
2. Raydium CLMM - second largest
3. Raydium CPMM - constant product pools
```

**Why Start Here:**
- ✅ Full control over execution
- ✅ Use exact pools you detected
- ✅ Atomic execution guaranteed
- ✅ Lower latency (no API)
- ✅ Covers most opportunities

### Phase 2: Add Jupiter Fallback (Week 3)

**For edge cases:**
- Complex multi-hop routes
- Unsupported DEXes
- When custom instruction fails

```rust
async fn execute_arbitrage(&self, opp: ArbitrageOpportunity) -> Result<Signature> {
    // Try custom instructions first
    match self.execute_custom(opp.clone()).await {
        Ok(sig) => Ok(sig),
        Err(e) => {
            log::warn!("Custom execution failed: {}, falling back to Jupiter", e);
            self.execute_via_jupiter(opp).await
        }
    }
}
```

### Phase 3: Optimize Based on Data (Week 4+)

**Track metrics:**
```rust
struct ExecutionMetrics {
    custom_attempts: u64,
    custom_successes: u64,
    custom_avg_profit: f64,

    jupiter_attempts: u64,
    jupiter_successes: u64,
    jupiter_avg_profit: f64,

    // Which method is better?
    custom_success_rate: f64,
    jupiter_success_rate: f64,
}
```

## Implementation Examples

### Building Orca Whirlpool Swap Instruction

```rust
use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

pub fn build_orca_whirlpool_swap(
    whirlpool: Pubkey,
    token_a: Pubkey,
    token_b: Pubkey,
    amount: u64,
    min_output: u64,
    user: Pubkey,
) -> Result<Instruction> {
    let whirlpool_program = Pubkey::from_str("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc")?;

    // Get associated token accounts
    let user_token_a = get_associated_token_address(&user, &token_a);
    let user_token_b = get_associated_token_address(&user, &token_b);

    // Get pool token vaults
    let (token_vault_a, _) = Pubkey::find_program_address(
        &[b"token_vault_a", whirlpool.as_ref()],
        &whirlpool_program,
    );
    let (token_vault_b, _) = Pubkey::find_program_address(
        &[b"token_vault_b", whirlpool.as_ref()],
        &whirlpool_program,
    );

    // Build instruction data
    let ix_data = WhirlpoolSwapInstruction {
        amount,
        other_amount_threshold: min_output,
        sqrt_price_limit: u128::MAX,
        amount_specified_is_input: true,
        a_to_b: token_a < token_b, // Determine swap direction
    };

    // Build instruction
    let ix = Instruction {
        program_id: whirlpool_program,
        accounts: vec![
            AccountMeta::new_readonly(Token::id(), false),
            AccountMeta::new(user, true),
            AccountMeta::new(whirlpool, false),
            AccountMeta::new(user_token_a, false),
            AccountMeta::new(user_token_b, false),
            AccountMeta::new(token_vault_a, false),
            AccountMeta::new(token_vault_b, false),
            // ... oracle, tick arrays, etc.
        ],
        data: ix_data.try_to_vec()?,
    };

    Ok(ix)
}
```

### Building Raydium CLMM Swap Instruction

```rust
pub fn build_raydium_clmm_swap(
    pool: Pubkey,
    token_a: Pubkey,
    token_b: Pubkey,
    amount: u64,
    min_output: u64,
    user: Pubkey,
) -> Result<Instruction> {
    let raydium_clmm_program = Pubkey::from_str("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK")?;

    // Similar structure to Orca
    // Implementation details depend on Raydium's instruction format

    todo!("Implement Raydium CLMM swap instruction")
}
```

## Final Recommendation

### For Your Solana Aggregator Arbitrage:

**Use Custom Instructions + Jupiter Hybrid**

```
┌─────────────────────────────────────┐
│   Your Fast Arbitrage Detection    │
│   (Event-driven, broadcast-based)   │
└─────────────────┬───────────────────┘
                  │
                  ▼
         ┌────────────────────┐
         │  Classify Opportunity │
         └────────┬───────────────┘
                  │
        ┌─────────┴──────────┐
        │                    │
        ▼                    ▼
┌───────────────┐    ┌──────────────┐
│ Simple 2-Pool │    │ Complex/     │
│ Known DEX     │    │ Unsupported  │
└───────┬───────┘    └──────┬───────┘
        │                   │
        ▼                   ▼
┌──────────────┐    ┌──────────────┐
│ Custom IX    │    │ Jupiter API  │
│ (Your Build) │    │ (Fallback)   │
└──────┬───────┘    └──────┬───────┘
        │                   │
        └─────────┬─────────┘
                  ▼
         ┌────────────────┐
         │  Jito Bundle   │
         │ (MEV Protection)│
         └────────────────┘
```

**Timeline:**
- Week 1-2: Implement Orca + Raydium CLMM instructions
- Week 3: Add Jupiter fallback
- Week 4+: Monitor and optimize

**Your Edge:**
- ⚡ Fastest detection (event-driven)
- 🎯 Exact pool execution (custom IX)
- 🛡️ MEV protection (Jito)
- 📊 Smart fallback (Jupiter when needed)
