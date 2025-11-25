use solana_sdk::{native_token::sol_str_to_lamports, pubkey::Pubkey};
use dashmap::DashMap;
use once_cell::sync::Lazy;

pub const CREATOR_FEE: u64 = 30;
pub const FEE_BASIS_POINTS: u64 = 95;

/// Calculates the amount of tokens that can be purchased with a given SOL amount
/// using the bonding curve formula.
///
/// # Arguments
/// * `virtual_token_reserves` - Virtual token reserves in the bonding curve
/// * `virtual_sol_reserves` - Virtual SOL reserves in the bonding curve
/// * `real_token_reserves` - Actual token reserves available for purchase
/// * `creator` - Creator's public key (affects fee calculation)
/// * `amount` - SOL amount to spend (in lamports)
///
/// # Returns
/// The amount of tokens that will be received (in token's smallest unit)
pub fn get_buy_token_amount_from_sol_amount(
    virtual_token_reserves: u128,
    virtual_sol_reserves: u128,
    real_token_reserves: u128,
    creator: Pubkey,
    amount: u64,
) -> u64 {
    if amount == 0 {
        return 0;
    }

    if virtual_token_reserves == 0 {
        return 0;
    }

    let total_fee_basis_points =
        FEE_BASIS_POINTS + if creator != Pubkey::default() { CREATOR_FEE } else { 0 };

    // Convert to u128 to prevent overflow
    let amount_128 = amount as u128;
    let total_fee_basis_points_128 = total_fee_basis_points as u128;

    let input_amount = amount_128
        .checked_mul(10_000)
        .unwrap()
        .checked_div(total_fee_basis_points_128 + 10_000)
        .unwrap();

    let denominator = virtual_sol_reserves + input_amount;

    let mut tokens_received =
        input_amount.checked_mul(virtual_token_reserves).unwrap().checked_div(denominator).unwrap();

    tokens_received = tokens_received.min(real_token_reserves);

    if tokens_received <= 100 * 1_000_000_u128 {
        tokens_received = if amount > sol_str_to_lamports("0.01").unwrap_or(0) {
            25547619 * 1_000_000_u128
        } else {
            255476 * 1_000_000_u128
        };
    }

    tokens_received as u64
}

/// Calculates the amount of SOL that will be received when selling a given token amount
/// using the bonding curve formula with transaction fees deducted.
///
/// # Arguments
/// * `virtual_token_reserves` - Virtual token reserves in the bonding curve
/// * `virtual_sol_reserves` - Virtual SOL reserves in the bonding curve
/// * `creator` - Creator's public key (affects fee calculation)
/// * `amount` - Token amount to sell (in token's smallest unit)
///
/// # Returns
/// The amount of SOL that will be received after fees (in lamports)
pub fn get_sell_sol_amount_from_token_amount(
    virtual_token_reserves: u128,
    virtual_sol_reserves: u128,
    creator: Pubkey,
    amount: u64,
) -> u64 {
    if amount == 0 {
        return 0;
    }

    // migrated bonding curve
    if virtual_token_reserves == 0 {
        return 0;
    }

    let amount_128 = amount as u128;

    // Calculate SOL amount received from selling tokens using constant product formula
    let numerator = amount_128.checked_mul(virtual_sol_reserves).unwrap_or(0);
    let denominator = virtual_token_reserves.checked_add(amount_128).unwrap_or(1);

    let sol_cost = numerator.checked_div(denominator).unwrap_or(0);

    let total_fee_basis_points =
        FEE_BASIS_POINTS + if creator != Pubkey::default() { CREATOR_FEE } else { 0 };
    let total_fee_basis_points_128 = total_fee_basis_points as u128;

    // Calculate transaction fee
    let fee = compute_fee(sol_cost, total_fee_basis_points_128);
    
    sol_cost.saturating_sub(fee) as u64
}

pub fn compute_fee(amount: u128, fee_basis_points: u128) -> u128 {
    ceil_div(amount * fee_basis_points, 10_000)
}

pub fn ceil_div(a: u128, b: u128) -> u128 {
    (a + b - 1) / b
}

pub fn get_bonding_curve_pda(mint: &Pubkey) -> Option<Pubkey> {
    get_cached_pda(
        PdaCacheKey::PumpFunBondingCurve(*mint),
        || {
            let seeds: &[&[u8]; 2] = &[seeds::BONDING_CURVE_SEED, mint.as_ref()];
            let program_id: &Pubkey = &accounts::PUMPFUN;
            let pda: Option<(Pubkey, u8)> = Pubkey::try_find_program_address(seeds, program_id);
            pda.map(|pubkey| pubkey.0)
        },
    )
}

