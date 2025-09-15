use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::AggregatorError;

#[derive(Accounts)]
pub struct Unpause<'info> {
    #[account(
        mut,
        seeds = [b"aggregator_state"],
        bump = aggregator_state.bump,
        has_one = admin
    )]
    pub aggregator_state: Account<'info, AggregatorState>,
    
    pub admin: Signer<'info>,
}

pub fn handler(ctx: Context<Unpause>) -> Result<()> {
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    
    require!(
        aggregator_state.is_paused,
        AggregatorError::ProgramPaused
    );
    
    aggregator_state.is_paused = false;
    
    msg!("Program unpaused by admin: {}", ctx.accounts.admin.key());
    
    Ok(())
}
