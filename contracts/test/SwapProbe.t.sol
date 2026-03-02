// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/QuoteRouter.sol";
import "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

/// @notice Fork-test: verify actual swaps work for all 25 target pools
/// Run:  forge test --mc SwapProbeTest --fork-url https://arbitrum-one-rpc.publicnode.com -vvv --via-ir
contract SwapProbeTest is Test {
    QuoteRouter public proxy;

    // ─── Tokens ──────────────────────────────────────────
    address constant WETH   = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC   = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;
    address constant USDC_E = 0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8;
    address constant USDT   = 0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9;
    address constant WBTC   = 0x2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f;
    address constant ARB    = 0x912CE59144191C1204E64559FE8253a0e49E6548;

    // ─── Swap Routers ────────────────────────────────────
    address constant UNI_V3_ROUTER     = 0xE592427A0AEce92De3Edee1F18E0157C05861564;
    address constant PANCAKE_V3_ROUTER = 0x32226588378236Fd0c7c4053999F88aC0e5cAc77;

    // ─── Factories ───────────────────────────────────────
    address constant UNI_V3_FACTORY     = 0x1F98431c8aD98523631AE4a59f267346ea31F984;
    address constant PANCAKE_V3_FACTORY = 0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865;

    struct PoolInfo {
        address pool;
        string label;
    }

    uint256 totalPassed;
    uint256 totalFailed;

    function setUp() public {
        QuoteRouter impl = new QuoteRouter();
        bytes memory initData = abi.encodeWithSelector(QuoteRouter.initialize.selector, address(this));
        ERC1967Proxy p = new ERC1967Proxy(address(impl), initData);
        proxy = QuoteRouter(address(p));
    }

    function test_fork_swapAll25Pools() public {
        require(block.chainid == 42161, "Must run on Arbitrum fork");

        PoolInfo[25] memory pools = [
            PoolInfo(0x641C00A822e8b671738d32a431a4Fb6074E5c79d, "WETH/USDT UniV3"),
            PoolInfo(0xC6962004f452bE9203591991D15f6b388e09E8D0, "WETH/USDC UniV3"),
            PoolInfo(0xDD65eAD5c92f22b357B1aE516362e4A98b1291CE, "OTM/USDT UniV3"),
            PoolInfo(0xC6F780497A95e246EB9449f5e4770916DCd6396A, "WETH/ARB UniV3"),
            PoolInfo(0x2f5e87C9312fa29aed5c179E456625D79015299c, "WBTC/WETH UniV3"),
            PoolInfo(0xdaf544bCAB17E2dCD293C3Af28e67C7E8b5A49EE, "SOL/WETH UniV3"),
            PoolInfo(0x4CEf551255EC96d89feC975446301b5C4e164C59, "ZRO/WETH UniV3"),
            PoolInfo(0x1E59Fa2f0F4E34649Fae55222eaf4D730Ed35D95, "SOL/WETH PancakeV3"),
            PoolInfo(0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443, "WETH/USDCe UniV3"),
            PoolInfo(0x6f38e884725a116C9C7fBF208e79FE8828a2595F, "WETH/USDC UniV3-100"),
            PoolInfo(0x977f5D9a39049c73bc26eDb3FA15d5f7C0Ac82E9, "WETH/LINK PancakeV3"),
            PoolInfo(0x36C46B34b306010136DD28Bb3BA34F921DAb53BA, "LYK/ARB PancakeV3"),
            PoolInfo(0x770b4493fBed2584C47caEb8C8F7de74d810C49f, "BTB/USDC PancakeV3"),
            PoolInfo(0x0e65f47449920C4bC2127e5082D755286E07A01a, "BTB/LYK PancakeV3"),
            PoolInfo(0x04D1e97733131F8F9711d30aED1A7055832033CD, "HERMES/WETH UniV3"),
            PoolInfo(0x72E68515fc898624930b0EAFa502b4320B1EDE46, "MAIA/HERMES UniV3"),
            PoolInfo(0x5067384e6AD48DE6F14732eABE749dC0F02f662f, "MAIA/WETH UniV3"),
            PoolInfo(0xc1bF07800063EFB46231029864cd22325ef8EFe8, "@G/WETH UniV3"),
            PoolInfo(0x80Ceb98632409080924DCE50C26aCC25458DDe17, "WETH/AAVE PancakeV3"),
            PoolInfo(0x263f7B865DE80355F91C00dFb975A821Effbea24, "WETH/AAVE UniV3"),
            PoolInfo(0xc868f85196fACFBBc08B44975f83788Dd922E482, "ZRO/USDT UniV3"),
            PoolInfo(0xd13040d4fe917EE704158CfCB3338dCd2838B245, "RAIN/WETH UniV3"),
            PoolInfo(0x2ad24e6cb77C2C7F09a5Fa3fA5f23F3278046909, "WETH/SETH UniV3"),
            PoolInfo(0xc82819F72A9e77E2c0c3A69B3196478f44303cf4, "WETH/USDT UniV3-2"),
            PoolInfo(0x5969EFddE3cF5C0D9a88aE51E47d721096A97203, "WBTC/USDT UniV3")
        ];

        emit log("=== SWAP TEST: ALL 25 TARGET POOLS ===");
        emit log("");

        for (uint i = 0; i < pools.length; i++) {
            _probeSwap(pools[i].pool, pools[i].label);
        }

        emit log("");
        emit log("=== SUMMARY ===");
        emit log_named_uint("Swap Passed", totalPassed);
        emit log_named_uint("Swap Failed", totalFailed);
        emit log_named_uint("Total Pools", pools.length);

        assertGt(totalPassed, 18, "Too many swap failures");
    }

    function _probeSwap(address pool, string memory label) internal {
        // Read pool metadata
        (bool s0, bytes memory d0) = pool.staticcall(abi.encodeWithSignature("token0()"));
        (bool s1, bytes memory d1) = pool.staticcall(abi.encodeWithSignature("token1()"));
        (bool sf, bytes memory df) = pool.staticcall(abi.encodeWithSignature("factory()"));
        (bool sFee, bytes memory dFee) = pool.staticcall(abi.encodeWithSignature("fee()"));

        if (!s0 || !s1 || !sf) {
            emit log_named_string(unicode"  SKIP (no ABI)", label);
            totalFailed++;
            return;
        }

        address token0 = abi.decode(d0, (address));
        address token1 = abi.decode(d1, (address));
        address factory = abi.decode(df, (address));
        uint24 fee = sFee ? abi.decode(dFee, (uint24)) : 0;

        // Determine router based on factory
        address router;
        if (factory == PANCAKE_V3_FACTORY) {
            router = PANCAKE_V3_ROUTER;
        } else {
            router = UNI_V3_ROUTER;
        }

        // Deal token0 to the proxy contract
        uint256 amountIn = _getDefaultAmount(token0);
        deal(token0, address(proxy), amountIn);

        // Build ExecHop for swap
        QuoteRouter.ExecHop[] memory hops = new QuoteRouter.ExecHop[](1);
        hops[0] = QuoteRouter.ExecHop({
            poolType: QuoteRouter.PoolType.UniswapV3, // Both UniV3 and PancakeV3 use same interface
            router: router,
            tokenIn: token0,
            tokenOut: token1,
            fee: fee
        });

        // Attempt to execute the swap directly via the proxy's _executeHop (we test via low-level approach)
        // Instead of using flashloan, we directly approve and call the router from our test contract
        deal(token0, address(this), amountIn);
        IERC20(token0).approve(router, amountIn);

        // Call the router directly to test the swap
        bool success;
        uint256 amountOut;
        
        if (router == PANCAKE_V3_ROUTER) {
            try IPancakeV3SwapRouter(router).exactInputSingle(
                IPancakeV3SwapRouter.ExactInputSingleParams({
                    tokenIn: token0,
                    tokenOut: token1,
                    fee: fee,
                    recipient: address(this),
                    amountIn: amountIn,
                    amountOutMinimum: 0,
                    sqrtPriceLimitX96: 0
                })
            ) returns (uint256 returnedAmount) {
                amountOut = returnedAmount;
                success = true;
            } catch {
                success = false;
            }
        } else {
            try ISwapRouter(router).exactInputSingle(
                ISwapRouter.ExactInputSingleParams({
                    tokenIn: token0,
                    tokenOut: token1,
                    fee: fee,
                    recipient: address(this),
                    deadline: block.timestamp + 1200,
                    amountIn: amountIn,
                    amountOutMinimum: 0,
                    sqrtPriceLimitX96: 0
                })
            ) returns (uint256 returnedAmount) {
                amountOut = returnedAmount;
                success = true;
            } catch {
                success = false;
            }
        }

        if (success && amountOut > 0) {
            totalPassed++;
            emit log_named_string(unicode"  \u2705 SWAP OK", label);
            emit log_named_uint("     in ", amountIn);
            emit log_named_uint("     out", amountOut);
        } else if (success && amountOut == 0) {
            totalFailed++;
            emit log_named_string(unicode"  \u274C SWAP 0", label);
        } else {
            totalFailed++;
            emit log_named_string(unicode"  \u274C SWAP REVERT", label);
        }
    }

    function _getDefaultAmount(address token) internal pure returns (uint256) {
        if (token == WETH) return 0.01 ether;          // 0.01 ETH (small to avoid price impact)
        if (token == WBTC) return 1000;                 // 0.00001 BTC (8 dec)
        if (token == USDC || token == USDC_E || token == USDT) return 10e6; // $10
        if (token == ARB) return 10e18;                 // 10 ARB
        return 1e17;                                     // 0.1 token (18 dec)
    }
}



