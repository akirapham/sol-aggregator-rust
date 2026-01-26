// Transaction builder with compute budget optimization
// Handles priority fees and compute unit estimation

use solana_sdk::{
    instruction::Instruction, message::Message, pubkey::Pubkey, transaction::Transaction,
};
use std::str::FromStr;

// Compute Budget Program ID (hardcoded for compatibility)
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";

/// Transaction builder with compute budget optimization
pub struct TransactionBuilder;

impl TransactionBuilder {
    /// Add compute budget instructions to optimize transaction
    pub fn add_compute_budget_instructions(
        instructions: &mut Vec<Instruction>,
        compute_units: u32,
        priority_fee_microlamports: u64,
    ) {
        let program_id = Pubkey::from_str(COMPUTE_BUDGET_PROGRAM_ID).unwrap();

        // Set compute unit limit instruction (instruction index 2)
        let cu_limit_data = [
            0x02, // SetComputeUnitLimit instruction
            (compute_units & 0xFF) as u8,
            ((compute_units >> 8) & 0xFF) as u8,
            ((compute_units >> 16) & 0xFF) as u8,
            ((compute_units >> 24) & 0xFF) as u8,
        ];
        let cu_limit_ix = Instruction::new_with_bytes(program_id, &cu_limit_data, vec![]);

        // Set compute unit price instruction (instruction index 3)
        let cu_price_data = [
            0x03, // SetComputeUnitPrice instruction
            (priority_fee_microlamports & 0xFF) as u8,
            ((priority_fee_microlamports >> 8) & 0xFF) as u8,
            ((priority_fee_microlamports >> 16) & 0xFF) as u8,
            ((priority_fee_microlamports >> 24) & 0xFF) as u8,
            ((priority_fee_microlamports >> 32) & 0xFF) as u8,
            ((priority_fee_microlamports >> 40) & 0xFF) as u8,
            ((priority_fee_microlamports >> 48) & 0xFF) as u8,
            ((priority_fee_microlamports >> 56) & 0xFF) as u8,
        ];
        let cu_price_ix = Instruction::new_with_bytes(program_id, &cu_price_data, vec![]);

        // Insert at the beginning of instruction list
        instructions.insert(0, cu_price_ix);
        instructions.insert(0, cu_limit_ix);
    }

    /// Estimate compute units from a simulated transaction
    /// Returns the units consumed + a safety buffer (10%)
    pub fn estimate_compute_units_with_buffer(simulated_units: u64) -> u32 {
        let with_buffer = (simulated_units as f64 * 1.1) as u64;
        // Cap at 1.4M CU (Solana max is 1.4M per transaction)
        std::cmp::min(with_buffer, 1_400_000) as u32
    }

    /// Build an optimized transaction with priority fees
    pub fn build_optimized_transaction(
        instructions: Vec<Instruction>,
        payer: &Pubkey,
        recent_blockhash: solana_sdk::hash::Hash,
        compute_units: u32,
        priority_fee_microlamports: u64,
    ) -> Transaction {
        let mut all_instructions = instructions;

        // Add compute budget instructions
        Self::add_compute_budget_instructions(
            &mut all_instructions,
            compute_units,
            priority_fee_microlamports,
        );

        let message =
            Message::new_with_blockhash(&all_instructions, Some(payer), &recent_blockhash);
        Transaction::new_unsigned(message)
    }

    /// Calculate priority fee based on desired priority level
    /// Returns microlamports per compute unit
    pub fn calculate_priority_fee(priority_level: PriorityLevel) -> u64 {
        match priority_level {
            PriorityLevel::Low => 1_000,        // 1,000 microlamports
            PriorityLevel::Medium => 10_000,    // 10,000 microlamports
            PriorityLevel::High => 100_000,     // 100,000 microlamports
            PriorityLevel::VeryHigh => 500_000, // 500,000 microlamports
            PriorityLevel::Custom(fee) => fee,
        }
    }
}

/// Priority level for transaction submission
#[derive(Debug, Clone, Copy)]
pub enum PriorityLevel {
    Low,
    Medium,
    High,
    VeryHigh,
    Custom(u64),
}

impl Default for PriorityLevel {
    fn default() -> Self {
        PriorityLevel::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_units_buffer() {
        assert_eq!(
            TransactionBuilder::estimate_compute_units_with_buffer(100_000),
            110_000
        );
        assert_eq!(
            TransactionBuilder::estimate_compute_units_with_buffer(2_000_000),
            1_400_000
        ); // Capped
    }

    #[test]
    fn test_priority_fee_calculation() {
        assert_eq!(
            TransactionBuilder::calculate_priority_fee(PriorityLevel::Low),
            1_000
        );
        assert_eq!(
            TransactionBuilder::calculate_priority_fee(PriorityLevel::High),
            100_000
        );
        assert_eq!(
            TransactionBuilder::calculate_priority_fee(PriorityLevel::Custom(50_000)),
            50_000
        );
    }
}
