use anyhow::Result;
use cex_price_provider::{mexc::MexcService, PriceProvider};

#[tokio::main]
async fn main() -> Result<()> {
    let mexc = MexcService::new(cex_price_provider::FilterAddressType::Ethereum);
    mexc.start().await?;
    Ok(())
}
