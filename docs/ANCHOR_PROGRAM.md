# Anchor Program Documentation

This document describes the Solana DEX Aggregator Anchor program that provides on-chain fee collection and configuration management.

## Overview

The Anchor program serves as the on-chain component of the DEX aggregator, handling:
- **Fee Collection**: Automatic fee collection from swaps
- **Configuration Management**: On-chain storage of aggregator settings
- **Admin Controls**: Administrative functions for program management
- **User Tracking**: Fee tracking per user

## Program Architecture

### Core Accounts

#### `AggregatorState`
The main program state account storing:
- Admin public key
- Fee rate (in basis points)
- Total fees collected
- Pause status
- Configuration settings

#### `FeeCollection`
Tracks collected fees:
- Total fees collected
- Last collection timestamp
- Bump seed

#### `UserFeeTracking`
Per-user fee tracking:
- User public key
- Total fees paid
- Last payment timestamp
- Bump seed

### Configuration Types

#### `AggregatorConfig`
On-chain configuration including:
- Maximum slippage tolerance
- Maximum routes
- Enabled DEXs
- Smart routing settings
- Gas configuration
- MEV protection settings
- Split trading configuration

## Instructions

### 1. Initialize

**Purpose**: Initialize the aggregator program

**Accounts**:
- `aggregator_state`: The main state account (PDA)
- `payer`: Account paying for initialization
- `system_program`: Solana system program

**Parameters**:
- `fee_rate`: Fee rate in basis points (0-10000)
- `admin`: Admin public key

**Example**:
```typescript
await program.methods
  .initialize(100, admin.publicKey) // 1% fee rate
  .accounts({
    aggregatorState,
    payer: admin.publicKey,
    systemProgram: SystemProgram.programId,
  })
  .signers([admin])
  .rpc();
```

### 2. Update Configuration

**Purpose**: Update aggregator configuration settings

**Accounts**:
- `aggregator_state`: The main state account
- `admin`: Admin signer

**Parameters**:
- `new_config`: New configuration object

**Example**:
```typescript
const config = {
  maxSlippage: 300, // 3% in basis points
  maxRoutes: 10,
  enabledDexs: [
    { pumpFun: {} },
    { raydium: {} },
    { orca: {} },
  ],
  // ... other config options
};

await program.methods
  .updateConfig(config)
  .accounts({
    aggregatorState,
    admin: admin.publicKey,
  })
  .signers([admin])
  .rpc();
```

### 3. Update Fee Rate

**Purpose**: Update the fee rate

**Accounts**:
- `aggregator_state`: The main state account
- `admin`: Admin signer

**Parameters**:
- `new_fee_rate`: New fee rate in basis points

**Example**:
```typescript
await program.methods
  .updateFeeRate(new anchor.BN(200)) // 2% fee rate
  .accounts({
    aggregatorState,
    admin: admin.publicKey,
  })
  .signers([admin])
  .rpc();
```

### 4. Collect Fee

**Purpose**: Collect fee from a user

**Accounts**:
- `aggregator_state`: The main state account
- `fee_collection`: Fee collection account
- `user_token_account`: User's token account
- `fee_token_account`: Fee collection token account
- `user`: User signer
- `token_program`: SPL Token program

**Parameters**:
- `amount`: Amount to calculate fee from

**Example**:
```typescript
await program.methods
  .collectFee(new anchor.BN(1000000)) // 1 token
  .accounts({
    aggregatorState,
    feeCollection,
    userTokenAccount,
    feeTokenAccount,
    user: user.publicKey,
    tokenProgram: TOKEN_PROGRAM_ID,
  })
  .signers([user])
  .rpc();
```

### 5. Execute Swap

**Purpose**: Execute a swap through the aggregator

**Accounts**:
- `aggregator_state`: The main state account
- `user_fee_tracking`: User's fee tracking account
- `user_token_account`: User's input token account
- `output_token_account`: User's output token account
- `user`: User signer
- `token_program`: SPL Token program

**Parameters**:
- `swap_params`: Swap parameters
- `route`: Swap route information

**Example**:
```typescript
const swapParams = {
  inputToken: mint,
  outputToken: mint,
  inputAmount: new anchor.BN(1000000),
  slippageTolerance: new anchor.BN(100),
  userWallet: user.publicKey,
  priority: { medium: {} },
};

const route = {
  dex: { raydium: {} },
  inputToken: mint,
  outputToken: mint,
  inputAmount: new anchor.BN(1000000),
  outputAmount: new anchor.BN(990000),
  priceImpact: new anchor.BN(100),
  fee: new anchor.BN(20000),
  routePath: [mint],
  gasCost: new anchor.BN(10000),
  executionTimeMs: new anchor.BN(1000),
  mevRisk: { low: {} },
  liquidityDepth: new anchor.BN(100000000),
};

await program.methods
  .executeSwap(swapParams, route)
  .accounts({
    aggregatorState,
    userFeeTracking,
    userTokenAccount,
    outputTokenAccount,
    user: user.publicKey,
    tokenProgram: TOKEN_PROGRAM_ID,
  })
  .signers([user])
  .rpc();
```

### 6. Withdraw Fees

**Purpose**: Withdraw collected fees (admin only)

**Accounts**:
- `aggregator_state`: The main state account
- `fee_collection`: Fee collection account
- `fee_token_account`: Fee collection token account
- `admin_token_account`: Admin's token account
- `admin`: Admin signer
- `token_program`: SPL Token program

**Parameters**:
- `amount`: Amount to withdraw

