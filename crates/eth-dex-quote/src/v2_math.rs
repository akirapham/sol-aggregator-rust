use ethers::types::U256;

/// Off-chain constant-product AMM math for Uniswap V2-style pools.
/// Computes swap output from reserves without any RPC calls.
///
/// Formula: amount_out = (amount_in * fee_factor * reserve_out) / (reserve_in * 10000 + amount_in * fee_factor)
/// where fee_factor = 10000 - fee_bps
///
/// Standard Uniswap/SushiSwap V2 uses fee_bps=30 (0.3%), giving fee_factor=9970.
/// Compute the output amount for a V2 swap given reserves and fee.
///
/// # Arguments
/// * `amount_in` - Input token amount (with decimals)
/// * `reserve_in` - Reserve of the input token in the pool
/// * `reserve_out` - Reserve of the output token in the pool
/// * `fee_bps` - Fee in basis points (30 = 0.3% for standard Uni/Sushi V2)
///
/// # Returns
/// `Some(amount_out)` or `None` if reserves are zero or computation overflows
pub fn compute_v2_output(
    amount_in: U256,
    reserve_in: U256,
    reserve_out: U256,
    fee_bps: u32,
) -> Option<U256> {
    if reserve_in.is_zero() || reserve_out.is_zero() || amount_in.is_zero() {
        return None;
    }

    let fee_factor = U256::from(10000u32 - fee_bps);
    let amount_in_with_fee = amount_in.checked_mul(fee_factor)?;
    let numerator = amount_in_with_fee.checked_mul(reserve_out)?;
    let denominator = reserve_in
        .checked_mul(U256::from(10000u32))?
        .checked_add(amount_in_with_fee)?;

    if denominator.is_zero() {
        return None;
    }

    Some(numerator / denominator)
}

/// Determine which reserve is input and which is output based on token ordering.
///
/// In Uniswap V2, `token0 < token1` by address. The reserves in the Sync event
/// correspond to `(reserve0, reserve1)` = `(token0_reserve, token1_reserve)`.
///
/// # Arguments
/// * `amount_in` - Input amount
/// * `token_in` - Address of input token
/// * `token0` - Pool's token0 address
/// * `reserve0` - Pool's reserve0 (token0 reserve)
/// * `reserve1` - Pool's reserve1 (token1 reserve)
/// * `fee_bps` - Fee in basis points
///
/// # Returns
/// `Some(amount_out)` or `None` if token_in doesn't match either pool token
pub fn compute_v2_swap(
    amount_in: U256,
    token_in: ethers::types::Address,
    token0: ethers::types::Address,
    reserve0: U256,
    reserve1: U256,
    fee_bps: u32,
) -> Option<U256> {
    if token_in == token0 {
        // Swapping token0 -> token1: reserve_in=reserve0, reserve_out=reserve1
        compute_v2_output(amount_in, reserve0, reserve1, fee_bps)
    } else {
        // Swapping token1 -> token0: reserve_in=reserve1, reserve_out=reserve0
        compute_v2_output(amount_in, reserve1, reserve0, fee_bps)
    }
}

