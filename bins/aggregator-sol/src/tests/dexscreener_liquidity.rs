// Test to fetch and print liquidity for arbitrage pairs from DexScreener API
// Run with: cargo test --bin aggregator-sol test_dexscreener_liquidity -- --nocapture

use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

/// DexScreener API response structures
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexScreenerResponse {
    pairs: Option<Vec<DexScreenerPair>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexScreenerPair {
    chain_id: String,
    dex_id: String,
    pair_address: String,
    base_token: DexScreenerToken,
    quote_token: DexScreenerToken,
    price_usd: Option<String>,
    liquidity: Option<DexScreenerLiquidity>,
    volume: Option<DexScreenerVolume>,
    fdv: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexScreenerToken {
    address: String,
    name: String,
    symbol: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexScreenerLiquidity {
    usd: Option<f64>,
    base: Option<f64>,
    quote: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexScreenerVolume {
    h24: Option<f64>,
    h6: Option<f64>,
    h1: Option<f64>,
    m5: Option<f64>,
}

/// Token addresses for arbitrage pairs
struct ArbitrageTokens {
    symbol: &'static str,
    address: &'static str,
}

const TOKENS: &[ArbitrageTokens] = &[
    ArbitrageTokens { symbol: "SOL", address: "So11111111111111111111111111111111111111112" },
    ArbitrageTokens { symbol: "USDC", address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" },
    ArbitrageTokens { symbol: "USDT", address: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" },
    ArbitrageTokens { symbol: "mSOL", address: "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So" },
    ArbitrageTokens { symbol: "jitoSOL", address: "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn" },
    ArbitrageTokens { symbol: "JupSOL", address: "jupSoLaHXQiZZTSfEWMTRRgpnyFm8f6sZdosWBjx93v" }, // New LST
    ArbitrageTokens { symbol: "RAY", address: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R" },
    ArbitrageTokens { symbol: "JUP", address: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN" },
    ArbitrageTokens { symbol: "PUMP", address: "pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn" },
    ArbitrageTokens { symbol: "BONK", address: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263" },
    ArbitrageTokens { symbol: "WIF", address: "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm" },
    ArbitrageTokens { symbol: "PYTH", address: "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3" },
    ArbitrageTokens { symbol: "RENDER", address: "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof" },
    ArbitrageTokens { symbol: "PENGU", address: "2zMMhcVQEXDtdE6vsFS7S7D5oUodfJHE8vd1gnBouauv" },
    ArbitrageTokens { symbol: "KMNO", address: "KMNo3nJsBXfcpJTVhZcXLW7RmTwTt4GVFE7suUBo9sS" }, // Kamino
    ArbitrageTokens { symbol: "MET", address: "METAewgxyPbgwsseH8T16a39CQ5VyVxZi9zXiDPY18m" }, // Meteora
    ArbitrageTokens { symbol: "USD1", address: "4oRwqhNroh7kgwNXCnu9idZ861zdbWLVfv7aERUcuzU3" }, // USD1 stablecoin
];

/// Fetch token data from DexScreener API
async fn fetch_token_pairs(client: &Client, token_address: &str) -> Result<Vec<DexScreenerPair>, String> {
    let url = format!(
        "https://api.dexscreener.com/latest/dex/tokens/{}",
        token_address
    );

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }

    let data: DexScreenerResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    Ok(data.pairs.unwrap_or_default())
}

/// Filter pairs to only Solana pairs with significant liquidity
fn filter_solana_pairs(pairs: Vec<DexScreenerPair>, min_liquidity_usd: f64) -> Vec<DexScreenerPair> {
    pairs
        .into_iter()
        .filter(|p| p.chain_id == "solana")
        .filter(|p| {
            p.liquidity
                .as_ref()
                .and_then(|l| l.usd)
                .unwrap_or(0.0)
                >= min_liquidity_usd
        })
        .collect()
}

#[tokio::test]
async fn test_dexscreener_liquidity() {
    println!("\n{}", "=".repeat(80));
    println!("DexScreener Liquidity Report for Arbitrage Pairs");
    println!("{}\n", "=".repeat(80));

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    let min_liquidity = 10_000.0; // Minimum $10k liquidity
    let mut total_pairs = 0;
    let mut total_liquidity = 0.0;

    // Summary table header
    println!(
        "{:<10} {:<45} {:<15} {:<12} {:<15}",
        "Token", "Pair Address", "DEX", "Liquidity", "24h Volume"
    );
    println!("{}", "-".repeat(100));

    for token in TOKENS {
        println!("\n🔍 Fetching pairs for {} ({})...", token.symbol, &token.address[..8]);

        match fetch_token_pairs(&client, token.address).await {
            Ok(pairs) => {
                let filtered = filter_solana_pairs(pairs, min_liquidity);
                
                if filtered.is_empty() {
                    println!("   ⚠️  No pairs with >${:.0}k liquidity", min_liquidity / 1000.0);
                    continue;
                }

                for pair in &filtered {
                    let liquidity = pair
                        .liquidity
                        .as_ref()
                        .and_then(|l| l.usd)
                        .unwrap_or(0.0);
                    let volume_24h = pair
                        .volume
                        .as_ref()
                        .and_then(|v| v.h24)
                        .unwrap_or(0.0);

                    total_pairs += 1;
                    total_liquidity += liquidity;

                    let pair_name = format!(
                        "{}/{}",
                        pair.base_token.symbol,
                        pair.quote_token.symbol
                    );

                    println!(
                        "{:<10} {:<45} {:<15} ${:<11.0} ${:<14.0}",
                        pair_name,
                        &pair.pair_address[..40],
                        pair.dex_id,
                        liquidity,
                        volume_24h
                    );
                }
            }
            Err(e) => {
                println!("   ❌ Error: {}", e);
            }
        }

        // Rate limiting - DexScreener allows ~300 req/min
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    // Summary
    println!("\n{}", "=".repeat(100));
    println!("📊 SUMMARY");
    println!("{}", "=".repeat(100));
    println!("Total pairs with >${:.0}k liquidity: {}", min_liquidity / 1000.0, total_pairs);
    println!("Total liquidity tracked: ${:.2}M", total_liquidity / 1_000_000.0);
    println!();
}

#[tokio::test]
async fn test_dexscreener_top_pairs_by_liquidity() {
    println!("\n{}", "=".repeat(80));
    println!("Top 20 Solana Pairs by Liquidity");
    println!("{}\n", "=".repeat(80));

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    let mut all_pairs: Vec<DexScreenerPair> = Vec::new();

    for token in TOKENS {
        if let Ok(pairs) = fetch_token_pairs(&client, token.address).await {
            let filtered: Vec<_> = pairs
                .into_iter()
                .filter(|p| p.chain_id == "solana")
                .filter(|p| p.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0) > 50_000.0)
                .collect();
            all_pairs.extend(filtered);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    // Deduplicate by pair address
    let mut seen: HashMap<String, bool> = HashMap::new();
    all_pairs.retain(|p| {
        if seen.contains_key(&p.pair_address) {
            false
        } else {
            seen.insert(p.pair_address.clone(), true);
            true
        }
    });

    // Sort by liquidity
    all_pairs.sort_by(|a, b| {
        let liq_a = a.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
        let liq_b = b.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
        liq_b.partial_cmp(&liq_a).unwrap()
    });

    // Print top 20
    println!(
        "{:<5} {:<20} {:<45} {:<15} {:<15}",
        "Rank", "Pair", "Address", "DEX", "Liquidity"
    );
    println!("{}", "-".repeat(100));

    for (i, pair) in all_pairs.iter().take(20).enumerate() {
        let liquidity = pair.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
        let pair_name = format!("{}/{}", pair.base_token.symbol, pair.quote_token.symbol);

        println!(
            "{:<5} {:<20} {:<45} {:<15} ${:<14.0}",
            i + 1,
            pair_name,
            &pair.pair_address[..40],
            pair.dex_id,
            liquidity
        );
    }

    println!("\nTotal unique pairs found: {}", all_pairs.len());
}
