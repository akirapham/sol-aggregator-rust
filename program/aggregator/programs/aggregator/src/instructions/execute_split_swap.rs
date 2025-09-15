use anchor_lang::prelude::*;
use crate::state::*;
use crate::dex_executor::DexExecutorFactory;

#[derive(Accounts)]
pub struct ExecuteSplitSwap<'info> {
    #[account(mut)]
    pub aggregator_state: Account<'info, AggregatorState>,
    
    #[account(mut)]
    pub user_wallet: Signer<'info>,
    
    #[account(mut)]
    pub input_token_account: AccountInfo<'info>,
    
    #[account(mut)]
    pub output_token_account: AccountInfo<'info>,
    
    /// CHECK: These are the DEX programs being called
    pub dex_program_1: AccountInfo<'info>,
    pub dex_program_2: AccountInfo<'info>,
    
    pub token_program: Program<'info, anchor_spl::token::Token>,
    
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<ExecuteSplitSwap>,
    swap_params: SwapParams,
    split_routes: Vec<SplitRoute>,
) -> Result<SwapResult> {
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    
    // Check if program is paused
    require!(!aggregator_state.is_paused, AggregatorError::ProgramPaused);
    
    // Validate we have at least 2 routes for split
    require!(
        split_routes.len() >= 2,
        AggregatorError::InvalidSwapRoute
    );
    
    // Validate deadline
    require!(
        Clock::get()?.unix_timestamp <= swap_params.deadline,
        AggregatorError::SwapExecutionFailed
    );
    
    // Validate split percentages add up to 100%
    validate_split_percentages(&split_routes)?;
    
    // Validate all routes have same input/output tokens
    validate_split_routes(&swap_params, &split_routes)?;
    
    let mut total_output = 0u64;
    let mut total_fee = 0u64;
    let mut total_gas = 0u64;
    let mut execution_time = 0u64;
    let mut last_dex = DexType::PumpFun; // Default
    
    // Execute all routes in parallel (simplified - in reality you'd handle parallel execution)
    for split_route in &split_routes {
        let executor = DexExecutorFactory::create_executor(&split_route.route)?;
        
        // Create modified swap params for this split
        let split_params = SwapParams {
            input_token: swap_params.input_token,
            output_token: swap_params.output_token,
            input_amount: split_route.split_amount,
            min_output_amount: (split_route.route.expected_output_amount * split_route.split_percentage) / 10000,
            slippage_tolerance: swap_params.slippage_tolerance,
            user_wallet: swap_params.user_wallet,
            priority: swap_params.priority,
            deadline: swap_params.deadline,
        };
        
        let result = executor.execute_swap(&split_params, &split_route.route)?;
        
        if !result.success {
            return Ok(SwapResult {
                success: false,
                actual_output_amount: 0,
                fee_paid: total_fee,
                gas_used: total_gas,
                execution_time_ms: execution_time,
                price_impact_actual: 0,
                error_code: result.error_code,
                dex_used: last_dex,
            });
        }
        
        total_output = total_output.checked_add(result.actual_output_amount).ok_or(AggregatorError::MathOverflow)?;
        total_fee = total_fee.checked_add(result.fee_paid).ok_or(AggregatorError::MathOverflow)?;
        total_gas = total_gas.checked_add(result.gas_used).ok_or(AggregatorError::MathOverflow)?;
        execution_time = execution_time.max(result.execution_time_ms); // Use max for parallel execution
        last_dex = result.dex_used;
    }
    
    // Validate final output meets minimum requirement
    require!(
        total_output >= swap_params.min_output_amount,
        AggregatorError::SwapExecutionFailed
    );
    
    // Update aggregator state
    if total_output > 0 {
        aggregator_state.total_swaps_executed = aggregator_state.total_swaps_executed
            .checked_add(1)
            .ok_or(AggregatorError::MathOverflow)?;
        
        aggregator_state.total_volume = aggregator_state.total_volume
            .checked_add(swap_params.input_amount)
            .ok_or(AggregatorError::MathOverflow)?;
        
        aggregator_state.total_fees_collected = aggregator_state.total_fees_collected
            .checked_add(total_fee)
            .ok_or(AggregatorError::MathOverflow)?;
    }
    
    Ok(SwapResult {
        success: true,
        actual_output_amount: total_output,
        fee_paid: total_fee,
        gas_used: total_gas,
        execution_time_ms: execution_time,
        price_impact_actual: 0, // Calculate based on final vs expected
        error_code: None,
        dex_used: last_dex,
    })
}

/// Validate that split percentages add up to 100%
fn validate_split_percentages(split_routes: &[SplitRoute]) -> Result<()> {
    let total_percentage: u64 = split_routes
        .iter()
        .map(|route| route.split_percentage)
        .sum();
    
    require!(
        total_percentage == 10000, // 100% in basis points
        AggregatorError::InvalidSwapRoute
    );
    
    Ok(())
}

/// Validate that all split routes have the same input/output tokens
fn validate_split_routes(
    swap_params: &SwapParams,
    split_routes: &[SplitRoute],
) -> Result<()> {
    for split_route in split_routes {
        require!(
            split_route.route.input_token == swap_params.input_token,
            AggregatorError::InvalidSwapRoute
        );
        
        require!(
            split_route.route.output_token == swap_params.output_token,
            AggregatorError::InvalidSwapRoute
        );
    }
    
    Ok(())
}
