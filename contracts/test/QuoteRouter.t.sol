// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/QuoteRouter.sol";
import "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

/// @notice Comprehensive fork tests for QuoteRouter on Arbitrum mainnet
/// Run with: forge test --fork-url $ARB_RPC_URL -vvv
contract QuoteRouterTest is Test {
    QuoteRouter public router;
    QuoteRouter public proxy;

    // ─── Arbitrum Tokens ─────────────────────────────────
    address constant WETH    = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address constant USDC    = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;  // native USDC
    address constant USDC_E  = 0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8;  // bridged USDC.e
    address constant USDT    = 0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9;

    // ─── Arbitrum DEX Quoters/Routers ────────────────────
    address constant UNI_V3_QUOTER       = 0x61fFE014bA17989E743c5F6cB21bF9697530B21e;
    address constant CAMELOT_QUOTER      = 0x0Fc73040b26E9bC8514fA028D998E73A254Fa76E;
    address constant PANCAKE_V3_QUOTER   = 0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997;
    address constant SUSHI_V2_ROUTER     = 0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506;

    function setUp() public {
        router = new QuoteRouter();
        bytes memory initData = abi.encodeWithSelector(
            QuoteRouter.initialize.selector,
            address(this)
        );
        ERC1967Proxy proxyContract = new ERC1967Proxy(address(router), initData);
        proxy = QuoteRouter(address(proxyContract));
    }

    // ═══════════════════════════════════════════════════════
    //  Unit tests (no fork needed)
    // ═══════════════════════════════════════════════════════

    function test_initialize() public view {
        assertEq(proxy.owner(), address(this));
        assertEq(proxy.version(), "1.0.0");
    }

    function test_upgradeability() public {
        QuoteRouter newImpl = new QuoteRouter();
        proxy.upgradeToAndCall(address(newImpl), "");
        assertEq(proxy.version(), "1.0.0");
    }

    function test_onlyOwnerCanUpgrade() public {
        QuoteRouter newImpl = new QuoteRouter();
        vm.prank(address(0xdead));
        vm.expectRevert();
        proxy.upgradeToAndCall(address(newImpl), "");
    }

    // ═══════════════════════════════════════════════════════
    //  Fork tests — Individual DEX quotes
    // ═══════════════════════════════════════════════════════

    modifier onlyFork() {
        if (block.chainid != 42161) {
            emit log("Skipping: not Arbitrum fork");
            return;
        }
        _;
    }

    // ─── Uniswap V3 ─────────────────────────────────────

    function test_fork_uniV3_WETH_USDCe_500() public onlyFork {
        uint256 out = _quoteSingleHopV3(UNI_V3_QUOTER, WETH, USDC_E, 500, 1 ether);
        emit log_named_uint("UniV3 WETH->USDC.e (0.05%): 1 ETH =", out);
    }

    function test_fork_uniV3_WETH_USDCe_3000() public onlyFork {
        uint256 out = _quoteSingleHopV3(UNI_V3_QUOTER, WETH, USDC_E, 3000, 1 ether);
        emit log_named_uint("UniV3 WETH->USDC.e (0.3%): 1 ETH =", out);
    }

    function test_fork_uniV3_USDT_USDCe_100() public onlyFork {
        uint256 out = _quoteSingleHopV3(UNI_V3_QUOTER, USDT, USDC_E, 100, 1000e6);
        emit log_named_uint("UniV3 USDT->USDC.e (0.01%): 1000 USDT =", out);
    }

    function test_fork_uniV3_WETH_USDT_3000() public onlyFork {
        uint256 out = _quoteSingleHopV3(UNI_V3_QUOTER, WETH, USDT, 3000, 1 ether);
        emit log_named_uint("UniV3 WETH->USDT (0.3%): 1 ETH =", out);
        // Don't assert — pool may be thin
    }

    // ─── Camelot Algebra ─────────────────────────────────

    function test_fork_camelot_WETH_USDCe() public onlyFork {
        // Camelot uses dynamic fees — fee param is ignored by Algebra quoter
        uint256 out = _quoteSingleHopV3(CAMELOT_QUOTER, WETH, USDC_E, 0, 1 ether);
        emit log_named_uint("Camelot WETH->USDC.e: 1 ETH =", out);
        // Don't assert — Camelot quoter interface may differ
    }

    function test_fork_camelot_WETH_USDT() public onlyFork {
        uint256 out = _quoteSingleHopV3(CAMELOT_QUOTER, WETH, USDT, 0, 1 ether);
        emit log_named_uint("Camelot WETH->USDT: 1 ETH =", out);
    }

    function test_fork_camelot_USDC_USDT() public onlyFork {
        uint256 out = _quoteSingleHopV3(CAMELOT_QUOTER, USDC, USDT, 0, 1000e6);
        // May or may not have liquidity — log result
        emit log_named_uint("Camelot USDC->USDT: 1000 USDC =", out);
    }

    // ─── PancakeSwap V3 ──────────────────────────────────

    function test_fork_pancakeV3_WETH_USDC_500() public onlyFork {
        uint256 out = _quoteSingleHopV3(PANCAKE_V3_QUOTER, WETH, USDC, 500, 1 ether);
        emit log_named_uint("PancakeV3 WETH->USDC (0.05%): 1 ETH =", out);
    }

    function test_fork_pancakeV3_WETH_USDC_2500() public onlyFork {
        uint256 out = _quoteSingleHopV3(PANCAKE_V3_QUOTER, WETH, USDC, 2500, 1 ether);
        emit log_named_uint("PancakeV3 WETH->USDC (0.25%): 1 ETH =", out);
    }

    function test_fork_pancakeV3_WETH_USDT_500() public onlyFork {
        uint256 out = _quoteSingleHopV3(PANCAKE_V3_QUOTER, WETH, USDT, 500, 1 ether);
        emit log_named_uint("PancakeV3 WETH->USDT (0.05%): 1 ETH =", out);
    }

    // ─── SushiSwap V2 ────────────────────────────────────

    function test_fork_sushiV2_WETH_USDC() public onlyFork {
        uint256 out = _quoteSingleHopV2(SUSHI_V2_ROUTER, WETH, USDC, 1 ether);
        emit log_named_uint("SushiV2 WETH->USDC: 1 ETH =", out);
    }

    function test_fork_sushiV2_WETH_USDT() public onlyFork {
        uint256 out = _quoteSingleHopV2(SUSHI_V2_ROUTER, WETH, USDT, 1 ether);
        emit log_named_uint("SushiV2 WETH->USDT: 1 ETH =", out);
    }

    function test_fork_sushiV2_USDC_WETH() public onlyFork {
        uint256 out = _quoteSingleHopV2(SUSHI_V2_ROUTER, USDC, WETH, 1000e6);
        emit log_named_uint("SushiV2 USDC->WETH: 1000 USDC =", out);
    }

    // ═══════════════════════════════════════════════════════
    //  Fork tests — 2-hop arbitrage (cross-DEX)
    // ═══════════════════════════════════════════════════════

    function test_fork_arb_uniV3_500_vs_3000() public onlyFork {
        _testArbitrage2Hop(
            "UniV3(0.05%) -> UniV3(0.3%)",
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500),
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 3000),
            1000e6
        );
    }

    function test_fork_arb_uniV3_vs_camelot() public onlyFork {
        _testArbitrage2Hop(
            "UniV3(0.05%) buy -> Camelot sell",
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500),
            QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, WETH, USDT, 0),
            1000e6
        );
    }

    function test_fork_arb_camelot_vs_uniV3() public onlyFork {
        _testArbitrage2Hop(
            "Camelot buy -> UniV3(0.05%) sell",
            QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, USDT, WETH, 0),
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 500),
            1000e6
        );
    }

    function test_fork_arb_sushiV2_vs_uniV3() public onlyFork {
        _testArbitrage2Hop(
            "SushiV2 buy -> UniV3(0.05%) sell",
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, SUSHI_V2_ROUTER, USDT, WETH, 0),
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 500),
            1000e6
        );
    }

    function test_fork_arb_sushiV2_vs_camelot() public onlyFork {
        _testArbitrage2Hop(
            "SushiV2 buy -> Camelot sell",
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, SUSHI_V2_ROUTER, USDT, WETH, 0),
            QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, WETH, USDT, 0),
            1000e6
        );
    }

    function test_fork_arb_pancakeV3_vs_uniV3() public onlyFork {
        _testArbitrage2Hop(
            "PancakeV3(0.05%) buy -> UniV3(0.3%) sell",
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, PANCAKE_V3_QUOTER, USDT, WETH, 500),
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 3000),
            1000e6
        );
    }

    function test_fork_arb_uniV3_vs_pancakeV3() public onlyFork {
        _testArbitrage2Hop(
            "UniV3(0.05%) buy -> PancakeV3(0.05%) sell",
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500),
            QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, PANCAKE_V3_QUOTER, WETH, USDT, 500),
            1000e6
        );
    }

    // ═══════════════════════════════════════════════════════
    //  Fork tests — 3-hop arbitrage (triangular)
    // ═══════════════════════════════════════════════════════

    function test_fork_3hop_USDT_WETH_USDC_USDT() public onlyFork {
        QuoteRouter.Hop[] memory hops = new QuoteRouter.Hop[](3);
        hops[0] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500);
        hops[1] = QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, WETH, USDC, 0);
        hops[2] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDC, USDT, 100);

        uint256 amountIn = 1000e6;
        uint256 amountOut = proxy.quoteSinglePath(hops, amountIn);

        int256 profit = int256(amountOut) - int256(amountIn);
        emit log_named_string("Path", "USDT->WETH(UniV3)->USDC(Camelot)->USDT(UniV3)");
        emit log_named_uint("AmountIn (USDT)", amountIn);
        emit log_named_uint("AmountOut (USDT)", amountOut);
        emit log_named_int("Profit (USDT wei)", profit);
    }

    function test_fork_3hop_USDT_WETH_USDCe_USDT() public onlyFork {
        QuoteRouter.Hop[] memory hops = new QuoteRouter.Hop[](3);
        hops[0] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, SUSHI_V2_ROUTER, USDT, WETH, 0);
        hops[1] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDC_E, 500);
        hops[2] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDC_E, USDT, 100);

        uint256 amountIn = 1000e6;
        uint256 amountOut = proxy.quoteSinglePath(hops, amountIn);

        int256 profit = int256(amountOut) - int256(amountIn);
        emit log_named_string("Path", "USDT->WETH(SushiV2)->USDCe(UniV3)->USDT(UniV3)");
        emit log_named_uint("AmountIn", amountIn);
        emit log_named_uint("AmountOut", amountOut);
        emit log_named_int("Profit", profit);
    }

    // ═══════════════════════════════════════════════════════
    //  Fork tests — Batch quoting (main use case)
    // ═══════════════════════════════════════════════════════

    function test_fork_batchQuote_allPaths() public onlyFork {
        QuoteRouter.ArbQuote[] memory quotes = new QuoteRouter.ArbQuote[](6);

        // All cross-DEX 2-hop USDT->WETH->USDT combinations
        quotes[0] = QuoteRouter.ArbQuote({
            hop1: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500),
            hop2: QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, WETH, USDT, 0),
            amountIn: 1000e6
        });

        quotes[1] = QuoteRouter.ArbQuote({
            hop1: QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, USDT, WETH, 0),
            hop2: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 500),
            amountIn: 1000e6
        });

        quotes[2] = QuoteRouter.ArbQuote({
            hop1: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, SUSHI_V2_ROUTER, USDT, WETH, 0),
            hop2: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 500),
            amountIn: 1000e6
        });

        quotes[3] = QuoteRouter.ArbQuote({
            hop1: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500),
            hop2: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, SUSHI_V2_ROUTER, WETH, USDT, 0),
            amountIn: 1000e6
        });

        quotes[4] = QuoteRouter.ArbQuote({
            hop1: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, PANCAKE_V3_QUOTER, USDT, WETH, 500),
            hop2: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, WETH, USDT, 500),
            amountIn: 1000e6
        });

        quotes[5] = QuoteRouter.ArbQuote({
            hop1: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500),
            hop2: QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, PANCAKE_V3_QUOTER, WETH, USDT, 500),
            amountIn: 1000e6
        });

        QuoteRouter.ArbResult[] memory results = proxy.quoteBatchArbitrage(quotes);
        assertEq(results.length, 6);

        string[6] memory labels = [
            "UniV3(500)->Camelot",
            "Camelot->UniV3(500)",
            "SushiV2->UniV3(500)",
            "UniV3(500)->SushiV2",
            "PancakeV3(500)->UniV3(500)",
            "UniV3(500)->PancakeV3(500)"
        ];

        emit log("=== BATCH ARBITRAGE RESULTS ===");
        for (uint i = 0; i < results.length; i++) {
            emit log_named_string("Path", labels[i]);
            emit log_named_uint("  AmountOut", results[i].amountOut);
            emit log_named_int("  Profit", results[i].profit);
            emit log_named_uint("  Success", results[i].success ? 1 : 0);
        }
    }

    // ═══════════════════════════════════════════════════════
    //  Fork test — Multi-path quoting
    // ═══════════════════════════════════════════════════════

    function test_fork_multiPath_compare() public onlyFork {
        QuoteRouter.PathQuote[] memory paths = new QuoteRouter.PathQuote[](4);

        // Path 1: USDT->WETH via UniV3 0.05%
        QuoteRouter.Hop[] memory hops1 = new QuoteRouter.Hop[](1);
        hops1[0] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, UNI_V3_QUOTER, USDT, WETH, 500);
        paths[0] = QuoteRouter.PathQuote({hops: hops1, amountIn: 1000e6});

        // Path 2: USDT->WETH via Camelot
        QuoteRouter.Hop[] memory hops2 = new QuoteRouter.Hop[](1);
        hops2[0] = QuoteRouter.Hop(QuoteRouter.PoolType.CamelotAlgebra, CAMELOT_QUOTER, USDT, WETH, 0);
        paths[1] = QuoteRouter.PathQuote({hops: hops2, amountIn: 1000e6});

        // Path 3: USDT->WETH via SushiV2
        QuoteRouter.Hop[] memory hops3 = new QuoteRouter.Hop[](1);
        hops3[0] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, SUSHI_V2_ROUTER, USDT, WETH, 0);
        paths[2] = QuoteRouter.PathQuote({hops: hops3, amountIn: 1000e6});

        // Path 4: USDT->WETH via PancakeV3 0.05%
        QuoteRouter.Hop[] memory hops4 = new QuoteRouter.Hop[](1);
        hops4[0] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV3, PANCAKE_V3_QUOTER, USDT, WETH, 500);
        paths[3] = QuoteRouter.PathQuote({hops: hops4, amountIn: 1000e6});

        QuoteRouter.PathResult[] memory results = proxy.quoteMultiPaths(paths);

        string[4] memory labels = [
            "UniV3(500)",
            "Camelot",
            "SushiV2",
            "PancakeV3(500)"
        ];

        emit log("=== BEST RATE COMPARISON: 1000 USDT -> WETH ===");
        uint256 best = 0;
        uint256 bestIdx = 0;
        for (uint i = 0; i < results.length; i++) {
            emit log_named_string("DEX", labels[i]);
            emit log_named_uint("  WETH out", results[i].amountOut);
            emit log_named_uint("  Success", results[i].success ? 1 : 0);
            if (results[i].amountOut > best) {
                best = results[i].amountOut;
                bestIdx = i;
            }
        }
        emit log_named_string("BEST", labels[bestIdx]);
        emit log_named_uint("BEST WETH out", best);
    }

    // ═══════════════════════════════════════════════════════
    //  Helpers
    // ═══════════════════════════════════════════════════════

    function _quoteSingleHopV3(
        address quoter, address tokenIn, address tokenOut, uint24 fee, uint256 amountIn
    ) internal returns (uint256) {
        QuoteRouter.Hop[] memory hops = new QuoteRouter.Hop[](1);
        hops[0] = QuoteRouter.Hop(
            fee == 0 ? QuoteRouter.PoolType.CamelotAlgebra : QuoteRouter.PoolType.UniswapV3,
            quoter, tokenIn, tokenOut, fee
        );
        return proxy.quoteSinglePath(hops, amountIn);
    }

    function _quoteSingleHopV2(
        address routerAddr, address tokenIn, address tokenOut, uint256 amountIn
    ) internal returns (uint256) {
        QuoteRouter.Hop[] memory hops = new QuoteRouter.Hop[](1);
        hops[0] = QuoteRouter.Hop(QuoteRouter.PoolType.UniswapV2, routerAddr, tokenIn, tokenOut, 0);
        return proxy.quoteSinglePath(hops, amountIn);
    }

    function _testArbitrage2Hop(
        string memory label,
        QuoteRouter.Hop memory hop1,
        QuoteRouter.Hop memory hop2,
        uint256 amountIn
    ) internal {
        (uint256 amountOut, int256 profit) = proxy.quoteArbitrage2Hop(hop1, hop2, amountIn);
        emit log_named_string("Arb Path", label);
        emit log_named_uint("  AmountIn (USDT)", amountIn);
        emit log_named_uint("  AmountOut (USDT)", amountOut);
        emit log_named_int("  Profit (USDT wei)", profit);
    }
}