**Example**:
```typescript
await program.methods
  .withdrawFees(new anchor.BN(1000000))
  .accounts({
    aggregatorState,
    feeCollection,
    feeTokenAccount,
    adminTokenAccount,
    admin: admin.publicKey,
    tokenProgram: TOKEN_PROGRAM_ID,
  })
  .signers([admin])
  .rpc();
```

### 7. Set Admin

**Purpose**: Change the admin (admin only)

**Accounts**:
- `aggregator_state`: The main state account
- `admin`: Current admin signer

**Parameters**:
- `new_admin`: New admin public key

**Example**:
```typescript
await program.methods
  .setAdmin(newAdmin.publicKey)
  .accounts({
    aggregatorState,
    admin: currentAdmin.publicKey,
  })
  .signers([currentAdmin])
  .rpc();
```

### 8. Pause/Unpause

**Purpose**: Pause or unpause the program (admin only)

**Example**:
```typescript
// Pause
await program.methods
  .pause()
  .accounts({
    aggregatorState,
    admin: admin.publicKey,
  })
  .signers([admin])
  .rpc();

// Unpause
await program.methods
  .unpause()
  .accounts({
    aggregatorState,
    admin: admin.publicKey,
  })
  .signers([admin])
  .rpc();
```

## SDK Usage

### Basic Setup

```typescript
import { AggregatorClient, AggregatorUtils } from "@sol-agg-rust/sdk";
import * as anchor from "@coral-xyz/anchor";

// Initialize client
const program = anchor.workspace.AggregatorProgram;
const client = new AggregatorClient(program);

// Initialize program
const admin = Keypair.generate();
await client.initialize(100, admin); // 1% fee rate
```

### Configuration Management

```typescript
// Update configuration
const config = AggregatorClient.createDefaultConfig();
config.maxSlippage = 300; // 3%
config.maxRoutes = 10;

await client.updateConfig(config, admin);

// Update fee rate
await client.updateFeeRate(200, admin); // 2%
```

### Fee Collection

```typescript
// Collect fee
const amount = 1000000; // 1 token
const fee = client.calculateFee(amount, 200); // 2% fee = 20000

await client.collectFee(amount, userTokenAccount, feeTokenAccount, user);
```

### Swap Execution

```typescript
// Execute swap
const swapParams = {
  inputToken: mint,
  outputToken: mint,
  inputAmount: 1000000,
  slippageTolerance: 100,
  userWallet: user.publicKey,
  priority: "medium",
};

const route = {
  dex: "raydium",
  inputToken: mint,
  outputToken: mint,
  inputAmount: 1000000,
  outputAmount: 990000,
  priceImpact: 100,
  fee: 20000,
  routePath: [mint],
  gasCost: 10000,
  executionTimeMs: 1000,
  mevRisk: "low",
  liquidityDepth: 100000000,
};

await client.executeSwap(swapParams, route, userTokenAccount, outputTokenAccount, user);
```

## Error Handling

The program includes comprehensive error handling:

```typescript
try {
  await client.executeSwap(swapParams, route, userTokenAccount, outputTokenAccount, user);
} catch (error) {
  if (error.message.includes("Unauthorized")) {
    console.error("Only admin can perform this action");
  } else if (error.message.includes("ProgramPaused")) {
    console.error("Program is currently paused");
  } else if (error.message.includes("InsufficientFunds")) {
    console.error("Insufficient funds for operation");
  } else if (error.message.includes("InvalidFeeRate")) {
    console.error("Invalid fee rate (must be 0-10000 basis points)");
  }
  // ... handle other errors
}
```

## Security Considerations

1. **Admin Controls**: Only the admin can update configuration and withdraw fees
2. **Fee Validation**: Fee rates are validated to be within reasonable bounds
3. **Pause Mechanism**: Program can be paused in emergency situations
4. **Input Validation**: All inputs are validated before processing
5. **Math Safety**: All calculations use checked arithmetic to prevent overflow

## Testing

Run the test suite:

```bash
# Run all tests
anchor test

# Run specific test
anchor test -- --grep "Initialize"

# Run with verbose output
anchor test -- --verbose
```

## Deployment

### Local Development

```bash
# Start local validator
solana-test-validator

# Build and deploy
anchor build
anchor deploy
```

### Devnet

```bash
# Deploy to devnet
anchor build
anchor deploy --provider.cluster devnet
```

### Mainnet

```bash
# Deploy to mainnet (be careful!)
anchor build
anchor deploy --provider.cluster mainnet
```

## Monitoring

### Program Logs

The program emits structured logs for monitoring:

```typescript
// Fee collection
msg!("Collected fee: {} tokens", fee_amount);
msg!("Total fees collected: {}", total_fees);

// Configuration updates
msg!("Aggregator configuration updated");
msg!("Fee rate updated from {} to {} basis points", old_rate, new_rate);

// Swap execution
msg!("Swap executed successfully");
msg!("Input amount: {}", input_amount);
msg!("Output amount: {}", output_amount);
msg!("Fee collected: {}", fee_amount);
```

### Metrics

Track key metrics:
- Total fees collected
- Number of swaps executed
- Average fee per swap
- Configuration update frequency
- Error rates

## Integration with Rust Aggregator

The Anchor program integrates seamlessly with the Rust aggregator:

1. **Configuration Sync**: Rust aggregator reads on-chain configuration
2. **Fee Collection**: Rust aggregator calls fee collection instructions
3. **Swap Execution**: Rust aggregator executes swaps through the program
4. **State Monitoring**: Rust aggregator monitors program state

This provides a complete on-chain/off-chain solution for DEX aggregation with proper fee management and configuration control.
