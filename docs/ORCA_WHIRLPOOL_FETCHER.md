# Orca Whirlpool Tick Array Fetcher

## Overview

The `orca_tick_array_fetcher.rs` module provides functionality to fetch and parse tick array data from the Orca Whirlpool concentrated liquidity AMM on Solana.

## Key Components

### Data Structures

#### `OrcaTickState`
Represents a single tick in an Orca Whirlpool:
- `initialized: bool` - Whether the tick is initialized
- `liquidity_net: i128` - Net liquidity at this tick
- `liquidity_gross: u128` - Total liquidity at this tick
- `fee_growth_outside_a: u128` - Fee growth for token A outside this tick
- `fee_growth_outside_b: u128` - Fee growth for token B outside this tick
- `reward_growths_outside: [u128; 3]` - Reward growth for up to 3 incentive programs

#### `FixedOrcaTickArrayState`
A fixed-size tick array containing exactly 88 ticks:
- `discriminator: [u8; 8]` - Account discriminator for identification
- `start_tick_index: i32` - Starting tick index for this array
- `ticks: [OrcaTickState; 88]` - Array of 88 ticks
- `whirlpool: Pubkey` - Reference to the parent whirlpool

#### `DynamicOrcaTickArrayState`
A dynamic tick array with variable-length data support:
- `discriminator: [u8; 8]` - Account discriminator
- `start_tick_index: i32` - Starting tick index
- `whirlpool: Pubkey` - Parent whirlpool reference
- `tick_bitmap: u128` - Bitmap for tracking initialized ticks (optimization)
- `ticks: [OrcaTickState; 88]` - Array of ticks

#### `OrcaTickArrayState`
Enum that can represent either fixed or dynamic tick arrays:
```rust
pub enum OrcaTickArrayState {
    Fixed(FixedOrcaTickArrayState),
    Dynamic(DynamicOrcaTickArrayState),
}
```

### Main Fetcher: `OrcaTickArrayFetcher`

#### Initialization
```rust
let fetcher = OrcaTickArrayFetcher::new(rpc_client_arc)?;
```

#### Methods

##### `fetch_tick_array(address: Pubkey) -> Result<OrcaTickArrayState>`
Fetch a single tick array account and deserialize it based on its discriminator.

**Usage:**
```rust
let tick_array = fetcher.fetch_tick_array(tick_array_address).await?;
let initialized_ticks = OrcaTickArrayFetcher::get_initialized_ticks(&tick_array);
```

##### `fetch_multiple_tick_arrays(addresses: Vec<Pubkey>) -> Result<Vec<OrcaTickArrayState>>`
Fetch multiple tick arrays efficiently in a single RPC call. Handles deserialization and logs warnings for invalid or missing accounts.

**Usage:**
```rust
let tick_arrays = fetcher.fetch_multiple_tick_arrays(vec![addr1, addr2, addr3]).await?;
```

##### `derive_tick_array_pda(whirlpool: &Pubkey, start_tick_index: i32) -> Result<(Pubkey, u8)>`
Derive the PDA (Program Derived Address) for a tick array given a whirlpool and start tick index.

**Formula:** `hash(["tick_array", whirlpool_address, start_tick_index])`

**Usage:**
```rust
let (pda, bump) = fetcher.derive_tick_array_pda(&whirlpool_address, 0)?;
```

##### `get_initialized_ticks(tick_array: &OrcaTickArrayState) -> Vec<(i32, OrcaTickState)>`
Extract all initialized ticks from a tick array with their absolute tick indices.

**Features:**
- Handles both fixed and dynamic tick arrays
- Uses bitmap for dynamic arrays (more efficient)
- Returns absolute tick indices (not relative to array start)

**Usage:**
```rust
let init_ticks = OrcaTickArrayFetcher::get_initialized_ticks(&tick_array);
for (tick_index, tick) in init_ticks {
    println!("Tick {}: {} liquidity", tick_index, tick.liquidity_gross);
}
```

##### `calculate_price_from_sqrt_price(sqrt_price_x64: u128) -> f64`
Convert a Q64 fixed-point sqrt price to a decimal price.

**Formula:**
```
sqrt_price = sqrt_price_x64 / 2^64
price = sqrt_price^2
```

**Usage:**
```rust
let price = OrcaTickArrayFetcher::calculate_price_from_sqrt_price(sqrt_price_x64);
```

## Orca Whirlpool Constants

- **Program ID:** `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc`
- **Tick Array Size:** 88 ticks per array
- **Fixed Discriminator:** `[69, 97, 189, 190, 110, 7, 66, 187]`
- **Dynamic Discriminator:** `[17, 216, 246, 142, 225, 199, 218, 56]`
- **Seed:** `"tick_array"`

## Differences from Raydium CLMM

| Aspect | Orca | Raydium |
|--------|------|---------|
| Tick Array Size | 88 | 60 |
| Dynamic Support | Yes (with bitmap) | No |
| Number of Rewards | 3 (max) | Variable |
| Fee Growth | Per token (A/B) | Per token (0/1) |

## Integration Example

```rust
use aggregator_sol::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;

// Create fetcher
let fetcher = OrcaTickArrayFetcher::new(Arc::new(rpc_client))?;

// Fetch a tick array
let tick_array = fetcher.fetch_tick_array(tick_array_address).await?;

// Get initialized ticks
let initialized = OrcaTickArrayFetcher::get_initialized_ticks(&tick_array);

// For dynamic arrays, get the bitmap
if let Some(bitmap) = tick_array.tick_bitmap() {
    println!("Bitmap: {:x}", bitmap);
}

// Derive PDA for a whirlpool
let (pda, bump) = fetcher.derive_tick_array_pda(&whirlpool_address, 0)?;
```

## Testing

The module includes comprehensive tests:
- `test_orca_tick_state_default()` - Verify default tick state
- `test_orca_tick_array_state_methods()` - Verify tick array methods
- `test_dynamic_tick_array_bitmap()` - Verify bitmap handling
- `test_price_calculation()` - Verify price calculation accuracy

Run tests with:
```bash
cargo test -p aggregator-sol --lib fetchers::orca_tick_array_fetcher
```

## Error Handling

The fetcher provides detailed error messages for:
- Account not owned by Orca program
- Account data size validation
- Invalid discriminators
- Deserialization failures (logged, not fatal during batch operations)

## Performance Notes

- **Batch Fetching:** `fetch_multiple_tick_arrays()` is optimized for RPC efficiency
- **Bitmap Lookup:** Dynamic arrays use bitmap for O(1) tick initialization checks
- **PDA Derivation:** Uses standard Solana PDA derivation algorithm
