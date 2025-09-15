use anchor_lang::prelude::*;
use crate::state::*;

#[derive(Accounts)]
pub struct WithdrawFees<'info> {
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
    
    #[account(mut)]
    pub admin_wallet: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<WithdrawFees>,
    amount: u64,
) -> Result<()> {
    let fee_collection = &mut ctx.accounts.fee_collection;
    
    require!(
        amount <= fee_collection.total_fees,
        AggregatorError::InsufficientFunds
    );
    
    // Update fee collection
    fee_collection.total_fees = fee_collection.total_fees
        .checked_sub(amount)
        .ok_or(AggregatorError::MathOverflow)?;
    
    // Transfer to admin wallet (simplified - in reality you'd handle the actual transfer)
    msg!("Withdrawing {} lamports to admin wallet", amount);
    
    Ok(())
}
