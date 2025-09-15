use anchor_lang::prelude::*;
use crate::state::*;
use crate::dex_executor::DexExecutorFactory;

#[derive(Accounts)]
pub struct ExecuteMultiHopSwap<'info> {
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
    ctx: Context<ExecuteMultiHopSwap>,
    swap_params: SwapParams,
    routes: Vec<SwapRoute>,
) -> Result<SwapResult> {
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    
    // Check if program is paused
    require!(!aggregator_state.is_paused, AggregatorError::ProgramPaused);
    
    // Validate we have at least 2 routes for multi-hop
    require!(
        routes.len() >= 2,
        AggregatorError::InvalidSwapRoute
    );
    
    // Validate deadline
    require!(
        Clock::get()?.unix_timestamp <= swap_params.deadline,
        AggregatorError::SwapExecutionFailed
    );
    
    // Validate route chain
    validate_route_chain(&swap_params, &routes)?;
    
    let mut total_output = 0u64;
    let mut total_fee = 0u64;
    let mut total_gas = 0u64;
    let mut execution_time = 0u64;
    let mut last_dex = DexType::PumpFun; // Default
    
    // Execute each hop in sequence
    for (i, route) in routes.iter().enumerate() {
        let executor = DexExecutorFactory::create_executor(route)?;
        
        // Create modified swap params for this hop
        let hop_params = SwapParams {
            input_token: route.input_token,
            output_token: route.output_token,
            input_amount: if i == 0 { swap_params.input_amount } else { total_output },
            min_output_amount: route.expected_output_amount,
            slippage_tolerance: swap_params.slippage_tolerance,
            user_wallet: swap_params.user_wallet,
            priority: swap_params.priority,
            deadline: swap_params.deadline,
        };
        
        let result = executor.execute_swap(&hop_params, route)?;
        
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
        
        total_output = result.actual_output_amount;
        total_fee = total_fee.checked_add(result.fee_paid).ok_or(AggregatorError::MathOverflow)?;
        total_gas = total_gas.checked_add(result.gas_used).ok_or(AggregatorError::MathOverflow)?;
        execution_time = execution_time.checked_add(result.execution_time_ms).ok_or(AggregatorError::MathOverflow)?;
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

/// Validate that routes form a valid chain
fn validate_route_chain(
    swap_params: &SwapParams,
    routes: &[SwapRoute],
) -> Result<()> {
    // First route must start with input token
    require!(
        routes[0].input_token == swap_params.input_token,
        AggregatorError::InvalidSwapRoute
    );
    
    // Last route must end with output token
    require!(
        routes.last().unwrap().output_token == swap_params.output_token,
        AggregatorError::InvalidSwapRoute
    );
    
    // Each route's output must be the next route's input
    for i in 0..routes.len() - 1 {
        require!(
            routes[i].output_token == routes[i + 1].input_token,
            AggregatorError::InvalidSwapRoute
        );
    }
    
    Ok(())
}
