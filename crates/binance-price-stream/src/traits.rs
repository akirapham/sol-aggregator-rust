/// Trait for price service to allow mocking in tests
pub trait PriceServiceTrait: Send + Sync {
    /// Get current SOL price in USD
    fn get_sol_price(&self) -> f64;
}

// Implement the trait for the concrete BinancePriceStream
impl PriceServiceTrait for crate::BinancePriceStream {
    fn get_sol_price(&self) -> f64 {
        self.get_price("SOLUSDT").map(|p| p.price).unwrap_or(0.0)
    }
}
