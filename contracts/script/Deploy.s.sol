// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import "../src/QuoteRouter.sol";
import "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

contract DeployQuoteRouter is Script {
    function run() external {
        // Private key to deploy from, fetched from env
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        // Start broadcasting transactions
        vm.startBroadcast(deployerPrivateKey);

        // 1. Deploy the implementation contract
        QuoteRouter implementation = new QuoteRouter();

        // 2. Encode the initialization data: initialize(address owner)
        // Note: Using `deployer` explicitly instead of `msg.sender`.
        // In scripts, `msg.sender` evaluated in memory resolves to Foundry's default 
        // default caller (0x1804c...) unless overridden.
        bytes memory initData = abi.encodeWithSelector(
            QuoteRouter.initialize.selector,
            deployer
        );

        // 3. Deploy the UUPS Proxy, pointing to the implementation and initializing
        ERC1967Proxy proxy = new ERC1967Proxy(
            address(implementation),
            initData
        );

        // Stop broadcasting
        vm.stopBroadcast();

        // Logging the deployed addresses
        console.log("Implementation deployed at:", address(implementation));
        console.log("QuoteRouter Proxy deployed at:", address(proxy));
        console.log("Proxy initialized with owner:", QuoteRouter(address(proxy)).owner());
    }
}
