use anchor_lang::prelude::*;

pub mod state;
pub mod errors;
pub mod utils;
pub mod dex_executor;
pub mod instructions;

use state::*;

declare_id!("3B2SkLKBUWWL5q3Py7k4UttJNGBpZn4QWViQGMdmmXJ8");

#[program]
pub mod aggregator {
    use super::*;

    /// Initialize the aggregator program
    pub fn initialize(
        ctx: Context<instructions::initialize::Initialize>,
        fee_rate: u64, // Fee rate in basis points (e.g., 10 = 0.1%)
        admin: Pubkey,
    ) -> Result<()> {
        instructions::initialize::handler(ctx, fee_rate, admin)
    }

    /// Execute a swap using the best route from the aggregator
    pub fn execute_swap(
        ctx: Context<instructions::execute_swap::ExecuteSwap>,
        swap_params: SwapParams,
        route: SwapRoute,
    ) -> Result<SwapResult> {
        instructions::execute_swap::handler(ctx, swap_params, route)
    }

    /// Update aggregator configuration
    pub fn update_config(
        ctx: Context<instructions::update_config::UpdateConfig>,
        new_config: AggregatorConfig,
    ) -> Result<()> {
        instructions::update_config::handler(ctx, new_config)
    }

    /// Update fee rate
    pub fn update_fee_rate(
        ctx: Context<instructions::update_fee_rate::UpdateFeeRate>,
        new_fee_rate: u64,
    ) -> Result<()> {
        instructions::update_fee_rate::handler(ctx, new_fee_rate)
    }

    /// Pause the program
    pub fn pause(ctx: Context<instructions::pause::Pause>) -> Result<()> {
        instructions::pause::handler(ctx)
    }

    /// Unpause the program
    pub fn unpause(ctx: Context<instructions::unpause::Unpause>) -> Result<()> {
        instructions::unpause::handler(ctx)
    }
}