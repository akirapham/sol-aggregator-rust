use eth_dex_quote::v2::V2Quoter;
use eth_dex_quote::v3::V3Quoter;
use eth_dex_quote::v4::{UniswapV4Quoter, V4Quoter};
use eth_dex_quote::{create_global_registry, Chain, DexVersion, UniswapV2Quoter, UniswapV3Quoter};
use ethers::providers::{Http, Provider};
use ethers::types::Address;
use std::str::FromStr;
use std::sync::Arc;

/// Ethereum mainnet RPC endpoint (public node)
const ETHEREUM_RPC: &str = "https://ethereum-rpc.publicnode.com";

/// Test token addresses on Ethereum mainnet
const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
const USDC: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
const DAI: &str = "0x6B175474E89094C44Da98b954EedeAC495271d0F";

#[tokio::test]
async fn test_uniswap_v2_weth_usdc_quote() {
    let provider =
        Arc::new(Provider::<Http>::try_from(ETHEREUM_RPC).expect("Failed to create provider"));

    let router_v2 = Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")
        .expect("Invalid router address"); // Uniswap V2 Router

    let quoter = UniswapV2Quoter::new(provider).with_router(router_v2);

    let weth = Address::from_str(WETH).expect("Invalid WETH address");
    let usdc = Address::from_str(USDC).expect("Invalid USDC address");
    let amount_in = ethers::types::U256::from(10_u64.pow(18)); // 1 WETH

    println!("Testing Uniswap V2 quote for {} WETH via router", amount_in);
    println!("WETH: {}", weth);
    println!("USDC: {}", usdc);
    println!("Router V2: {}", router_v2);

    let path = vec![weth, usdc];
    let result = quoter.get_quote(amount_in, path).await;

    match result {
        Ok(amount_out) => {
            assert!(
                amount_out > ethers::types::U256::zero(),
                "Amount out should be > 0"
            );
            println!(
                "✅ Uniswap V2 Quote: {} WETH -> {} USDC (via router)",
                amount_in, amount_out
            );
        }
        Err(e) => {
            eprintln!("❌ Failed to get V2 quote: {:?}", e);
            panic!("Failed to get V2 quote: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_uniswap_v3_weth_usdc_quote() {
    let provider =
        Arc::new(Provider::<Http>::try_from(ETHEREUM_RPC).expect("Failed to create provider"));

    let quoter_v3 = Address::from_str("0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6")
        .expect("Invalid quoter address");

    let quoter = UniswapV3Quoter::new(provider, quoter_v3);

    let weth = Address::from_str(WETH).expect("Invalid WETH address");
    let usdc = Address::from_str(USDC).expect("Invalid USDC address");
    let amount_in = ethers::types::U256::from(10_u64.pow(18)); // 1 WETH
    let fee_tier = 3000; // 0.3% fee tier

    let result = quoter.get_quote(weth, usdc, amount_in, fee_tier).await;

    assert!(result.is_ok(), "Failed to get V3 quote: {:?}", result.err());

    let quote = result.unwrap();
    assert!(
        quote.amount_out > ethers::types::U256::zero(),
        "Amount out should be > 0"
    );
    assert_eq!(quote.route.len(), 2, "Route should have 2 tokens");
    assert_eq!(quote.route[0], weth, "First token should be WETH");
    assert_eq!(quote.route[1], usdc, "Second token should be USDC");

    println!(
        "✅ Uniswap V3 Quote: {} WETH -> {} USDC (0.3% fee)",
        amount_in, quote.amount_out
    );
}

#[tokio::test]
#[ignore = "V4 pools may not be available on testnet; enable to test live V4 deployment"]
async fn test_uniswap_v4_weth_usdc_quote() {
    let provider =
        Arc::new(Provider::<Http>::try_from(ETHEREUM_RPC).expect("Failed to create provider"));

    let quote_router = Address::from_str("0x52F0E24D1c21C8A0cB1e5a5dD6198556BD9E1203")
        .expect("Invalid V4 quote router address");

    let quoter = UniswapV4Quoter::new(provider, quote_router);

    let weth = Address::from_str(WETH).expect("Invalid WETH address");
    let usdc = Address::from_str(USDC).expect("Invalid USDC address");
    let amount_in = ethers::types::U256::from(10_u64.pow(18));
    let fee_tier = 3000;
    let tick_spacing = 60;
    let hooks = Address::zero();

    let result = quoter
        .get_quote(weth, usdc, amount_in, fee_tier, tick_spacing, hooks)
        .await;

    // V4 may not have active liquidity pairs yet, so we just log the result
    match result {
        Ok(quote) => {
            assert!(
                quote.amount_out > ethers::types::U256::zero(),
                "Amount out should be > 0"
            );
            println!(
                "✅ Uniswap V4 Quote: {} WETH -> {} USDC (0.3% fee, hooks=0)",
                amount_in, quote.amount_out
            );
        }
        Err(e) => {
            eprintln!(
                "⚠️  V4 quoter test failed (may be due to no active liquidity): {:?}",
                e
            );
            // Don't fail the test - V4 may not have pools yet
        }
    }
}

#[tokio::test]
async fn test_uniswap_v3_different_fee_tiers() {
    let provider =
        Arc::new(Provider::<Http>::try_from(ETHEREUM_RPC).expect("Failed to create provider"));

    let quoter_v3 = Address::from_str("0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6")
        .expect("Invalid quoter address");

    let quoter = UniswapV3Quoter::new(provider, quoter_v3);

    let weth = Address::from_str(WETH).expect("Invalid WETH address");
    let usdc = Address::from_str(USDC).expect("Invalid USDC address");
    let amount_in = ethers::types::U256::from(10_u64.pow(18)); // 1 WETH

    let fee_tiers = vec![100, 500, 3000, 10000]; // 0.01%, 0.05%, 0.3%, 1%
    let mut quotes = Vec::new();

    for fee_tier in fee_tiers {
        let result = quoter.get_quote(weth, usdc, amount_in, fee_tier).await;
        assert!(
            result.is_ok(),
            "Failed to get V3 quote for fee tier {}: {:?}",
            fee_tier,
            result.err()
        );

        let quote = result.unwrap();
        quotes.push((fee_tier, quote.amount_out));
    }

    println!("✅ Uniswap V3 Quotes for different fee tiers:");
    for (fee_tier, amount_out) in quotes {
        let fee_pct = fee_tier as f64 / 10000.0;
        println!("  Fee {:.2}%: {} USDC", fee_pct, amount_out);
    }
}

#[tokio::test]
async fn test_v2_quote_usdc_dai() {
    let provider =
        Arc::new(Provider::<Http>::try_from(ETHEREUM_RPC).expect("Failed to create provider"));

    let router_v2 = Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")
        .expect("Invalid router address"); // Uniswap V2 Router

    let quoter = UniswapV2Quoter::new(provider).with_router(router_v2);

    let usdc = Address::from_str(USDC).expect("Invalid USDC address");
    let dai = Address::from_str(DAI).expect("Invalid DAI address");
    let amount_in = ethers::types::U256::from(1000 * 10_u64.pow(6)); // 1000 USDC

    let path = vec![usdc, dai];
    let result = quoter.get_quote(amount_in, path).await;

    assert!(result.is_ok(), "Failed to get V2 quote: {:?}", result.err());

    let amount_out = result.unwrap();
    assert!(
        amount_out > ethers::types::U256::zero(),
        "Amount out should be > 0"
    );

    println!(
        "✅ Uniswap V2 Quote: {} USDC -> {} DAI",
        amount_in, amount_out
    );
}

#[test]
fn test_dex_version_from_str() {
    use std::str::FromStr;

    let v2 = DexVersion::from_str("uniswap_v2");
    assert!(v2.is_ok());
    assert_eq!(v2.unwrap(), DexVersion::UniswapV2);

    let v3 = DexVersion::from_str("uniswap_v3");
    assert!(v3.is_ok());
    assert_eq!(v3.unwrap(), DexVersion::UniswapV3);

    let sushi = DexVersion::from_str("sushiswap_v3");
    assert!(sushi.is_ok());

    let invalid = DexVersion::from_str("invalid_dex");
    assert!(invalid.is_err());

    println!("✅ DexVersion parsing works correctly");
}

#[test]
fn test_chain_registry() {
    let registry = create_global_registry();

    // Check Ethereum is available
    let eth_chain = registry.get_chain(Chain::Ethereum);
    assert!(eth_chain.is_some(), "Ethereum should be in global registry");

    let eth_registry = eth_chain.unwrap();

    // Check V2 is registered
    let v2_config = eth_registry.get(&DexVersion::UniswapV2);
    assert!(
        v2_config.is_some(),
        "Uniswap V2 should be registered on Ethereum"
    );

    // Check V3 is registered
    let v3_config = eth_registry.get(&DexVersion::UniswapV3);
    assert!(
        v3_config.is_some(),
        "Uniswap V3 should be registered on Ethereum"
    );

    // Check all DEXes on Ethereum
    let dexes = eth_registry.list_dexes();
    println!("✅ Available DEXes on Ethereum: {} total", dexes.len());
    for dex in dexes {
        println!("  - {}", dex.as_str());
    }
}

#[test]
fn test_all_chains_in_registry() {
    let registry = create_global_registry();
    let chains = registry.list_chains();

    println!("✅ Supported chains in registry:");
    for chain in chains {
        println!("  - {} (Chain ID: {})", chain.as_str(), chain.chain_id());

        let chain_registry = registry.get_chain(chain).unwrap();
        let dexes = chain_registry.list_dexes();
        println!("    DEXes: {}", dexes.len());
    }
}
#[tokio::test]
async fn test_multiple_consecutive_quotes() {
    let provider =
        Arc::new(Provider::<Http>::try_from(ETHEREUM_RPC).expect("Failed to create provider"));

    let router_v2 = Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")
        .expect("Invalid router address"); // Uniswap V2 Router

    let quoter = UniswapV2Quoter::new(provider).with_router(router_v2);

    let weth = Address::from_str(WETH).expect("Invalid WETH address");
    let usdc = Address::from_str(USDC).expect("Invalid USDC address");

    println!("✅ Testing multiple consecutive quotes:");
    for i in 1..=3 {
        let amount_in = ethers::types::U256::from(i * 10_u64.pow(18));
        let path = vec![weth, usdc];
        let result = quoter.get_quote(amount_in, path).await;

        assert!(result.is_ok(), "Quote {} failed", i);
        let amount_out = result.unwrap();
        println!("  Quote {}: {} WETH -> {} USDC", i, amount_in, amount_out);
    }
}
