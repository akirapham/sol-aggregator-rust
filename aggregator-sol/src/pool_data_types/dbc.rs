use crate::pool_data_types::{GetAmmConfig, PoolUpdateEventType};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::parser::DBC_PROGRAM_ID;
pub use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::{
    LiquidityDistributionConfig, PoolConfig, PoolFeesConfig, VolatilityTracker,
};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbcPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    /// config key
    pub config: Pubkey,
    /// creator
    pub creator: Pubkey,
    /// base mint
    pub base_mint: Pubkey,
    /// base vault
    pub base_vault: Pubkey,
    /// quote vault
    pub quote_vault: Pubkey,
    /// base reserve
    pub base_reserve: u64,
    /// quote reserve
    pub quote_reserve: u64,
    /// protocol base fee
    pub protocol_base_fee: u64,
    /// protocol quote fee
    pub protocol_quote_fee: u64,
    /// partner base fee
    pub partner_base_fee: u64,
    /// trading quote fee
    pub partner_quote_fee: u64,
    /// current price
    pub sqrt_price: u128,
    /// Activation point
    pub activation_point: u64,
    /// pool type, spl token or token2022
    pub pool_type: u8,
    /// is migrated
    pub is_migrated: u8,
    /// is partner withdraw surplus
    pub is_partner_withdraw_surplus: u8,
    /// is protocol withdraw surplus
    pub is_protocol_withdraw_surplus: u8,
    /// migration progress
    pub migration_progress: u8,
    /// is withdraw leftover
    pub is_withdraw_leftover: u8,
    /// is creator withdraw surplus
    pub is_creator_withdraw_surplus: u8,
    /// migration fee withdraw status, first bit is for partner, second bit is for creator
    pub migration_fee_withdraw_status: u8,
    /// The time curve is finished
    pub finish_curve_timestamp: u64,
    /// creator base fee
    pub creator_base_fee: u64,
    /// creator quote fee
    pub creator_quote_fee: u64,
    pub liquidity_usd: f64,
    pub last_updated: u64,

    // PoolConfig parameters for quote calculation
    pub pool_config: Option<PoolConfig>,

    // Volatility Tracker from VirtualPool
    pub volatility_tracker: Option<VolatilityTracker>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DbcPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    /// config key
    pub config: Pubkey,
    /// creator
    pub creator: Pubkey,
    /// base mint
    pub base_mint: Pubkey,
    /// base vault
    pub base_vault: Pubkey,
    /// quote vault
    pub quote_vault: Pubkey,
    /// base reserve
    pub base_reserve: u64,
    /// quote reserve
    pub quote_reserve: u64,
    /// protocol base fee
    pub protocol_base_fee: u64,
    /// protocol quote fee
    pub protocol_quote_fee: u64,
    /// partner base fee
    pub partner_base_fee: u64,
    /// trading quote fee
    pub partner_quote_fee: u64,
    /// current price
    pub sqrt_price: u128,
    /// Activation point
    pub activation_point: u64,
    /// pool type, spl token or token2022
    pub pool_type: u8,
    /// is migrated
    pub is_migrated: u8,
    /// is partner withdraw surplus
    pub is_partner_withdraw_surplus: u8,
    /// is protocol withdraw surplus
    pub is_protocol_withdraw_surplus: u8,
    /// migration progress
    pub migration_progress: u8,
    /// is withdraw leftover
    pub is_withdraw_leftover: u8,
    /// is creator withdraw surplus
    pub is_creator_withdraw_surplus: u8,
    /// migration fee withdraw status, first bit is for partner, second bit is for creator
    pub migration_fee_withdraw_status: u8,
    /// The time curve is finished
    pub finish_curve_timestamp: u64,
    /// creator base fee
    pub creator_base_fee: u64,
    /// creator quote fee
    pub creator_quote_fee: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32,
    pub last_updated: u64,

    // PoolConfig parameters
    pub is_config_update: bool,
    pub pool_config: Option<PoolConfig>,

    // Volatility Tracker
    pub volatility_tracker: Option<VolatilityTracker>,
}