pub fn get_cached_pda<F>(cache_key: PdaCacheKey, compute_fn: F) -> Option<Pubkey>
where
    F: FnOnce() -> Option<Pubkey>,
{
    // Fast path: check if already in cache
    if let Some(pda) = PDA_CACHE.get(&cache_key) {
        return Some(*pda);
    }

    // Slow path: compute and cache
    let pda_result = compute_fn();

    if let Some(pda) = pda_result {
        PDA_CACHE.insert(cache_key, pda);
    }

    pda_result
}

static PDA_CACHE: Lazy<DashMap<PdaCacheKey, Pubkey>> =
    Lazy::new(|| DashMap::with_capacity(MAX_PDA_CACHE_SIZE));
const MAX_PDA_CACHE_SIZE: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PdaCacheKey {
    PumpFunUserVolume(Pubkey),
    PumpFunBondingCurve(Pubkey),
    PumpFunCreatorVault(Pubkey),
    BonkPool(Pubkey, Pubkey),
    BonkVault(Pubkey, Pubkey),
    PumpSwapUserVolume(Pubkey),
}

pub mod seeds {
    /// Seed for bonding curve PDAs
    pub const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";

    /// Seed for creator vault PDAs
    pub const CREATOR_VAULT_SEED: &[u8] = b"creator-vault";

    /// Seed for metadata PDAs
    pub const METADATA_SEED: &[u8] = b"metadata";

    /// Seed for user volume accumulator PDAs
    pub const USER_VOLUME_ACCUMULATOR_SEED: &[u8] = b"user_volume_accumulator";

    /// Seed for global volume accumulator PDAs
    pub const GLOBAL_VOLUME_ACCUMULATOR_SEED: &[u8] = b"global_volume_accumulator";

    pub const FEE_CONFIG_SEED: &[u8] = b"fee_config";
}

pub mod accounts {
    use solana_sdk::{pubkey, pubkey::Pubkey};

    /// Public key for the Pump.fun program
    pub const PUMPFUN: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");

    /// Public key for the MPL Token Metadata program
    pub const MPL_TOKEN_METADATA: Pubkey = pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

    /// Authority for program events
    pub const EVENT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");

    /// Associated Token Program ID
    pub const ASSOCIATED_TOKEN_PROGRAM: Pubkey =
        pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

    pub const AMM_PROGRAM: Pubkey = pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");

    pub const FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");

    pub const GLOBAL_VOLUME_ACCUMULATOR: Pubkey =
        pubkey!("Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y");

    pub const FEE_CONFIG: Pubkey = pubkey!("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");

    // META
    pub const PUMPFUN_META: solana_sdk::instruction::AccountMeta =
        solana_sdk::instruction::AccountMeta {
            pubkey: PUMPFUN,
            is_signer: false,
            is_writable: false,
        };

    pub const EVENT_AUTHORITY_META: solana_sdk::instruction::AccountMeta =
        solana_sdk::instruction::AccountMeta {
            pubkey: EVENT_AUTHORITY,
            is_signer: false,
            is_writable: false,
        };

    pub const FEE_PROGRAM_META: solana_sdk::instruction::AccountMeta =
        solana_sdk::instruction::AccountMeta {
            pubkey: FEE_PROGRAM,
            is_signer: false,
            is_writable: false,
        };

    pub const GLOBAL_VOLUME_ACCUMULATOR_META: solana_sdk::instruction::AccountMeta =
        solana_sdk::instruction::AccountMeta {
            pubkey: GLOBAL_VOLUME_ACCUMULATOR,
            is_signer: false,
            is_writable: true,
        };

    pub const FEE_CONFIG_META: solana_sdk::instruction::AccountMeta =
        solana_sdk::instruction::AccountMeta {
            pubkey: FEE_CONFIG,
            is_signer: false,
            is_writable: false,
        };
}

pub fn get_user_volume_accumulator_pda(user: &Pubkey) -> Option<Pubkey> {
    get_cached_pda(
        PdaCacheKey::PumpFunUserVolume(*user),
        || {
            let seeds: &[&[u8]; 2] = &[seeds::USER_VOLUME_ACCUMULATOR_SEED, user.as_ref()];
            let program_id: &Pubkey = &accounts::PUMPFUN;
            let pda: Option<(Pubkey, u8)> = Pubkey::try_find_program_address(seeds, program_id);
            pda.map(|pubkey| pubkey.0)
        },
    )
}