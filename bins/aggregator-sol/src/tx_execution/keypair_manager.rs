// Keypair management for transaction signing
// Supports loading from file path or base58-encoded environment variable

use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::fs;
use std::sync::Arc;

/// Manages keypair loading and transaction signing
pub struct KeypairManager {
    keypair: Arc<Keypair>,
}

impl KeypairManager {
    /// Create from file path (JSON array format - 64 byte keypair)
    pub fn from_file(path: &str) -> Result<Self, String> {
        let data = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read keypair file: {}", e))?;

        let bytes: Vec<u8> = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse keypair JSON: {}", e))?;

        // Solana keypair files contain 64 bytes: 32-byte secret + 32-byte public
        // We need only the first 32 bytes (secret key) for new_from_array
        if bytes.len() < 32 {
            return Err(format!("Invalid keypair length: expected at least 32 bytes, got {}", bytes.len()));
        }
        
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes[..32]);

        let keypair = Keypair::new_from_array(secret);

        log::info!("Loaded keypair: {}", keypair.pubkey());
        Ok(Self {
            keypair: Arc::new(keypair),
        })
    }

    /// Create from base58-encoded private key
    pub fn from_base58(base58_key: &str) -> Result<Self, String> {
        let keypair = Keypair::from_base58_string(base58_key);

        log::info!("Loaded keypair from base58: {}", keypair.pubkey());
        Ok(Self {
            keypair: Arc::new(keypair),
        })
    }

    /// Load from environment (tries SOLANA_KEYPAIR_PATH first, then SOLANA_PRIVATE_KEY)
    pub fn from_env() -> Result<Self, String> {
        if let Ok(path) = std::env::var("SOLANA_KEYPAIR_PATH") {
            return Self::from_file(&path);
        }

        if let Ok(key) = std::env::var("SOLANA_PRIVATE_KEY") {
            return Self::from_base58(&key);
        }

        Err("Neither SOLANA_KEYPAIR_PATH nor SOLANA_PRIVATE_KEY environment variable is set".to_string())
    }

    /// Get the public key
    pub fn pubkey(&self) -> solana_sdk::pubkey::Pubkey {
        self.keypair.pubkey()
    }

    /// Sign a transaction
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<(), String> {
        tx.try_sign(&[self.keypair.as_ref()], tx.message.recent_blockhash)
            .map_err(|e| format!("Failed to sign transaction: {}", e))
    }

    /// Get the keypair (for cases where direct access is needed)
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }
}
