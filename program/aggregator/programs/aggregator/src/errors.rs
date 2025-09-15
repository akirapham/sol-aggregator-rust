use anchor_lang::prelude::*;

#[error_code]
pub enum AggregatorError {
    #[msg("Unauthorized: Only admin can perform this action")]
    Unauthorized,
    
    #[msg("Program is paused")]
    ProgramPaused,
    
    #[msg("Invalid fee rate: Must be between 0 and 10000 basis points")]
    InvalidFeeRate,
    
    #[msg("Invalid slippage tolerance: Must be between 0 and 10000 basis points")]
    InvalidSlippageTolerance,
    
    #[msg("Insufficient funds for operation")]
    InsufficientFunds,
    
    #[msg("Invalid swap route")]
    InvalidSwapRoute,
    
    #[msg("Price impact too high")]
    PriceImpactTooHigh,
    
    #[msg("MEV risk too high")]
    MevRiskTooHigh,
    
    #[msg("Invalid configuration")]
    InvalidConfiguration,
    
    #[msg("DEX not supported")]
    DexNotSupported,
    
    #[msg("DEX not enabled")]
    DexNotEnabled,
    
    #[msg("DEX already exists")]
    DexAlreadyExists,
    
    #[msg("DEX not found")]
    DexNotFound,
    
    #[msg("Invalid DEX configuration")]
    InvalidDexConfig,
    
    #[msg("Swap execution failed")]
    SwapExecutionFailed,
    
    #[msg("Token not supported")]
    TokenNotSupported,
    
    #[msg("Route not found")]
    RouteNotFound,
    
    #[msg("Invalid amount")]
    InvalidAmount,
    
    #[msg("Math overflow")]
    MathOverflow,
    
    #[msg("Invalid token account")]
    InvalidTokenAccount,
    
    #[msg("Transfer failed")]
    TransferFailed,
    
    #[msg("Invalid program ID")]
    InvalidProgramId,
    
    #[msg("Configuration too large")]
    ConfigurationTooLarge,
    
    #[msg("Invalid trade size")]
    InvalidTradeSize,
    
    #[msg("Liquidity too low")]
    LiquidityTooLow,
    
    #[msg("Route validation failed")]
    RouteValidationFailed,
}
