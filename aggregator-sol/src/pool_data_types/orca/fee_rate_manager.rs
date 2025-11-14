use crate::pool_data_types::orca::{
    math::{ceil_division_u128, ceil_division_u32, floor_division, sqrt_price_from_tick_index},
    state::{
        AdaptiveFeeConstants, AdaptiveFeeInfo, AdaptiveFeeVariables,
        ADAPTIVE_FEE_CONTROL_FACTOR_DENOMINATOR, MAX_TICK_INDEX, MIN_TICK_INDEX,
        VOLATILITY_ACCUMULATOR_SCALE_FACTOR,
    },
};
use anchor_lang::prelude::*;

// max fee rate should be controlled by max_volatility_accumulator, so this is a hard limit for safety.
// Fee rate is represented as hundredths of a basis point.
pub const FEE_RATE_HARD_LIMIT: u32 = 100_000; // 10%

#[derive(Debug)]
pub enum FeeRateManager {
    Adaptive {
        a_to_b: bool,
        tick_group_index: i32,
        static_fee_rate: u16,
        adaptive_fee_constants: AdaptiveFeeConstants,
        adaptive_fee_variables: AdaptiveFeeVariables,
        core_tick_group_range_lower_bound: Option<(i32, u128)>,
        core_tick_group_range_upper_bound: Option<(i32, u128)>,
    },
    Static {
        static_fee_rate: u16,
    },
}

impl FeeRateManager {
    pub fn new(
        a_to_b: bool,
        current_tick_index: i32,
        timestamp: u64,
        static_fee_rate: u16,
        adaptive_fee_info: &Option<AdaptiveFeeInfo>,
    ) -> Result<Self> {
        match adaptive_fee_info {
            None => Ok(Self::Static { static_fee_rate }),
            Some(adaptive_fee_info) => {
                let tick_group_index = floor_division(
                    current_tick_index,
                    adaptive_fee_info.constants.tick_group_size as i32,
                );
                let adaptive_fee_constants = adaptive_fee_info.constants;
                let mut adaptive_fee_variables = adaptive_fee_info.variables;

                // update reference at the initialization of the fee rate manager
                adaptive_fee_variables.update_reference(
                    tick_group_index,
                    timestamp,
                    &adaptive_fee_constants,
                )?;

                // max_volatility_accumulator < volatility_reference + tick_group_index_delta * VOLATILITY_ACCUMULATOR_SCALE_FACTOR
                // -> ceil((max_volatility_accumulator - volatility_reference) / VOLATILITY_ACCUMULATOR_SCALE_FACTOR) < tick_group_index_delta
                // From the above, if tick_group_index_delta is sufficiently large, volatility_accumulator always sticks to max_volatility_accumulator
                let max_volatility_accumulator_tick_group_index_delta = ceil_division_u32(
                    adaptive_fee_constants.max_volatility_accumulator
                        - adaptive_fee_variables.volatility_reference,
                    VOLATILITY_ACCUMULATOR_SCALE_FACTOR as u32,
                );

                // we need to calculate the adaptive fee rate for each tick_group_index in the range of core tick group
                let core_tick_group_range_lower_index = adaptive_fee_variables
                    .tick_group_index_reference
                    - max_volatility_accumulator_tick_group_index_delta as i32;
                let core_tick_group_range_upper_index = adaptive_fee_variables
                    .tick_group_index_reference
                    + max_volatility_accumulator_tick_group_index_delta as i32;
                let core_tick_group_range_lower_bound_tick_index = core_tick_group_range_lower_index
                    * adaptive_fee_constants.tick_group_size as i32;
                let core_tick_group_range_upper_bound_tick_index = core_tick_group_range_upper_index
                    * adaptive_fee_constants.tick_group_size as i32
                    + adaptive_fee_constants.tick_group_size as i32;

                let core_tick_group_range_lower_bound =
                    if core_tick_group_range_lower_bound_tick_index > MIN_TICK_INDEX {
                        Some((
                            core_tick_group_range_lower_index,
                            sqrt_price_from_tick_index(
                                core_tick_group_range_lower_bound_tick_index,
                            ),
                        ))
                    } else {
                        None
                    };
                let core_tick_group_range_upper_bound =
                    if core_tick_group_range_upper_bound_tick_index < MAX_TICK_INDEX {
                        Some((
                            core_tick_group_range_upper_index,
                            sqrt_price_from_tick_index(
                                core_tick_group_range_upper_bound_tick_index,
                            ),
                        ))
                    } else {
                        None
                    };

                // Note: reduction uses the value of volatility_accumulator, but update_reference does not update it.
                //       update_volatility_accumulator is always called if the swap loop is executed at least once,
                //       amount == 0 and sqrt_price_limit == whirlpool.sqrt_price are rejected, so the loop is guaranteed to run at least once.

                Ok(Self::Adaptive {
                    a_to_b,
                    tick_group_index,
                    static_fee_rate,
                    adaptive_fee_constants,
                    adaptive_fee_variables,
                    core_tick_group_range_lower_bound,
                    core_tick_group_range_upper_bound,
                })
            }
        }
    }

    pub fn update_volatility_accumulator(&mut self) -> Result<()> {
        match self {
            Self::Static { .. } => Ok(()),
            Self::Adaptive {
                tick_group_index,
                adaptive_fee_constants,
                adaptive_fee_variables,
                ..
            } => adaptive_fee_variables
                .update_volatility_accumulator(*tick_group_index, adaptive_fee_constants),
        }
    }

