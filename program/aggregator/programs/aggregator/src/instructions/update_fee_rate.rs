use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::AggregatorError;

#[derive(Accounts)]
pub struct UpdateFeeRate<'info> {
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
    ctx: Context<UpdateFeeRate>,
    new_fee_rate: u64,
) -> Result<()> {
    require!(
        new_fee_rate <= 10000, // Max 100% in basis points
        AggregatorError::InvalidFeeRate
    );
    
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    aggregator_state.fee_rate = new_fee_rate;
    
    msg!("Fee rate updated to: {} bps", new_fee_rate);
    
    Ok(())
}