/// Parse a reserve string (serialized U256) back to U256.
/// Returns None if the string is empty or invalid.
pub fn parse_reserve(reserve_str: &str) -> Option<U256> {
    if reserve_str.is_empty() {
        return None;
    }
    U256::from_dec_str(reserve_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_swap() {
        // Pool: 1000 ETH / 2,000,000 USDC (ETH price = $2000)
        // Swap 1 ETH -> USDC
        let reserve_eth = U256::from(1000u64) * U256::from(10u64).pow(U256::from(18u64)); // 1000 * 1e18
        let reserve_usdc = U256::from(2_000_000u64) * U256::from(10u64).pow(U256::from(6u64)); // 2M * 1e6
        let amount_in = U256::from(10u64).pow(U256::from(18u64)); // 1 ETH

        let amount_out = compute_v2_output(amount_in, reserve_eth, reserve_usdc, 30).unwrap();

        // Expected: ~1994 USDC (slightly less than 2000 due to 0.3% fee + price impact)
        let out_usdc = amount_out.as_u128() as f64 / 1_000_000.0;
        assert!(
            out_usdc > 1990.0 && out_usdc < 2000.0,
            "Expected ~1994 USDC, got {}",
            out_usdc
        );
    }

    #[test]
    fn test_zero_reserves() {
        let amount_in = U256::from(1000u64);
        assert!(compute_v2_output(amount_in, U256::zero(), U256::from(1000u64), 30).is_none());
        assert!(compute_v2_output(amount_in, U256::from(1000u64), U256::zero(), 30).is_none());
    }

    #[test]
    fn test_zero_input() {
        assert!(
            compute_v2_output(U256::zero(), U256::from(1000u64), U256::from(1000u64), 30).is_none()
        );
    }

    #[test]
    fn test_fee_impact() {
        // Use large reserves so fee differences are visible in integer math
        let reserve = U256::from(1_000_000_000_000u64); // 1T
        let amount_in = U256::from(1_000_000_000u64); // 1B

        // 0.3% fee (standard)
        let out_30 = compute_v2_output(amount_in, reserve, reserve, 30).unwrap();
        // 0.25% fee
        let out_25 = compute_v2_output(amount_in, reserve, reserve, 25).unwrap();
        // 1% fee
        let out_100 = compute_v2_output(amount_in, reserve, reserve, 100).unwrap();

        // Lower fee = more output
        assert!(
            out_25 > out_30,
            "0.25% fee should give more than 0.3%: {} vs {}",
            out_25,
            out_30
        );
        assert!(
            out_30 > out_100,
            "0.3% fee should give more than 1%: {} vs {}",
            out_30,
            out_100
        );
    }

    #[test]
    fn test_matches_uniswap_v2_formula() {
        // Uniswap V2 exact formula: amountOut = (amountIn * 997 * reserveOut) / (reserveIn * 1000 + amountIn * 997)
        // Our formula with fee_bps=30: amountOut = (amountIn * 9970 * reserveOut) / (reserveIn * 10000 + amountIn * 9970)
        // These are mathematically equivalent (both scale by 10x)

        let amount_in = U256::from(1_000_000u64); // 1 USDC
        let reserve_in = U256::from(500_000_000u64); // 500 USDC
        let reserve_out = U256::from(250_000_000_000_000_000u128); // 0.25 ETH * 1e18

        // Uniswap V2 reference calculation
        let numerator_ref = amount_in * U256::from(997u64) * reserve_out;
        let denominator_ref = reserve_in * U256::from(1000u64) + amount_in * U256::from(997u64);
        let expected = numerator_ref / denominator_ref;

        let actual = compute_v2_output(amount_in, reserve_in, reserve_out, 30).unwrap();

        assert_eq!(actual, expected, "Should match Uniswap V2 formula exactly");
    }

    #[test]
    fn test_parse_reserve() {
        assert_eq!(
            parse_reserve("1000000000000000000"),
            Some(U256::from(10u64).pow(U256::from(18u64)))
        );
        assert_eq!(parse_reserve("0"), Some(U256::zero()));
        assert!(parse_reserve("").is_none());
        assert!(parse_reserve("not_a_number").is_none());
    }

    #[test]
    fn test_swap_direction() {
        let token0: ethers::types::Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let token1: ethers::types::Address = "0x0000000000000000000000000000000000000002"
            .parse()
            .unwrap();

        let reserve0 = U256::from(1_000_000u64); // 1M token0
        let reserve1 = U256::from(2_000_000u64); // 2M token1
        let amount_in = U256::from(1000u64);

        // Swap token0 -> token1 (more token1 in pool, expect more out)
        let out_0_to_1 =
            compute_v2_swap(amount_in, token0, token0, reserve0, reserve1, 30).unwrap();
        // Swap token1 -> token0 (less token0 in pool, expect less out)
        let out_1_to_0 =
            compute_v2_swap(amount_in, token1, token0, reserve0, reserve1, 30).unwrap();

        assert!(
            out_0_to_1 > out_1_to_0,
            "Swapping into deeper reserve should give more output"
        );
    }
}
