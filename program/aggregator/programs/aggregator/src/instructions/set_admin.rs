use anchor_lang::prelude::*;
use crate::state::*;

#[derive(Accounts)]
pub struct SetAdmin<'info> {
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
    ctx: Context<SetAdmin>,
    new_admin: Pubkey,
) -> Result<()> {
    require!(
        new_admin != Pubkey::default(),
        AggregatorError::InvalidConfiguration
    );
    
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    let old_admin = aggregator_state.admin;
    aggregator_state.admin = new_admin;
    
    msg!("Admin changed from {} to {}", old_admin, new_admin);
    
    Ok(())
}
