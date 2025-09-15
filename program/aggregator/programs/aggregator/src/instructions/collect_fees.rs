use anchor_lang::prelude::*;
use crate::state::*;

#[derive(Accounts)]
pub struct CollectFees<'info> {
    #[account(
        mut,
        seeds = [b"aggregator_state"],
        bump = aggregator_state.bump,
        has_one = admin
    )]
    pub aggregator_state: Account<'info, AggregatorState>,
    
    #[account(mut)]
    pub admin: Signer<'info>,
    
    #[account(mut)]
    pub fee_collection: Account<'info, FeeCollection>,
    
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<CollectFees>,
    amount: u64,
) -> Result<()> {
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    let fee_collection = &mut ctx.accounts.fee_collection;
    
    require!(
        amount <= aggregator_state.total_fees_collected,
        AggregatorError::InsufficientFunds
    );
    
    // Update fee collection
    fee_collection.total_fees = fee_collection.total_fees
        .checked_add(amount)
        .ok_or(AggregatorError::MathOverflow)?;
    
    fee_collection.last_collected = Clock::get()?.unix_timestamp;
    
    // Update aggregator state
    aggregator_state.total_fees_collected = aggregator_state.total_fees_collected
        .checked_sub(amount)
        .ok_or(AggregatorError::MathOverflow)?;
    
    msg!("Collected {} lamports in fees", amount);
    
    Ok(())
}
