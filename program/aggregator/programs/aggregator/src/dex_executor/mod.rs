pub mod pumpfun;
pub mod raydium;
pub mod orca;
pub mod jupiter;

use anchor_lang::prelude::*;
use crate::state::{DexType, SwapRoute, SwapParams, SwapResult};

/// DEX executor trait for executing swaps on different DEXs
pub trait DexExecutor {
    /// Execute a swap on the specific DEX
    fn execute_swap(
        &self,
        swap_params: &SwapParams,
        route: &SwapRoute,
    ) -> Result<SwapResult>;
    
    /// Validate the route before execution
    fn validate_route(&self, route: &SwapRoute) -> Result<()>;
    
    /// Get the DEX type
    fn get_dex_type(&self) -> DexType;
}

/// DEX executor factory
pub struct DexExecutorFactory;

impl DexExecutorFactory {
    /// Create a DEX executor based on the route's DEX type
    pub fn create_executor(route: &SwapRoute) -> Result<Box<dyn DexExecutor>> {
        match route.dex {
            DexType::PumpFun => Ok(Box::new(pumpfun::PumpFunExecutor::new())),
            DexType::Raydium => Ok(Box::new(raydium::RaydiumExecutor::new())),
            DexType::Orca => Ok(Box::new(orca::OrcaExecutor::new())),
            DexType::Jupiter => Ok(Box::new(jupiter::JupiterExecutor::new())),
            _ => Err(AggregatorError::DexNotSupported.into()),
        }
    }
}

/// Common utilities for DEX execution
pub mod utils {
    use anchor_lang::prelude::*;
    use crate::state::{SwapResult, DexType};
    
    /// Calculate actual price impact
    pub fn calculate_price_impact(
        expected_amount: u64,
        actual_amount: u64,
    ) -> u64 {
        if expected_amount == 0 {
            return 0;
        }
        
        let impact = ((expected_amount - actual_amount) * 10000) / expected_amount;
        impact
    }
    
    /// Validate slippage tolerance
    pub fn validate_slippage(
        expected_amount: u64,
        actual_amount: u64,
        slippage_tolerance: u64,
    ) -> Result<()> {
        let actual_slippage = calculate_price_impact(expected_amount, actual_amount);
        
        require!(
            actual_slippage <= slippage_tolerance,
            AggregatorError::PriceImpactTooHigh
        );
        
        Ok(())
    }
    
    /// Create a successful swap result
    pub fn create_success_result(
        dex: DexType,
        actual_output_amount: u64,
        fee_paid: u64,
        gas_used: u64,
        execution_time_ms: u64,
        price_impact_actual: u64,
    ) -> SwapResult {
        SwapResult {
            success: true,
            actual_output_amount,
            fee_paid,
            gas_used,
            execution_time_ms,
            price_impact_actual,
            error_code: None,
            dex_used: dex,
        }
    }
    
    /// Create a failed swap result
    pub fn create_failure_result(
        dex: DexType,
        error_code: u32,
    ) -> SwapResult {
        SwapResult {
            success: false,
            actual_output_amount: 0,
            fee_paid: 0,
            gas_used: 0,
            execution_time_ms: 0,
            price_impact_actual: 0,
            error_code: Some(error_code),
            dex_used: dex,
        }
    }
}
