// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";

/// @title QuoteRouter - UUPS upgradeable multi-path quoter
/// @notice Chains V2/V3 quotes internally for multi-hop arbitrage estimation in a single eth_call
/// @dev Upgradeable via UUPS so we can add executor logic later without redeploying
contract QuoteRouter is UUPSUpgradeable, OwnableUpgradeable {

    // ─── Pool type enum ──────────────────────────────────
    enum PoolType {
        UniswapV2,      // getAmountsOut via router
        UniswapV3,      // quoteExactInputSingle via quoter
        CamelotAlgebra  // quoteExactInputSingle via Algebra quoter (same interface as V3)
    }

    // ─── Structs ─────────────────────────────────────────

    /// @notice A single hop in a swap path
    struct Hop {
        PoolType poolType;
        address router;      // V2: router address, V3/Algebra: quoter address
        address tokenIn;
        address tokenOut;
        uint24 fee;          // V3 fee tier (ignored for V2)
    }

    /// @notice A complete swap path to quote
    struct PathQuote {
        Hop[] hops;
        uint256 amountIn;
    }

    /// @notice Result for a single path
    struct PathResult {
        uint256 amountOut;
        bool success;
    }

    // ─── Initializer (replaces constructor for UUPS) ─────

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    function initialize(address owner_) external initializer {
        __Ownable_init(owner_);
    }

    // ─── UUPS authorization ──────────────────────────────

    function _authorizeUpgrade(address newImplementation) internal override onlyOwner {}

    // ─── Core quoting functions ──────────────────────────

    /// @notice Quote a single multi-hop path
    /// @param hops Array of hops in the path
    /// @param amountIn Starting input amount
    /// @return amountOut Final output amount (0 if any hop fails)
    function quoteSinglePath(
        Hop[] calldata hops,
        uint256 amountIn
    ) public returns (uint256 amountOut) {
        amountOut = amountIn;

        for (uint256 i = 0; i < hops.length; i++) {
            if (hops[i].poolType == PoolType.UniswapV2) {
                amountOut = _quoteV2(
                    hops[i].router,
                    amountOut,
                    hops[i].tokenIn,
                    hops[i].tokenOut
                );
            } else {
                // V3 and CamelotAlgebra use the same quoter interface
                amountOut = _quoteV3(
                    hops[i].router,
                    amountOut,
                    hops[i].tokenIn,
                    hops[i].tokenOut,
                    hops[i].fee
                );
            }

            if (amountOut == 0) return 0;
        }
    }

    /// @notice Quote multiple paths in a single call
    /// @param paths Array of paths to quote
    /// @return results Array of (amountOut, success) for each path
    function quoteMultiPaths(
        PathQuote[] calldata paths
    ) external returns (PathResult[] memory results) {
        results = new PathResult[](paths.length);

        for (uint256 i = 0; i < paths.length; i++) {
            try this.quoteSinglePath(paths[i].hops, paths[i].amountIn) returns (uint256 out) {
                results[i] = PathResult({amountOut: out, success: out > 0});
            } catch {
                results[i] = PathResult({amountOut: 0, success: false});
            }
        }
    }

    /// @notice Quick 2-hop arbitrage quote: tokenX -> tokenA -> tokenX
    /// @param hop1 First hop (buy)
    /// @param hop2 Second hop (sell)
    /// @param amountIn Starting amount of tokenX
    /// @return amountOut Final amount of tokenX
    /// @return profit Net profit (amountOut - amountIn), negative if loss
    function quoteArbitrage2Hop(
        Hop calldata hop1,
        Hop calldata hop2,
        uint256 amountIn
    ) external returns (uint256 amountOut, int256 profit) {
        uint256 intermediate = _quoteHop(hop1, amountIn);
        if (intermediate == 0) return (0, -int256(amountIn));

        amountOut = _quoteHop(hop2, intermediate);
        profit = int256(amountOut) - int256(amountIn);
    }

    /// @notice Batch multiple 2-hop arbitrage quotes
    /// @dev Most common use case: same amountIn, different pool pairs
    struct ArbQuote {
        Hop hop1;
        Hop hop2;
        uint256 amountIn;
    }

    struct ArbResult {
        uint256 amountOut;
        int256 profit;
        bool success;
    }

    function quoteBatchArbitrage(
        ArbQuote[] calldata quotes
    ) external returns (ArbResult[] memory results) {
        results = new ArbResult[](quotes.length);

        for (uint256 i = 0; i < quotes.length; i++) {
            try this.quoteArbitrage2HopExternal(
                quotes[i].hop1,
                quotes[i].hop2,
                quotes[i].amountIn
            ) returns (uint256 out, int256 prof) {
                results[i] = ArbResult({amountOut: out, profit: prof, success: true});
            } catch {
                results[i] = ArbResult({amountOut: 0, profit: 0, success: false});
            }
        }
    }

    /// @dev External wrapper for try/catch in batch (can't try/catch internal calls)
    function quoteArbitrage2HopExternal(
        Hop calldata hop1,
        Hop calldata hop2,
        uint256 amountIn
    ) external returns (uint256 amountOut, int256 profit) {
        uint256 intermediate = _quoteHop(hop1, amountIn);
        if (intermediate == 0) return (0, -int256(amountIn));

        amountOut = _quoteHop(hop2, intermediate);
        profit = int256(amountOut) - int256(amountIn);
    }

    // ─── Internal quote helpers ──────────────────────────

    function _quoteHop(Hop calldata hop, uint256 amountIn) internal returns (uint256) {
        if (hop.poolType == PoolType.UniswapV2) {
            return _quoteV2(hop.router, amountIn, hop.tokenIn, hop.tokenOut);
        } else if (hop.poolType == PoolType.UniswapV3) {
            return _quoteV3(hop.router, amountIn, hop.tokenIn, hop.tokenOut, hop.fee);
        } else {
            return _quoteCamelot(hop.router, amountIn, hop.tokenIn, hop.tokenOut);
        }
    }

    /// @dev Get V2 quote via router.getAmountsOut
    function _quoteV2(
        address router,
        uint256 amountIn,
        address tokenIn,
        address tokenOut
    ) internal view returns (uint256) {
        address[] memory path = new address[](2);
        path[0] = tokenIn;
        path[1] = tokenOut;

        try IUniswapV2Router(router).getAmountsOut(amountIn, path) returns (
            uint256[] memory amounts
        ) {
            return amounts[amounts.length - 1];
        } catch {
            return 0;
        }
    }

    /// @dev Get V3 quote via QuoterV2
    function _quoteV3(
        address quoter,
        uint256 amountIn,
        address tokenIn,
        address tokenOut,
        uint24 fee
    ) internal returns (uint256) {
        try IQuoterV2(quoter).quoteExactInputSingle(
            IQuoterV2.QuoteExactInputSingleParams({
                tokenIn: tokenIn,
                tokenOut: tokenOut,
                amountIn: amountIn,
                fee: fee,
                sqrtPriceLimitX96: 0
            })
        ) returns (uint256 amountOut, uint160, uint32, uint256) {
            return amountOut;
        } catch {
            return 0;
        }
    }

    /// @dev Get Camelot quote via Algebra Quoter
    function _quoteCamelot(
        address quoter,
        uint256 amountIn,
        address tokenIn,
        address tokenOut
    ) internal returns (uint256) {
        try IAlgebraQuoter(quoter).quoteExactInputSingle(
            tokenIn, tokenOut, amountIn, 0
        ) returns (uint256 amountOut, uint16) {
            return amountOut;
        } catch {
            return 0;
        }
    }

    // ─── Execution functions ─────────────────────────────
    
    address public constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;

    struct ExecHop {
        PoolType poolType;
        address router;      // Execution router address for swapping
        address tokenIn;
        address tokenOut;
        uint24 fee;
    }

    /// @notice Execute arbitrage flashloan using Balancer
    function executeArbitrage(
        ExecHop[] calldata paths, 
        uint256 flashloanAmount, 
        address flashloanToken
    ) external onlyOwner {
        address[] memory tokens = new address[](1);
        tokens[0] = flashloanToken;
        uint256[] memory amounts = new uint256[](1);
        amounts[0] = flashloanAmount;
        
        bytes memory userData = abi.encode(paths, msg.sender);
        
        IBalancerVault(BALANCER_VAULT).flashLoan(address(this), tokens, amounts, userData);
    }
    
    /// @notice Balancer flashloan callback
    function receiveFlashLoan(
        address[] calldata tokens,
        uint256[] calldata amounts,
        uint256[] calldata feeAmounts,
        bytes calldata userData
    ) external {
        require(msg.sender == BALANCER_VAULT, "Only Balancer Vault");
        
        (ExecHop[] memory paths, address initiator) = abi.decode(userData, (ExecHop[], address));
        
        uint256 currentAmount = amounts[0];
        
        // Execute hops
        for (uint256 i = 0; i < paths.length; i++) {
            currentAmount = _executeHop(paths[i], currentAmount);
        }
        
        uint256 amountToRepay = amounts[0] + feeAmounts[0];
        require(currentAmount >= amountToRepay, "Unprofitable arbitrage");
        
        // Repay flashloan
        IERC20(tokens[0]).transfer(BALANCER_VAULT, amountToRepay);
        
        // Send profits back to initiator
        uint256 profit = currentAmount - amountToRepay;
        if (profit > 0) {
            IERC20(tokens[0]).transfer(initiator, profit);
        }
    }

    function _executeHop(ExecHop memory hop, uint256 amountIn) internal returns (uint256 amountOut) {
        IERC20(hop.tokenIn).approve(hop.router, amountIn);
        
        if (hop.poolType == PoolType.UniswapV2) {
            address[] memory path = new address[](2);
            path[0] = hop.tokenIn;
            path[1] = hop.tokenOut;
            
            uint[] memory amounts = IUniswapV2Router(hop.router).swapExactTokensForTokens(
                amountIn, 1, path, address(this), block.timestamp
            );
            amountOut = amounts[amounts.length - 1];
            
        } else if (hop.poolType == PoolType.UniswapV3) {
            // Check if this is the PancakeSwap V3 Arbitrum router which lacks the deadline parameter
            if (hop.router == 0x32226588378236Fd0c7c4053999F88aC0e5cAc77) {
                IPancakeV3SwapRouter.ExactInputSingleParams memory params = IPancakeV3SwapRouter.ExactInputSingleParams({
                    tokenIn: hop.tokenIn,
                    tokenOut: hop.tokenOut,
                    fee: hop.fee,
                    recipient: address(this),
                    amountIn: amountIn,
                    amountOutMinimum: 1,
                    sqrtPriceLimitX96: 0
                });
                amountOut = IPancakeV3SwapRouter(hop.router).exactInputSingle(params);
            } else {
                ISwapRouter.ExactInputSingleParams memory params = ISwapRouter.ExactInputSingleParams({
                    tokenIn: hop.tokenIn,
                    tokenOut: hop.tokenOut,
                    fee: hop.fee,
                    recipient: address(this),
                    deadline: block.timestamp + 1200,
                    amountIn: amountIn,
                    amountOutMinimum: 1,
                    sqrtPriceLimitX96: 0
                });
                amountOut = ISwapRouter(hop.router).exactInputSingle(params);
            }
        } else if (hop.poolType == PoolType.CamelotAlgebra) {
            IAlgebraSwapRouter.ExactInputSingleParams memory params = IAlgebraSwapRouter.ExactInputSingleParams({
                tokenIn: hop.tokenIn,
                tokenOut: hop.tokenOut,
                recipient: address(this),
                deadline: block.timestamp + 1200,
                amountIn: amountIn,
                amountOutMinimum: 1,
                limitSqrtPrice: 0
            });
            amountOut = IAlgebraSwapRouter(hop.router).exactInputSingle(params);
        }
    }

    // ─── Version ─────────────────────────────────────────    // --- Internal Helpers ---

    function version() external pure returns (string memory) {
        return "1.1.0";
    }
}

// ─── Interfaces ──────────────────────────────────────────

interface IERC20 {
    function balanceOf(address account) external view returns (uint256);
    function approve(address spender, uint256 amount) external returns (bool);
    function transfer(address to, uint256 amount) external returns (bool);
}

interface IBalancerVault {
    function flashLoan(
        address recipient,
        address[] calldata tokens,
        uint256[] calldata amounts,
        bytes calldata userData
    ) external;
}

interface ISwapRouter {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);
}

interface IAlgebraSwapRouter {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 limitSqrtPrice;
    }
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);
}

interface IPancakeV3SwapRouter {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);
}

interface IUniswapV2Router {
    function swapExactTokensForTokens(
        uint amountIn,
        uint amountOutMin,
        address[] calldata path,
        address to,
        uint deadline
    ) external returns (uint[] memory amounts);
    
    function getAmountsOut(
        uint256 amountIn,
        address[] calldata path
    ) external view returns (uint256[] memory amounts);
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
        uint16 fee
    );
}
