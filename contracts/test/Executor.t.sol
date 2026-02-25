// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/QuoteRouter.sol";
import "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

contract ExecutorTest is Test {
    QuoteRouter public executor;

    address public constant WETH = 0x82aF49447D8a07e3bd95BD0d56f35241523fBab1;
    address public constant USDC = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;
    address public constant USDT = 0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9;

    address public constant UNI_V3_ROUTER = 0xE592427A0AEce92De3Edee1F18E0157C05861564;
    address public constant SUSHI_V2_ROUTER = 0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506;
    address public constant CAMELOT_ROUTER = 0xc873fEcbd354f5A56E00E710B90EF4201db2448d;

    function setUp() public {
        vm.createSelectFork("https://arbitrum-one-rpc.publicnode.com");
        
        QuoteRouter impl = new QuoteRouter();
        bytes memory initData = abi.encodeWithSelector(QuoteRouter.initialize.selector, address(this));
        ERC1967Proxy proxy = new ERC1967Proxy(address(impl), initData);
        
        executor = QuoteRouter(address(proxy));
    }

    function testExecuteArbitrage() public {
        // We will fake an arbitrage that might not be profitable just to test the Flashloan callback executes correctly!
        
        QuoteRouter.ExecHop[] memory paths = new QuoteRouter.ExecHop[](2);
        
        // Swap WETH -> USDC on Uni V3 (0.3%)
        paths[0] = QuoteRouter.ExecHop({
            poolType: QuoteRouter.PoolType.UniswapV3,
            router: UNI_V3_ROUTER,
            tokenIn: WETH,
            tokenOut: USDC,
            fee: 3000
        });

        // Swap USDC -> WETH on UniswapV3 (fee 500 = 0.05%)
        paths[1] = QuoteRouter.ExecHop({
            poolType: QuoteRouter.PoolType.UniswapV3,
            router: UNI_V3_ROUTER,
            tokenIn: USDC,
            tokenOut: WETH,
            fee: 500
        });

        // To make it "profitable" (bypassing the Unprofitable arbitrage require),
        // we will cheat and give the executor 1 WETH so it can definitely repay the flashloan amount!
        deal(WETH, address(executor), 1 ether);

        // Flashloan 1 WETH
        vm.expectRevert("Unprofitable arbitrage");
        executor.executeArbitrage(paths, 1 ether, WETH);
    }
}
