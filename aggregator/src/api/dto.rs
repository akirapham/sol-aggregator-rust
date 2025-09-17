use serde::Serialize; // Ensure serde is imported for Json serialization

#[derive(Serialize)] // Required for Json response
pub struct PoolInfoResponse {
    pub address: String,
    pub dex: String,
    pub base_token: String,
    pub quote_token: String,
    pub last_updated: u64,
}
