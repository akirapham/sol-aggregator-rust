use anyhow::{Context, Result};
use dashmap::DashMap;
use ethers::types::Address;
use log::{error, info};
use rocksdb::{Options, DB};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPairData {
    pub token0: String, // Address as hex string
    pub token1: String, // Address as hex string
    pub token0_decimals: u8,
    pub token1_decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDecimalData {
    pub address: String,
    pub decimals: u8,
}

pub struct TokenPairDb {
    db: Arc<DB>,
}

impl TokenPairDb {
    /// Open or create a RocksDB database at the specified path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let db = DB::open(&opts, path).context("Failed to open RocksDB")?;

        info!("Opened RocksDB at {:?}", db.path());

        Ok(Self { db: Arc::new(db) })
    }

    /// Save token pair to database with decimals
    pub fn save_token_pair(
        &self,
        pool_address: Address,
        token0: Address,
        token1: Address,
        token0_decimals: u8,
        token1_decimals: u8,
    ) -> Result<()> {
        let key = format!("pair:{:?}", pool_address);
        let data = TokenPairData {
            token0: format!("{:?}", token0),
            token1: format!("{:?}", token1),
            token0_decimals,
            token1_decimals,
        };

        let value = serde_json::to_vec(&data)?;
        self.db.put(key.as_bytes(), value)?;

        Ok(())
    }

    /// Load token pair from database
    pub fn load_token_pair(
        &self,
        pool_address: Address,
    ) -> Result<Option<(Address, Address, u8, u8)>> {
        let key = format!("pair:{:?}", pool_address);

        match self.db.get(key.as_bytes())? {
            Some(value) => {
                let data: TokenPairData = serde_json::from_slice(&value)?;
                let token0 = data.token0.parse::<Address>()?;
                let token1 = data.token1.parse::<Address>()?;
                Ok(Some((
                    token0,
                    token1,
                    data.token0_decimals,
                    data.token1_decimals,
                )))
            }
            None => Ok(None),
        }
    }

    /// Save token decimal to database
    pub fn save_token_decimal(&self, token_address: Address, decimals: u8) -> Result<()> {
        let key = format!("token:{:?}", token_address);
        let data = TokenDecimalData {
            address: format!("{:?}", token_address),
            decimals,
        };

        let value = serde_json::to_vec(&data)?;
        self.db.put(key.as_bytes(), value)?;

        Ok(())
    }

    /// Load token decimal from database
    pub fn load_token_decimal(&self, token_address: Address) -> Result<Option<u8>> {
        let key = format!("token:{:?}", token_address);

        match self.db.get(key.as_bytes())? {
            Some(value) => {
                let data: TokenDecimalData = serde_json::from_slice(&value)?;
                Ok(Some(data.decimals))
            }
            None => Ok(None),
        }
    }

    /// Save all token pairs from DashMap to database
    pub fn save_all_from_cache(
        &self,
        pair_cache: &DashMap<Address, (Address, Address, u8, u8)>,
        decimal_cache: &DashMap<Address, u8>,
    ) -> Result<usize> {
        let mut count = 0;

        // Save all token pairs
        for entry in pair_cache.iter() {
            let pool_address = *entry.key();
            let (token0, token1, decimals0, decimals1) = *entry.value();

            if let Err(e) = self.save_token_pair(pool_address, token0, token1, decimals0, decimals1)
            {
                error!("Failed to save token pair for {:?}: {}", pool_address, e);
            } else {
                count += 1;
            }
        }

        // Save all token decimals
        for entry in decimal_cache.iter() {
            let token_address = *entry.key();
            let decimals = *entry.value();

            if let Err(e) = self.save_token_decimal(token_address, decimals) {
                error!(
                    "Failed to save token decimal for {:?}: {}",
                    token_address, e
                );
            }
        }

        Ok(count)
    }

    /// Load all token pairs from database into DashMap
    pub fn load_all_into_cache(
        &self,
        pair_cache: &DashMap<Address, (Address, Address, u8, u8)>,
        decimal_cache: &DashMap<Address, u8>,
    ) -> Result<usize> {
        let mut count = 0;
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);

        for item in iter {
            match item {
                Ok((key, value)) => {
                    let key_str = String::from_utf8_lossy(&key);

                    // Load token pairs (key starts with "pair:")
                    if key_str.starts_with("pair:") {
                        if let Ok(data) = serde_json::from_slice::<TokenPairData>(&value) {
                            if let (Ok(pool_address), Ok(token0), Ok(token1)) = (
                                key_str.trim_start_matches("pair:").parse::<Address>(),
                                data.token0.parse::<Address>(),
                                data.token1.parse::<Address>(),
                            ) {
                                pair_cache.insert(
                                    pool_address,
                                    (token0, token1, data.token0_decimals, data.token1_decimals),
                                );
                                count += 1;
                            }
                        }
                    }
                    // Load token decimals (key starts with "token:")
                    else if key_str.starts_with("token:") {
                        if let Ok(data) = serde_json::from_slice::<TokenDecimalData>(&value) {
                            if let Ok(token_address) =
                                key_str.trim_start_matches("token:").parse::<Address>()
                            {
                                decimal_cache.insert(token_address, data.decimals);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading from RocksDB: {}", e);
                }
            }
        }

        Ok(count)
    }

    /// Get the number of entries in the database
    pub fn count(&self) -> usize {
        self.db.iterator(rocksdb::IteratorMode::Start).count()
    }
}
