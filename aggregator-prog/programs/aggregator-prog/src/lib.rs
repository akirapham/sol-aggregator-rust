use anchor_lang::{prelude::*, solana_program::{instruction::Instruction, program::invoke}};

declare_id!("8gCcELWdPcE8cN5j7b9KDjBhQBXCU4hiWiRPSb8FsStG");

#[program]
pub mod aggregator_prog {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }

    // Execute a series of DEX swaps/operations
    // This is the main entry point for routing through multiple DEXes
    pub fn execute_route(
        ctx: Context<ExecuteRoute>,
        route_data: RouteData,
    ) -> Result<()> {
        msg!("Executing route with {} steps", route_data.steps.len());

        // Validate route data
        require!(route_data.steps.len() > 0, AggregatorError::EmptyRoute);
        require!(route_data.steps.len() <= MAX_ROUTE_STEPS, AggregatorError::TooManySteps);

        // Execute each step in the route
        for (i, step) in route_data.steps.iter().enumerate() {
            msg!("Executing step {}: DEX {:?}", i, step.dex_type);

            // Validate step
            require!(step.accounts.len() <= MAX_ACCOUNTS_PER_STEP, AggregatorError::TooManyAccounts);

            // Execute the DEX call
            execute_dex_call(ctx.remaining_accounts, step)?;
        }

        msg!("Route execution completed successfully");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    initializer: Signer<'info>,

    #[account(
        init,
        payer = initializer,
        space = 8 + 32 + 1,
        seeds = [b"program_config".as_ref(), &1_u64.to_le_bytes()],
        bump
    )]
    pub config: Account<'info, ProgramConfig>,
    pub system_program: Program<'info, System>,
}

/// Execute a call to a specific DEX program
fn execute_dex_call(
    remaining_accounts: &[AccountInfo],
    step: &RouteStep,
) -> Result<()> {
    // Validate accounts count
    require!(
        remaining_accounts.len() >= step.accounts.len(),
        AggregatorError::InsufficientAccounts
    );

    // Build the instruction for the DEX program
    let instruction = Instruction {
        program_id: step.program_id,
        accounts: step.accounts.iter().enumerate().map(|(i, account_meta)| {
            let account_info = &remaining_accounts[i];
            anchor_lang::solana_program::instruction::AccountMeta {
                pubkey: *account_info.key,
                is_signer: account_meta.is_signer,
                is_writable: account_meta.is_writable,
            }
        }).collect(),
        data: step.instruction_data.clone(),
    };

    // Get account infos for the instruction
    let account_infos: Vec<AccountInfo> = step.accounts.iter().enumerate()
        .map(|(i, _)| remaining_accounts[i].clone())
        .collect();

    // Invoke the DEX program
    invoke(&instruction, &account_infos)?;

    msg!("DEX call executed successfully");
    Ok(())
}

// ============================================================================
// Account Structs
// ============================================================================

#[derive(Accounts)]
pub struct ExecuteRoute<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut)]
    /// CHECK: User's source token account, validated by the DEX program during execution
    pub user_source_token: AccountInfo<'info>,

    #[account(mut)]
    /// CHECK: User's destination token account, validated by the DEX program during execution
    pub user_destination_token: AccountInfo<'info>,

    /// program fee vault, should have validation
    #[account(mut)]
    pub program_fee_vault: AccountInfo<'info>,

    /// System program
    pub system_program: Program<'info, System>,

    /// CHECK: Token program account, validated by the DEX program during execution
    pub token_program: AccountInfo<'info>,
}

// ============================================================================
// Data Structs
// ============================================================================

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct RouteData {
    /// Expected input amount
    pub input_amount: u64,
    /// Minimum output amount (slippage protection)
    pub minimum_output_amount: u64,
    /// Source token mint
    pub source_mint: Pubkey,
    /// Destination token mint
    pub destination_mint: Pubkey,
    /// Route steps to execute
    pub steps: Vec<RouteStep>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct RouteStep {
    /// Type of DEX (Raydium, Orca, PumpFun, etc.)
    pub dex_type: DexType,
    /// Program ID of the DEX
    pub program_id: Pubkey,
    /// Instruction data for the DEX call
    pub instruction_data: Vec<u8>,
    /// Account metas for the instruction
    pub accounts: Vec<AccountMeta>,
    /// Expected output amount for this step
    pub expected_output_amount: u64,
    /// Slippage tolerance for this step (basis points)
    pub slippage_tolerance: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AccountMeta {
    /// Public key of the account
    pub pubkey: Pubkey,
    /// Whether the account is a signer
    pub is_signer: bool,
    /// Whether the account is writable
    pub is_writable: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub enum DexType {
    RaydiumV4,
    RaydiumCLMM,
    RaydiumCPMM,
    Orca,
    OrcaWhirlpool,
    PumpFun,
    PumpSwap,
    Jupiter,
    Serum,
    Saber,
    Mercurial,
    Custom { program_id: Pubkey }, // For custom/unknown DEXes
}


/// Config account data
#[account]
pub struct ProgramConfig {
    /// Admin authority
    pub admin: Pubkey,
    pub bump: u8,
}

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of steps in a route
pub const MAX_ROUTE_STEPS: usize = 5;

/// Maximum number of accounts per step
pub const MAX_ACCOUNTS_PER_STEP: usize = 20;

// ============================================================================
// Errors
// ============================================================================

#[error_code]
pub enum AggregatorError {
    #[msg("Route cannot be empty")]
    EmptyRoute,

    #[msg("Too many steps in route")]
    TooManySteps,

    #[msg("Too many accounts for this step")]
    TooManyAccounts,

    #[msg("Insufficient accounts provided")]
    InsufficientAccounts,

    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,

    #[msg("Invalid DEX type")]
    InvalidDexType,

    #[msg("Invalid instruction data")]
    InvalidInstructionData,

    #[msg("Unauthorized access")]
    Unauthorized,
}
