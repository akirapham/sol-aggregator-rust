/// Chain-specific configuration for DEXes
/// Supports Ethereum and other EVM chains with their respective DEX implementations
use ethers::types::Address;
use std::collections::HashMap;

use crate::DexVersion;

/// Represents a chain (e.g., Ethereum, Polygon, Arbitrum, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chain {
    Ethereum,
    Polygon,
    Arbitrum,
    Optimism,
    Avalanche,
    Base,
    Scroll,
}

impl Chain {
    pub fn as_str(&self) -> &'static str {
        match self {
            Chain::Ethereum => "ethereum",
            Chain::Polygon => "polygon",
            Chain::Arbitrum => "arbitrum",
            Chain::Optimism => "optimism",
            Chain::Avalanche => "avalanche",
            Chain::Base => "base",
            Chain::Scroll => "scroll",
        }
    }

    pub fn chain_id(&self) -> u64 {
        match self {
            Chain::Ethereum => 1,
            Chain::Polygon => 137,
            Chain::Arbitrum => 42161,
            Chain::Optimism => 10,
            Chain::Avalanche => 43114,
            Chain::Base => 8453,
            Chain::Scroll => 534352,
        }
    }
}

/// Configuration for V2-style DEXes (Uniswap V2, Sushiswap V2, etc.)
#[derive(Debug, Clone)]
pub struct V2Config {
    /// Factory contract address
    pub factory: Address,
    /// Fee basis points (e.g., 30 for 0.3% = 30/10000)
    pub fee_bps: u32,
    /// Init code hash for pair creation calculation
    pub init_code_hash: [u8; 32],
}

/// Configuration for V3-style DEXes (Uniswap V3, Sushiswap V3, etc.)
#[derive(Debug, Clone)]
pub struct V3Config {
    /// Factory contract address
    pub factory: Address,
    /// Quoter contract address (usually Quoter V1 or V2)
    pub quoter: Address,
    /// Router contract address (optional)
    pub router: Option<Address>,
    /// Supported fee tiers in basis points
    pub fee_tiers: Vec<u32>,
}

/// Configuration for V4-style DEXes
#[derive(Debug, Clone)]
pub struct V4Config {
    /// Vault contract address
    pub vault: Address,
    /// Position manager contract address
    pub position_manager: Address,
    /// Quoter contract address
    pub quoter: Address,
    /// Router contract address
    pub router: Address,
}

/// Union of all DEX configurations
#[derive(Debug, Clone)]
pub enum DexConfig {
    V2(V2Config),
    V3(V3Config),
    V4(V4Config),
}

/// Per-chain DEX registry
#[derive(Debug, Clone)]
pub struct ChainDexRegistry {
    pub chain: Chain,
    pub dexes: HashMap<DexVersion, DexConfig>,
}

impl ChainDexRegistry {
    pub fn new(chain: Chain) -> Self {
        Self {
            chain,
            dexes: HashMap::new(),
        }
    }

    pub fn register(&mut self, version: DexVersion, config: DexConfig) {
        self.dexes.insert(version, config);
    }

    pub fn get(&self, version: &DexVersion) -> Option<&DexConfig> {
        self.dexes.get(version)
    }

    pub fn list_dexes(&self) -> Vec<DexVersion> {
        self.dexes.keys().copied().collect()
    }
}

/// Global registry for all chains and their DEXes
pub struct GlobalDexRegistry {
    chains: HashMap<Chain, ChainDexRegistry>,
}

impl GlobalDexRegistry {
    pub fn new() -> Self {
        Self {
            chains: HashMap::new(),
        }
    }

    pub fn register_chain(&mut self, registry: ChainDexRegistry) {
        self.chains.insert(registry.chain, registry);
    }

    pub fn get_chain(&self, chain: Chain) -> Option<&ChainDexRegistry> {
        self.chains.get(&chain)
    }

    pub fn get_chain_mut(&mut self, chain: Chain) -> Option<&mut ChainDexRegistry> {
        self.chains.get_mut(&chain)
    }

    pub fn list_chains(&self) -> Vec<Chain> {
        self.chains.keys().copied().collect()
    }
}

