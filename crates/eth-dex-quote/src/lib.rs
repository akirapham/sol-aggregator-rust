pub mod chains;
pub mod quoter;
pub mod types;
pub mod v2;
pub mod v3;

pub use chains::*;
pub use quoter::UniversalQuoter;
pub use types::*;
pub use v2::UniswapV2Quoter;
pub use v3::UniswapV3Quoter;

#[derive(Debug, Clone)]
pub struct QuoteRequest {
    pub token_in: String,
    pub token_out: String,
    pub amount_in: ethers::types::U256,
    pub fee_tier: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct QuoteResponse {
    pub amount_out: ethers::types::U256,
    pub path: Vec<String>,
    pub dex: String,
}