    pub fn get_total_fee_rate(&self) -> u32 {
        match self {
            Self::Static { static_fee_rate } => *static_fee_rate as u32,
            Self::Adaptive {
                static_fee_rate,
                adaptive_fee_constants,
                adaptive_fee_variables,
                ..
            } => {
                let adaptive_fee_rate =
                    Self::compute_adaptive_fee_rate(adaptive_fee_constants, adaptive_fee_variables);
                let total_fee_rate = *static_fee_rate as u32 + adaptive_fee_rate;

                if total_fee_rate > FEE_RATE_HARD_LIMIT {
                    FEE_RATE_HARD_LIMIT
                } else {
                    total_fee_rate
                }
            }
        }
    }

    // returns (bounded_sqrt_price, skip)
    // skip is true if the step-by-step calculation of adaptive fee is meaningless.
    //
    // When skip is true, we need to call advance_tick_group_after_skip() instead of advance_tick_group().
    pub fn get_bounded_sqrt_price_target(
        &self,
        sqrt_price: u128,
        curr_liquidity: u128,
    ) -> (u128, bool) {
        match self {
            Self::Static { .. } => (sqrt_price, false),
            Self::Adaptive {
                a_to_b,
                tick_group_index,
                adaptive_fee_constants,
                core_tick_group_range_lower_bound,
                core_tick_group_range_upper_bound,
                ..
            } => {
                // If the adaptive fee control factor is 0, the adaptive fee is not applied,
                // and the step-by-step calculation of adaptive fee is meaningless.
                if adaptive_fee_constants.adaptive_fee_control_factor == 0 {
                    return (sqrt_price, true);
                }

                // If the liquidity is 0, obviously no trades occur,
                // and the step-by-step calculation of adaptive fee is meaningless.
                if curr_liquidity == 0 {
                    return (sqrt_price, true);
                }

                // If the tick group index is out of the core tick group range (lower side),
                // the range where volatility_accumulator is always max_volatility_accumulator can be skipped.
                if let Some((lower_tick_group_index, lower_tick_group_bound_sqrt_price)) =
                    core_tick_group_range_lower_bound
                {
                    if *tick_group_index < *lower_tick_group_index {
                        if *a_to_b {
                            // <<-- swap direction -- <current tick group index> | core range |
                            return (sqrt_price, true);
                        } else {
                            // <current tick group index> -- swap direction -->> | core range |
                            return (sqrt_price.min(*lower_tick_group_bound_sqrt_price), true);
                        }
                    }
                }

                // If the tick group index is out of the core tick group range (upper side)
                // the range where volatility_accumulator is always max_volatility_accumulator can be skipped.
                if let Some((upper_tick_group_index, upper_tick_group_bound_sqrt_price)) =
                    core_tick_group_range_upper_bound
                {
                    if *tick_group_index > *upper_tick_group_index {
                        if *a_to_b {
                            // | core range | <<-- swap direction -- <current tick group index>
                            return (sqrt_price.max(*upper_tick_group_bound_sqrt_price), true);
                        } else {
                            // | core range | <current tick group index> -- swap direction -->>
                            return (sqrt_price, true);
                        }
                    }
                }

                let boundary_tick_index = if *a_to_b {
                    *tick_group_index * adaptive_fee_constants.tick_group_size as i32
                } else {
                    *tick_group_index * adaptive_fee_constants.tick_group_size as i32
                        + adaptive_fee_constants.tick_group_size as i32
                };

                let boundary_sqrt_price = sqrt_price_from_tick_index(
                    boundary_tick_index.clamp(MIN_TICK_INDEX, MAX_TICK_INDEX),
                );

                if *a_to_b {
                    (sqrt_price.max(boundary_sqrt_price), false)
                } else {
                    (sqrt_price.min(boundary_sqrt_price), false)
                }
            }
        }
    }

    pub fn get_next_adaptive_fee_info(&self) -> Option<AdaptiveFeeInfo> {
        match self {
            Self::Static { .. } => None,
            Self::Adaptive {
                adaptive_fee_constants,
                adaptive_fee_variables,
                ..
            } => Some(AdaptiveFeeInfo {
                constants: *adaptive_fee_constants,
                variables: *adaptive_fee_variables,
            }),
        }
    }

    fn compute_adaptive_fee_rate(
        adaptive_fee_constants: &AdaptiveFeeConstants,
        adaptive_fee_variables: &AdaptiveFeeVariables,
    ) -> u32 {
        let crossed = adaptive_fee_variables.volatility_accumulator
            * adaptive_fee_constants.tick_group_size as u32;

        let squared = u64::from(crossed) * u64::from(crossed);

        let fee_rate = ceil_division_u128(
            u128::from(adaptive_fee_constants.adaptive_fee_control_factor) * u128::from(squared),
            u128::from(ADAPTIVE_FEE_CONTROL_FACTOR_DENOMINATOR)
                * u128::from(VOLATILITY_ACCUMULATOR_SCALE_FACTOR)
                * u128::from(VOLATILITY_ACCUMULATOR_SCALE_FACTOR),
        );

        if fee_rate > FEE_RATE_HARD_LIMIT as u128 {
            FEE_RATE_HARD_LIMIT
        } else {
            fee_rate as u32
        }
    }
}
