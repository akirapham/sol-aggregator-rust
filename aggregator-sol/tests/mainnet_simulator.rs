mod common;

#[cfg(test)]
mod tests {
    use crate::common::*;

    #[tokio::test]
    async fn test_arbitrage_detection_and_execution() {
        let _ = env_logger::try_init();
        log::info!("Starting test");

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let opportunities = simulator.detect_opportunities().await;
        assert!(!opportunities.is_empty());

        let first_opp = &opportunities[0];
        let result = simulator.execute_arbitrage(first_opp).await;

        assert!(result.success);
        assert!(result.amount_out.is_some());
        assert!(result.amount_out.unwrap() > first_opp.input_amount);
    }

    #[tokio::test]
    async fn test_whirlpool_swap_execution() {
        let _ = env_logger::try_init();

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let result = simulator.test_whirlpool_swap().await;

        assert!(result.success);
        assert!(result.forward_signature.is_some());
        assert!(result.reverse_signature.is_some());
    }

    #[tokio::test]
    async fn test_raydium_swap_execution() {
        let _ = env_logger::try_init();

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let result = simulator.test_raydium_swap().await;

        assert!(result.success);
        assert!(result.forward_signature.is_some());
        assert!(result.reverse_signature.is_some());
    }

    #[tokio::test]
    async fn test_multiple_opportunities_sequential() {
        let _ = env_logger::try_init();

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let opportunities = simulator.detect_opportunities().await;
        assert!(opportunities.len() >= 3);

        let mut execution_records = Vec::new();

        for opp in opportunities.iter().take(3) {
            let record = simulator.execute_and_track(opp).await;
            execution_records.push(record);
        }

        assert_eq!(execution_records.len(), 3);
        for record in &execution_records {
            assert_eq!(record.status, "Completed");
        }
    }

    #[tokio::test]
    async fn test_transaction_status_tracking() {
        let _ = env_logger::try_init();

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let opportunities = simulator.detect_opportunities().await;
        let record = simulator.execute_and_track(&opportunities[0]).await;

        assert!(!record.opportunity_id.is_empty());
        assert!(record.forward_signature_count > 0);
        assert!(record.reverse_signature_count > 0);
        assert!(record.completed_at >= record.started_at);
    }

    #[tokio::test]
    async fn test_slippage_protection() {
        let _ = env_logger::try_init();

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let result = simulator.test_slippage_protection(500).await;

        assert!(result.success);
        assert!(result.amount_out.is_some());

        if let Some(out) = result.amount_out {
            let input = result.amount_in;
            let actual_slippage = if out < input {
                ((input - out) as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            assert!(actual_slippage <= 5.0);
        }
    }

    #[tokio::test]
    async fn test_profit_calculation() {
        let _ = env_logger::try_init();

        let simulator = MainnetSimulator::new("http://localhost:8899").await;
        simulator.setup_test_environment().await;

        let total_profit = simulator.calculate_profit_correctly().await;

        assert!(total_profit > 0);
    }
}
