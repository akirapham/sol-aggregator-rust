use crate::types::SwapParams;
use crate::{
    constants::wsol,
    pool_data_types::{
        BonkPoolState, BuildSwapInstruction, DbcPoolState, DexType, GetAmmConfig,
        MeteoraDammV2PoolState, MeteoraDlmmPoolState, PumpSwapPoolState, PumpfunPoolState,
        RaydiumAmmV4PoolState, RaydiumClmmPoolState, RaydiumCpmmPoolState, WhirlpoolPoolState,
    },
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

/// Macro to delegate simple field access across all PoolState variants
/// Usage: pool_state_delegate!(self, field_name)
macro_rules! pool_state_delegate {
    ($self:expr, $field:ident) => {
        match $self {
            PoolState::Pumpfun(state) => state.$field,
            PoolState::PumpSwap(state) => state.$field,
            PoolState::RaydiumAmmV4(state) => state.$field,
            PoolState::RaydiumCpmm(state) => state.$field,
            PoolState::Bonk(state) => state.$field,
            PoolState::RadyiumClmm(state) => state.$field,
            PoolState::MeteoraDbc(state) => state.$field,
            PoolState::MeteoraDammV2(state) => state.$field,
            PoolState::OrcaWhirlpool(state) => state.$field,
            PoolState::MeteoraDlmm(state) => state.$field,
        }
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolState {
    Pumpfun(PumpfunPoolState),
    PumpSwap(PumpSwapPoolState),
    RaydiumAmmV4(RaydiumAmmV4PoolState),
    RaydiumCpmm(RaydiumCpmmPoolState),
    Bonk(BonkPoolState),
    RadyiumClmm(Box<RaydiumClmmPoolState>),
    MeteoraDbc(Box<DbcPoolState>),
    MeteoraDammV2(Box<MeteoraDammV2PoolState>),
    OrcaWhirlpool(WhirlpoolPoolState),
    MeteoraDlmm(Box<MeteoraDlmmPoolState>),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PoolUpdateEventType {
    // PumpSwap
    PumpSwapBuy,
    PumpSwapSell,
    PumpSwapCreatePool,
    PumpSwapDeposit,
    PumpSwapWithdraw,
    PumpSwapPoolAccount,
    // PumpFun
    PumpFunTrade,
    PumpFunMigrate,
    PumpFunBuy,
    PumpFunSell,
    PumpFunCreateToken,
    PumpFunBondingCurveAccount,
    // Raydium CPMM
    RaydiumCpmmSwap,
    RaydiumCpmmDeposit,
    RaydiumCpmmWithdraw,
    RaydiumCpmmInitialize,
    RaydiumCpmmPoolStateAccount,
    // Raydium CLMM
    RaydiumClmmSwap,
    RaydiumClmmSwapV2,
    RaydiumClmmIncreaseLiquidity,
    RaydiumClmmIncreaseLiquidityV2,
    RaydiumClmmDecreaseLiquidity,
    RaydiumClmmDecreaseLiquidityV2,
    RaydiumClmmClosePosition,
    RaydiumClmmOpenPositionV2,
    RaydiumClmmOpenPositionWithToken22Nft,
    RaydiumClmmPoolStateAccount,
    RaydiumClmmTickArrayStateAccount,
    RaydiumClmmTickArrayBitmapExtensionAccount,
    // Raydium AMM V4
    RaydiumAmmV4Swap, // General swap event
    RaydiumAmmV4SwapBaseIn,
    RaydiumAmmV4SwapBaseOut,
    RaydiumAmmV4Deposit,
    RaydiumAmmV4Initialize2,
    RaydiumAmmV4Withdraw,
    RaydiumAmmV4WithdrawPnl,
    RaydiumAmmV4AmmInfoAccount,
    // Whirlpool
    WhirlpoolSwap,
    WhirlpoolSwapV2,
    WhirlpoolIncreaseLiquidity,
    WhirlpoolIncreaseLiquidityV2,
    WhirlpoolDecreaseLiquidity,
    WhirlpoolDecreaseLiquidityV2,
    WhirlpoolTwoHopSwap,
    WhirlpoolTwoHopSwapV2,
    WhirlpoolPoolStateAccount,
    WhirlpoolTickArrayStateAccount,
    WhirlpoolOracleStateAccount,
    // Meteora DLMM
    MeteoraDlmmSwap,
    MeteoraDlmmLbPairAccount,
    MeteoraDlmmBinArrayAccount,
    MeteoraDlmmBinArrayBitmapExtensionAccount,
    // Meteora DBC
    DbcVirtualPoolAccount,
    DbcPoolConfigAccount,
    // Meteora DAMM V2
    MeteoraDammV2PoolStateAccount,
    // Bonk
    BonkPoolStateAccount,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PoolStateMetadata {
    pub slot: u64,
    pub transaction_index: Option<u64>,
}

impl PoolState {
    pub fn last_updated(&self) -> u64 {
        pool_state_delegate!(self, last_updated)
    }

    pub fn address(&self) -> Pubkey {
        pool_state_delegate!(self, address)
    }

    pub fn get_tokens(&self) -> (Pubkey, Pubkey) {
        match self {
            PoolState::Pumpfun(state) => (state.mint, wsol()),
            PoolState::PumpSwap(state) => (state.base_mint, state.quote_mint),
            PoolState::RaydiumAmmV4(state) => (state.base_mint, state.quote_mint),
            PoolState::RaydiumCpmm(state) => (state.token0, state.token1),
            PoolState::Bonk(state) => (state.base_mint, state.quote_mint),
            PoolState::RadyiumClmm(state) => (state.token_mint0, state.token_mint1),
            PoolState::MeteoraDbc(state) => {
                let quote_mint = state
                    .pool_config
                    .as_ref()
                    .map(|config| config.quote_mint)
                    .unwrap_or_default();
                (state.base_mint, quote_mint)
            }
            PoolState::OrcaWhirlpool(state) => (state.token_mint_a, state.token_mint_b),
            PoolState::MeteoraDammV2(state) => (state.token_a_mint, state.token_b_mint),
            PoolState::MeteoraDlmm(state) => (state.lbpair.token_x_mint, state.lbpair.token_y_mint),
        }
    }

    pub fn dex(&self) -> DexType {
        match self {
            PoolState::Pumpfun(_) => DexType::PumpFun,
            PoolState::PumpSwap(_) => DexType::PumpFunSwap,
            PoolState::RaydiumAmmV4(_) => DexType::Raydium,
            PoolState::RaydiumCpmm(_) => DexType::RaydiumCpmm,
            PoolState::Bonk(_) => DexType::Bonk,
            PoolState::RadyiumClmm(_) => DexType::RaydiumClmm,
            PoolState::MeteoraDbc(_) => DexType::MeteoraDbc,
            PoolState::OrcaWhirlpool(_) => DexType::Orca,
            PoolState::MeteoraDammV2(_) => DexType::MeteoraDammV2,
            PoolState::MeteoraDlmm(_) => DexType::MeteoraDlmm,
        }
    }

    pub fn get_metadata(&self) -> PoolStateMetadata {
        match self {
            PoolState::Pumpfun(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::PumpSwap(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RaydiumAmmV4(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RaydiumCpmm(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::Bonk(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RadyiumClmm(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::MeteoraDbc(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::OrcaWhirlpool(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::MeteoraDammV2(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::MeteoraDlmm(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
        }
    }

    pub fn get_reserves(&self) -> (u64, u64) {
        match self {
            PoolState::Pumpfun(state) => (state.virtual_token_reserves, state.virtual_sol_reserves),
            PoolState::PumpSwap(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RaydiumAmmV4(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RaydiumCpmm(state) => (state.token0_reserve, state.token1_reserve),
            PoolState::Bonk(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RadyiumClmm(state) => (state.token0_reserve, state.token1_reserve),
            PoolState::MeteoraDbc(state) => (state.base_reserve, state.quote_reserve),
            PoolState::OrcaWhirlpool(state) => (state.token_a_reserve, state.token_b_reserve),
            PoolState::MeteoraDammV2(state) => {
                // Calculate reserves from DAMM V2 state
                let delta_sqrt_price_b = state.sqrt_price.saturating_sub(state.sqrt_min_price);
                let product_b = state.liquidity.saturating_mul(delta_sqrt_price_b);
                let reserve_b = if product_b > 0 {
                    (product_b / (1u128 << 64) / (1u128 << 64)) as u64
                } else {
                    0
                };

                let numerator = state
                    .liquidity
                    .saturating_mul(state.sqrt_max_price.saturating_sub(state.sqrt_price));
                let denominator = state.sqrt_max_price.saturating_mul(state.sqrt_price);
                let reserve_a = if denominator > 0 {
                    (numerator / denominator) as u64
                } else {
                    0
                };

                (reserve_a, reserve_b)
            }
            PoolState::MeteoraDlmm(state) => (state.reserve_x.unwrap(), state.reserve_y.unwrap()),
        }
    }

    pub fn get_liquidity_usd(&self) -> f64 {
        pool_state_delegate!(self, liquidity_usd)
    }

    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        amm_config_fetcher: &dyn GetAmmConfig,
    ) -> u64 {
        match self {
            PoolState::Pumpfun(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::PumpSwap(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::RaydiumAmmV4(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::RaydiumCpmm(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::Bonk(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::RadyiumClmm(state) => {
                let output_amount = state
                    .calculate_output_amount(input_token, input_amount, amm_config_fetcher)
                    .await;
                output_amount
            }
            PoolState::MeteoraDbc(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::OrcaWhirlpool(state) => {
                let output_amount = state
                    .calculate_output_amount(input_token, input_amount, amm_config_fetcher)
                    .await;
                output_amount
            }
            PoolState::MeteoraDammV2(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
            PoolState::MeteoraDlmm(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_config_fetcher)
            }
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        match self {
            PoolState::Pumpfun(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::PumpSwap(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::RaydiumAmmV4(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::RaydiumCpmm(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::Bonk(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::RadyiumClmm(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::MeteoraDbc(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::OrcaWhirlpool(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::MeteoraDammV2(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
            PoolState::MeteoraDlmm(state) => {
                state.calculate_token_prices(sol_price, base_decimals, quote_decimals)
            }
        }
    }
}

#[async_trait]
impl BuildSwapInstruction for PoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: &dyn GetAmmConfig,
    ) -> std::result::Result<Vec<Instruction>, String> {
        // Validation
        if params.input_amount == 0 {
            return Err("Input amount is zero".to_string());
        }
        if params.input_token.address == params.output_token.address {
            return Err("Input and output tokens are the same".to_string());
        }
        if params.slippage_bps > 500 {
            return Err("Slippage tolerance exceeds maximum allowed (5%)".to_string());
        }

        match self {
            PoolState::Pumpfun(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("Pumpfun build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::PumpSwap(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("PumpSwap build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::RaydiumAmmV4(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("RaydiumAmmV4 build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::RaydiumCpmm(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("RaydiumCpmm build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::Bonk(_state) => {
                let e = "Bonk BuildSwapInstruction not yet implemented".to_string();
                log::error!("{}", e);
                Err(e)
            }
            PoolState::RadyiumClmm(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("RadyiumClmm build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::MeteoraDbc(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("MeteoraDbc build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::OrcaWhirlpool(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("OrcaWhirlpool build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::MeteoraDammV2(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("MeteoraDammV2 build_swap_instruction error: {}", e);
                    e
                }),
            PoolState::MeteoraDlmm(state) => state
                .build_swap_instruction(params, amm_config_fetcher)
                .await
                .map_err(|e| {
                    log::error!("MeteoraDlmm build_swap_instruction error: {}", e);
                    e
                }),
        }
    }
}
