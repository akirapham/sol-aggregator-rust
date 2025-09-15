use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::AggregatorError;

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"aggregator_state"],
        bump = aggregator_state.bump,
        has_one = admin
    )]
    pub aggregator_state: Account<'info, AggregatorState>,
    
    pub admin: Signer<'info>,
}

pub fn handler(
    ctx: Context<UpdateConfig>,
    new_config: AggregatorConfig,
) -> Result<()> {
    require!(
        new_config.max_slippage <= 10000, // Max 100%
        AggregatorError::InvalidSlippageTolerance
    );
    
    require!(
        new_config.max_routes > 0 && new_config.max_routes <= 10,
        AggregatorError::InvalidConfiguration
    );
    
    require!(
        new_config.price_impact_threshold <= 10000, // Max 100%
        AggregatorError::InvalidConfiguration
    );
    
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    aggregator_state.config = new_config;
    
    msg!("Aggregator configuration updated");
    
    Ok(())
}