impl Default for GlobalDexRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize Ethereum mainnet DEX registry
pub fn ethereum_registry() -> ChainDexRegistry {
    let mut registry = ChainDexRegistry::new(Chain::Ethereum);

    // Uniswap V2
    let v2_init_code = [
        0x96, 0xe8, 0xac, 0x42, 0x77, 0x19, 0x8f, 0xf8, 0xb6, 0xf7, 0x85, 0x47, 0x8a, 0xa9, 0xa3,
        0x9f, 0x40, 0x3c, 0xb7, 0x68, 0xdd, 0x02, 0xcb, 0xee, 0x32, 0x6c, 0x3e, 0x26, 0x5c, 0xbd,
        0x36, 0x27,
    ];
    registry.register(
        DexVersion::UniswapV2,
        DexConfig::V2(V2Config {
            factory: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f"
                .parse()
                .unwrap(),
            fee_bps: 30,
            init_code_hash: v2_init_code,
        }),
    );

    // Uniswap V3
    registry.register(
        DexVersion::UniswapV3,
        DexConfig::V3(V3Config {
            factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            quoter: "0xb27f1eea633e94c6f33eee83f00648d5b32545f4"
                .parse()
                .unwrap(),
            router: Some(
                "0xE592427A0AEce92De3Edee1F18E0157C05861564"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    // Sushiswap V2
    let sushi_v2_init_code = [
        0xe1, 0x8a, 0x34, 0xeb, 0x0e, 0x04, 0xb0, 0x4f, 0x7a, 0x0a, 0xc2, 0x9a, 0x6e, 0x80, 0x74,
        0x8d, 0xca, 0x56, 0xea, 0x3f, 0x40, 0xe8, 0xe2, 0x1e, 0x02, 0xc3, 0x35, 0x82, 0x0c, 0x3e,
        0xad, 0x02,
    ];
    registry.register(
        DexVersion::SushiswapV2,
        DexConfig::V2(V2Config {
            factory: "0xC0AEe478e3861677F9B2dF0b7cEFEAD571b5590F"
                .parse()
                .unwrap(),
            fee_bps: 30,
            init_code_hash: sushi_v2_init_code,
        }),
    );

    // Sushiswap V3
    registry.register(
        DexVersion::SushiswapV3,
        DexConfig::V3(V3Config {
            factory: "0xbACEB8f6f2f64F4ad4cAFFA62f3b326D883c3213"
                .parse()
                .unwrap(),
            quoter: "0x1D24f3DBcAc7f37B4d0c5b6aff50Bb2643720f5e"
                .parse()
                .unwrap(),
            router: Some(
                "0x2214A42d8e2830BEe360331851FD07b3BA109244"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    registry
}

/// Initialize Polygon (Matic) DEX registry
pub fn polygon_registry() -> ChainDexRegistry {
    let mut registry = ChainDexRegistry::new(Chain::Polygon);

    // Uniswap V3
    registry.register(
        DexVersion::UniswapV3,
        DexConfig::V3(V3Config {
            factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            quoter: "0xb27f1eea633e94c6f33eee83f00648d5b32545f4"
                .parse()
                .unwrap(),
            router: Some(
                "0xE592427A0AEce92De3Edee1F18E0157C05861564"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    // Sushiswap V3
    registry.register(
        DexVersion::SushiswapV3,
        DexConfig::V3(V3Config {
            factory: "0xbACEB8f6f2f64F4ad4cAFFA62f3b326D883c3213"
                .parse()
                .unwrap(),
            quoter: "0x49d1f43Cc02eA0F9D1dB95eAc4cDb65C1B83A33c"
                .parse()
                .unwrap(),
            router: Some(
                "0x2214A42d8e2830BEe360331851FD07b3BA109244"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    registry
}

/// Initialize Arbitrum DEX registry
pub fn arbitrum_registry() -> ChainDexRegistry {
    let mut registry = ChainDexRegistry::new(Chain::Arbitrum);

    // Uniswap V3
    registry.register(
        DexVersion::UniswapV3,
        DexConfig::V3(V3Config {
            factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            quoter: "0xb27f1eea633e94c6f33eee83f00648d5b32545f4"
                .parse()
                .unwrap(),
            router: Some(
                "0xE592427A0AEce92De3Edee1F18E0157C05861564"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    registry
}

/// Initialize Optimism DEX registry
pub fn optimism_registry() -> ChainDexRegistry {
    let mut registry = ChainDexRegistry::new(Chain::Optimism);

    // Uniswap V3
    registry.register(
        DexVersion::UniswapV3,
        DexConfig::V3(V3Config {
            factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            quoter: "0xb27f1eea633e94c6f33eee83f00648d5b32545f4"
                .parse()
                .unwrap(),
            router: Some(
                "0xE592427A0AEce92De3Edee1F18E0157C05861564"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    registry
}

/// Initialize Base DEX registry
pub fn base_registry() -> ChainDexRegistry {
    let mut registry = ChainDexRegistry::new(Chain::Base);

    // Uniswap V3
    registry.register(
        DexVersion::UniswapV3,
        DexConfig::V3(V3Config {
            factory: "0x33128a8fC17869897DCE68Ed026d694621f6FDaD"
                .parse()
                .unwrap(),
            quoter: "0x3d4e44eb1374240CE5F1B048EC6766cCd51f2b50"
                .parse()
                .unwrap(),
            router: Some(
                "0x2626664c2603336E57B271c5C0b26F421741e481"
                    .parse()
                    .unwrap(),
            ),
            fee_tiers: vec![100, 500, 3000, 10000],
        }),
    );

    registry
}

/// Create a global registry with all supported chains
pub fn create_global_registry() -> GlobalDexRegistry {
    let mut global = GlobalDexRegistry::new();

    global.register_chain(ethereum_registry());
    global.register_chain(polygon_registry());
    global.register_chain(arbitrum_registry());
    global.register_chain(optimism_registry());
    global.register_chain(base_registry());

    global
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethereum_registry() {
        let registry = ethereum_registry();
        assert_eq!(registry.chain, Chain::Ethereum);
        assert!(registry.get(&DexVersion::UniswapV2).is_some());
        assert!(registry.get(&DexVersion::UniswapV3).is_some());
        assert!(registry.get(&DexVersion::SushiswapV2).is_some());
    }

    #[test]
    fn test_global_registry() {
        let global = create_global_registry();
        assert!(global.get_chain(Chain::Ethereum).is_some());
        assert!(global.get_chain(Chain::Polygon).is_some());
        assert!(global.get_chain(Chain::Arbitrum).is_some());
    }

    #[test]
    fn test_chain_ids() {
        assert_eq!(Chain::Ethereum.chain_id(), 1);
        assert_eq!(Chain::Polygon.chain_id(), 137);
        assert_eq!(Chain::Arbitrum.chain_id(), 42161);
    }
}
