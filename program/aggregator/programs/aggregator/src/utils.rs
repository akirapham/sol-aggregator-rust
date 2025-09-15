use anchor_lang::prelude::*;
use crate::state::*;

/// Utility functions for the aggregator program

/// Calculate the aggregator fee for a given amount
pub fn calculate_aggregator_fee(amount: u64, fee_rate: u64) -> Result<u64> {
    Ok((amount * fee_rate) / 10000)
}

/// Validate that a swap amount is within acceptable limits
pub fn validate_swap_amount(amount: u64) -> Result<()> {
    require!(
        amount > 0,
        AggregatorError::InvalidAmount
    );
    
    require!(
        amount <= u64::MAX / 2, // Prevent overflow in calculations
        AggregatorError::MathOverflow
    );
    
    Ok(())
}

/// Calculate price impact percentage
pub fn calculate_price_impact(expected: u64, actual: u64) -> u64 {
    if expected == 0 {
        return 0;
    }
    
    let impact = ((expected - actual) * 10000) / expected;
    impact
}

/// Validate slippage tolerance
pub fn validate_slippage_tolerance(tolerance: u64) -> Result<()> {
    require!(
        tolerance <= 10000, // Max 100%
        AggregatorError::InvalidSlippageTolerance
    );
    
    Ok(())
}

/// Check if a DEX is supported
pub fn is_dex_supported(dex: DexType, supported_dexs: &[DexType]) -> bool {
    supported_dexs.contains(&dex)
}

/// Calculate minimum output amount based on slippage
pub fn calculate_min_output_amount(
    expected_output: u64,
    slippage_tolerance: u64,
) -> Result<u64> {
    let slippage_amount = (expected_output * slippage_tolerance) / 10000;
    let min_output = expected_output.checked_sub(slippage_amount)
        .ok_or(AggregatorError::MathOverflow)?;
    
    Ok(min_output)
}

/// Validate deadline
pub fn validate_deadline(deadline: i64) -> Result<()> {
    let current_time = Clock::get()?.unix_timestamp;
    
    require!(
        deadline > current_time,
        AggregatorError::SwapExecutionFailed
    );
    
    // Check if deadline is not too far in the future (e.g., 1 hour)
    require!(
        deadline - current_time <= 3600,
        AggregatorError::InvalidConfiguration
    );
    
    Ok(())
}

/// Calculate gas cost for a route
pub fn calculate_route_gas_cost(route: &SwapRoute) -> u64 {
    let base_gas = 5000; // Base gas cost
    let hop_gas = route.route_path.len() as u64 * 2000; // Additional gas per hop
    let priority_multiplier = match route.mev_risk {
        MevRisk::Low => 100,
        MevRisk::Medium => 110,
        MevRisk::High => 125,
        MevRisk::Critical => 150,
    };
    
    (base_gas + hop_gas) * priority_multiplier / 100
}

/// Validate route safety
pub fn validate_route_safety(route: &SwapRoute, config: &AggregatorConfig) -> Result<()> {
    // Check price impact
    require!(
        route.price_impact <= config.price_impact_threshold,
        AggregatorError::PriceImpactTooHigh
    );
    
    // Check liquidity depth
    require!(
        route.liquidity_depth >= config.min_liquidity_threshold,
        AggregatorError::LiquidityTooLow
    );
    
    // Check MEV risk
    let max_mev_risk = match config.mev_protection.max_mev_risk_tolerance {
        MevRisk::Low => 0,
        MevRisk::Medium => 1,
        MevRisk::High => 2,
        MevRisk::Critical => 3,
    };
    
    let route_mev_risk = match route.mev_risk {
        MevRisk::Low => 0,
        MevRisk::Medium => 1,
        MevRisk::High => 2,
        MevRisk::Critical => 3,
    };
    
    require!(
        route_mev_risk <= max_mev_risk,
        AggregatorError::MevRiskTooHigh
    );
    
    Ok(())
}

/// Create a swap log entry
pub fn create_swap_log(
    user: Pubkey,
    input_token: Pubkey,
    output_token: Pubkey,
    input_amount: u64,
    output_amount: u64,
    dex_used: DexType,
    fee_paid: u64,
    success: bool,
) -> SwapLog {
    SwapLog {
        user,
        input_token,
        output_token,
        input_amount,
        output_amount,
        dex_used,
        fee_paid,
        timestamp: Clock::get().unwrap().unix_timestamp,
        success,
        bump: 0, // Will be set when account is created
    }
}
