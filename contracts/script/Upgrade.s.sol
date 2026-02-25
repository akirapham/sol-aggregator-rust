// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import "../src/QuoteRouter.sol";

contract UpgradeQuoteRouter is Script {
    function run() external {
        // Private key to deploy from, fetched from env
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        
        // Ensure proxy address is provided via ENV
        address proxyAddress = vm.envAddress("PROXY_ADDRESS");
        require(proxyAddress != address(0), "PROXY_ADDRESS env variable missing");

        // Start broadcasting transactions
        vm.startBroadcast(deployerPrivateKey);

        // 1. Deploy the new implementation contract logic
        QuoteRouter newImplementation = new QuoteRouter();

        // 2. Call upgradeToAndCall on the existing Proxy casting it through the QuoteRouter interface
        // We pass empty data "" because there's no new initialization needed
        QuoteRouter(proxyAddress).upgradeToAndCall(
            address(newImplementation),
            ""
        );

        // Stop broadcasting
        vm.stopBroadcast();

        // Logging the deployed addresses
        console.log("New Implementation deployed at:", address(newImplementation));
        console.log("Successfully upgraded Proxy at:", proxyAddress);
        console.log("Verification checks, owner is still:", QuoteRouter(proxyAddress).owner());
        console.log("New Version:", QuoteRouter(proxyAddress).version());
    }
}