#[allow(dead_code)]
impl DbcPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::from_str(&DBC_PROGRAM_ID.to_string()).unwrap_or_else(|_| Pubkey::default())
    }

    /// Calculate output amount for DBC bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        // Ensure we have necessary config data
        if self.pool_config.is_none() {
            return 0;
        }
        let pool_config = self.pool_config.as_ref().unwrap();

        // Helper to convert solana_sdk::pubkey::Pubkey to anchor_lang::prelude::Pubkey
        let to_anchor_pubkey = |p: Pubkey| anchor_lang::prelude::Pubkey::from(p.to_bytes());

        // Construct VirtualPool
        let virtual_pool = dynamic_bonding_curve::state::VirtualPool {
            volatility_tracker: if let Some(vt) = &self.volatility_tracker {
                dynamic_bonding_curve::state::fee::VolatilityTracker {
                    last_update_timestamp: vt.last_update_timestamp,
                    padding: vt.padding,
                    sqrt_price_reference: vt.sqrt_price_reference,
                    volatility_accumulator: vt.volatility_accumulator,
                    volatility_reference: vt.volatility_reference,
                }
            } else {
                dynamic_bonding_curve::state::fee::VolatilityTracker::default()
            },
            config: to_anchor_pubkey(self.config),
            creator: to_anchor_pubkey(self.creator),
            base_mint: to_anchor_pubkey(self.base_mint),
            base_vault: to_anchor_pubkey(self.base_vault),
            quote_vault: to_anchor_pubkey(self.quote_vault),
            base_reserve: self.base_reserve,
            quote_reserve: self.quote_reserve,
            protocol_base_fee: self.protocol_base_fee,
            protocol_quote_fee: self.protocol_quote_fee,
            partner_base_fee: self.partner_base_fee,
            partner_quote_fee: self.partner_quote_fee,
            sqrt_price: self.sqrt_price,
            activation_point: self.activation_point,
            pool_type: self.pool_type,
            is_migrated: self.is_migrated,
            is_partner_withdraw_surplus: self.is_partner_withdraw_surplus,
            is_protocol_withdraw_surplus: self.is_protocol_withdraw_surplus,
            migration_progress: self.migration_progress,
            is_withdraw_leftover: self.is_withdraw_leftover,
            is_creator_withdraw_surplus: self.is_creator_withdraw_surplus,
            migration_fee_withdraw_status: self.migration_fee_withdraw_status,
            metrics: dynamic_bonding_curve::state::PoolMetrics::default(),
            finish_curve_timestamp: self.finish_curve_timestamp,
            creator_base_fee: self.creator_base_fee,
            creator_quote_fee: self.creator_quote_fee,
            creation_fee_bits: 0,
            _padding_0: [0; 7],
            _padding_1: [0; 6],
        };

        // Construct PoolConfig
        let sdk_pool_fees = dynamic_bonding_curve::state::PoolFeesConfig {
            base_fee: dynamic_bonding_curve::state::BaseFeeConfig {
                cliff_fee_numerator: pool_config.pool_fees.base_fee.cliff_fee_numerator,
                second_factor: pool_config.pool_fees.base_fee.second_factor,
                third_factor: pool_config.pool_fees.base_fee.third_factor,
                first_factor: pool_config.pool_fees.base_fee.first_factor,
                base_fee_mode: pool_config.pool_fees.base_fee.base_fee_mode,
                padding_0: pool_config.pool_fees.base_fee.padding_0,
            },
            dynamic_fee: dynamic_bonding_curve::state::DynamicFeeConfig {
                initialized: pool_config.pool_fees.dynamic_fee.initialized,
                padding: pool_config.pool_fees.dynamic_fee.padding,
                max_volatility_accumulator: pool_config
                    .pool_fees
                    .dynamic_fee
                    .max_volatility_accumulator,
                variable_fee_control: pool_config.pool_fees.dynamic_fee.variable_fee_control,
                bin_step: pool_config.pool_fees.dynamic_fee.bin_step,
                filter_period: pool_config.pool_fees.dynamic_fee.filter_period,
                decay_period: pool_config.pool_fees.dynamic_fee.decay_period,
                reduction_factor: pool_config.pool_fees.dynamic_fee.reduction_factor,
                padding2: pool_config.pool_fees.dynamic_fee.padding2,
                bin_step_u128: pool_config.pool_fees.dynamic_fee.bin_step_u128,
            },
            padding_0: pool_config.pool_fees.padding_0,
            padding_1: pool_config.pool_fees.padding_1,
            protocol_fee_percent: pool_config.pool_fees.protocol_fee_percent,
            referral_fee_percent: pool_config.pool_fees.referral_fee_percent,
        };

        let mut sdk_curve =
            [dynamic_bonding_curve::state::LiquidityDistributionConfig::default(); 20];
        for (i, point) in pool_config.curve.iter().enumerate() {
            if i >= 20 {
                break;
            }
            sdk_curve[i] = dynamic_bonding_curve::state::LiquidityDistributionConfig {
                sqrt_price: point.sqrt_price,
                liquidity: point.liquidity,
            };
        }

        let sdk_pool_config = dynamic_bonding_curve::state::PoolConfig {
            quote_mint: to_anchor_pubkey(pool_config.quote_mint),
            fee_claimer: to_anchor_pubkey(pool_config.fee_claimer),
            leftover_receiver: to_anchor_pubkey(pool_config.leftover_receiver),
            pool_fees: sdk_pool_fees,
            collect_fee_mode: pool_config.collect_fee_mode,
            migration_option: pool_config.migration_option,
            activation_type: pool_config.activation_type,
            token_decimal: pool_config.token_decimal,
            version: pool_config.version,
            token_type: pool_config.token_type,
            quote_token_flag: pool_config.quote_token_flag,
            partner_locked_lp_percentage: pool_config.partner_locked_lp_percentage,
            partner_lp_percentage: pool_config.partner_lp_percentage,
            creator_locked_lp_percentage: pool_config.creator_locked_lp_percentage,
            creator_lp_percentage: pool_config.creator_lp_percentage,
            migration_fee_option: pool_config.migration_fee_option,
            fixed_token_supply_flag: pool_config.fixed_token_supply_flag,
            creator_trading_fee_percentage: pool_config.creator_trading_fee_percentage,
            token_update_authority: pool_config.token_update_authority,
            migration_fee_percentage: pool_config.migration_fee_percentage,
            creator_migration_fee_percentage: pool_config.creator_migration_fee_percentage,
            _padding_0: pool_config._padding_0,
            swap_base_amount: pool_config.swap_base_amount,
            migration_quote_threshold: pool_config.migration_quote_threshold,
            migration_base_threshold: pool_config.migration_base_threshold,
            migration_sqrt_price: pool_config.migration_sqrt_price,
            locked_vesting_config: dynamic_bonding_curve::state::LockedVestingConfig {
                amount_per_period: pool_config.locked_vesting_config.amount_per_period,
                cliff_duration_from_migration_time: pool_config
                    .locked_vesting_config
                    .cliff_duration_from_migration_time,
                frequency: pool_config.locked_vesting_config.frequency,
                number_of_period: pool_config.locked_vesting_config.number_of_period,
                cliff_unlock_amount: pool_config.locked_vesting_config.cliff_unlock_amount,
                _padding: pool_config.locked_vesting_config._padding,
            },
            pre_migration_token_supply: pool_config.pre_migration_token_supply,
            post_migration_token_supply: pool_config.post_migration_token_supply,
            migrated_collect_fee_mode: pool_config.migrated_collect_fee_mode,
            migrated_dynamic_fee: pool_config.migrated_dynamic_fee,
            migrated_pool_fee_bps: pool_config.migrated_pool_fee_bps,
            _padding_1: pool_config._padding_1,
            _padding_2: pool_config._padding_2,
            sqrt_start_price: pool_config.sqrt_start_price,
            curve: sdk_curve,
        };

        let swap_base_for_quote = *input_token == self.base_mint;

        let current_slot = self.slot;
        let current_timestamp = self.last_updated / 1_000_000;

        let result = dynamic_bonding_curve_sdk::quote_exact_in::quote_exact_in(
            &virtual_pool,
            &sdk_pool_config,
            swap_base_for_quote,
            current_timestamp,
            current_slot,
            input_amount,
            false,
        );

        match result {
            Ok(swap_result) => swap_result.output_amount,
            Err(e) => {
                log::warn!("DBC quote calculation failed: {}", e);
                0
            }
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        if self.quote_reserve == 0 || self.base_reserve == 0 {
            return (0.0, sol_price);
        }

        let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);
        let base_price =
            (self.quote_reserve as f64 / self.base_reserve as f64) * decimal_scale * sol_price;

        (base_price, sol_price)
    }
}
