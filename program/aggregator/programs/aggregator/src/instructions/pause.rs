use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::AggregatorError;

#[derive(Accounts)]
pub struct Pause<'info> {
    #[account(
        mut,
        seeds = [b"aggregator_state"],
        bump = aggregator_state.bump,
        has_one = admin
    )]
    pub aggregator_state: Account<'info, AggregatorState>,
    
    pub admin: Signer<'info>,
}

pub fn handler(ctx: Context<Pause>) -> Result<()> {
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    
    require!(
        !aggregator_state.is_paused,
        AggregatorError::ProgramPaused
    );
    
    aggregator_state.is_paused = true;
    
    msg!("Program paused by admin: {}", ctx.accounts.admin.key());
    
    Ok(())
}
