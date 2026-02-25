// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";

interface IQuoterV1 {
    function quoteExactInputSingle(
        address tokenIn,
        address tokenOut,
        uint24 fee,
        uint256 amountIn,
        uint160 sqrtPriceLimitX96
    ) external returns (uint256 amountOut);
}

interface IQuoterV2 {
    struct QuoteExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint24 fee;
        uint160 sqrtPriceLimitX96;
    }

    function quoteExactInputSingle(
        QuoteExactInputSingleParams memory params
    ) external returns (
        uint256 amountOut,
        uint160 sqrtPriceX96After,
        uint32 initializedTicksCrossed,
        uint256 gasEstimate
    );
}

interface IAlgebraQuoter {
    function quoteExactInputSingle(
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint160 limitSqrtPrice
    ) external returns (
        uint256 amountOut,
        uint160 feeAmount,
        uint160 sqrtPriceX96After,
        uint32 initializedTicksCrossed
    );
}

interface IAlgebraQuoterV2 {
    struct QuoteExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint160 limitSqrtPrice;
    }
    function quoteExactInputSingle(
        QuoteExactInputSingleParams memory params
    ) external returns (
        uint256 amountOut,
        uint160 sqrtPriceX96After,
        uint32 initializedTicksCrossed,
        uint256 gasEstimate
    );
}

contract QuoterProbeTest is Test {
    address constant WETH    = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC    = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;

    address constant UNI_V3_QUOTER_V1 = 0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6;
    address constant UNI_V3_QUOTER_V2 = 0x61fFE014bA17989E743c5F6cB21bF9697530B21e;
    address constant CAMELOT_QUOTER   = 0x0Fc73040b26E9bC8514fA028D998E73A254Fa76E;
    address constant PANCAKE_QUOTER   = 0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997;

    function setUp() public {
        uint256 forkId = vm.createFork("https://arb1.arbitrum.io/rpc");
        vm.selectFork(forkId);
    }

    function test_uniV3_v2() public {
        try IQuoterV2(UNI_V3_QUOTER_V2).quoteExactInputSingle(
            IQuoterV2.QuoteExactInputSingleParams({
                tokenIn: WETH,
                tokenOut: USDC,
                amountIn: 1 ether,
                fee: 500,
                sqrtPriceLimitX96: 0
            })
        ) returns (uint256 amountOut, uint160, uint32, uint256) {
            emit log_named_uint("UniV3 V2 Quoter works! Out", amountOut);
            assertTrue(amountOut > 0);
        } catch (bytes memory reason) {
            emit log_named_bytes("UniV3 V2 reverted", reason);
        }
    }

    function test_pancake_v2() public {
        try IQuoterV2(PANCAKE_QUOTER).quoteExactInputSingle(
            IQuoterV2.QuoteExactInputSingleParams({
                tokenIn: WETH,
                tokenOut: USDC,
                amountIn: 1 ether,
                fee: 500,
                sqrtPriceLimitX96: 0
            })
        ) returns (uint256 amountOut, uint160, uint32, uint256) {
            emit log_named_uint("Pancake V2 Quoter works! Out", amountOut);
            assertTrue(amountOut > 0);
        } catch (bytes memory reason) {
            emit log_named_bytes("Pancake V2 reverted", reason);
        }
    }

    function test_camelot_algebra_struct() public {
        try IAlgebraQuoterV2(CAMELOT_QUOTER).quoteExactInputSingle(
            IAlgebraQuoterV2.QuoteExactInputSingleParams({
                tokenIn: WETH,
                tokenOut: USDC,
                amountIn: 1 ether,
                limitSqrtPrice: 0
            })
        ) returns (uint256 amountOut, uint160, uint32, uint256) {
            emit log_named_uint("Camelot Struct Quoter works! Out", amountOut);
            assertTrue(amountOut > 0);
        } catch (bytes memory reason) {
            emit log_named_bytes("Camelot Struct reverted", reason);
        }
    }
}
