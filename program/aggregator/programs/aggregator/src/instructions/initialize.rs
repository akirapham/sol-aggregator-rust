use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::AggregatorError;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = AggregatorState::INIT_SPACE,
        seeds = [b"aggregator_state"],
        bump
    )]
    pub aggregator_state: Account<'info, AggregatorState>,
    
    #[account(mut)]
    pub admin: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<Initialize>,
    fee_rate: u64,
    admin: Pubkey,
) -> Result<()> {
    require!(
        fee_rate <= 10000, // Max 100% in basis points
        AggregatorError::InvalidFeeRate
    );
    
    let aggregator_state = &mut ctx.accounts.aggregator_state;
    
    aggregator_state.admin = admin;
    aggregator_state.fee_rate = fee_rate;
    aggregator_state.total_fees_collected = 0;
    aggregator_state.total_swaps_executed = 0;
    aggregator_state.total_volume = 0;
    aggregator_state.is_paused = false;
    aggregator_state.config = AggregatorConfig {
        max_slippage: 500, // 5% default
        max_routes: 5,
        min_liquidity_threshold: 1000000, // 1 SOL default
        price_impact_threshold: 1000, // 10% default
        mev_protection: MevProtectionConfig {
            max_slippage_tolerance: 300, // 3%
            min_liquidity_threshold: 1000000,
            max_mev_risk_tolerance: MevRisk::Medium,
            use_private_mempool: false,
        },
        supported_dexs: vec![
            DexType::PumpFun,
            DexType::Raydium,
            DexType::Orca,
            DexType::Jupiter,
        ],
    };
    aggregator_state.bump = ctx.bumps.aggregator_state;
    
    msg!("Aggregator initialized with fee rate: {} bps", fee_rate);
    msg!("Admin: {}", admin);
    
    Ok(())
}
