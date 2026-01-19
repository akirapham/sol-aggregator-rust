use crate::constants::is_base_token;
use crate::pool_data_types::common::constants;
use solana_program::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;

/// Create an idempotent ATA (Associated Token Account) creation instruction
///
/// # Arguments
/// * `user_wallet` - The wallet that will own and fund the ATA
/// * `token_account` - The ATA address to create
/// * `mint` - The token mint address
/// * `is_token_2022` - Whether this is a Token-2022 token
///
/// # Returns
/// An instruction that creates the ATA if it doesn't exist (idempotent)
pub fn create_ata_instruction(
    user_wallet: Pubkey,
    token_account: Pubkey,
    mint: Pubkey,
    is_token_2022: bool,
) -> Instruction {
    let token_program_meta = if is_token_2022 {
        constants::TOKEN_PROGRAM_2022_META
    } else {
        constants::TOKEN_PROGRAM_META
    };

    let accounts = vec![
        AccountMeta::new(user_wallet, true),           // funding
        AccountMeta::new(token_account, false),        // associated_token
        AccountMeta::new_readonly(user_wallet, false), // wallet
        AccountMeta::new_readonly(mint, false),        // mint
        constants::SYSTEM_PROGRAM_META,                // system_program
        token_program_meta,                            // token_program
    ];

    let spl_associated_token_account_program_id =
        Pubkey::new_from_array(spl_associated_token_account::id().to_bytes());

    Instruction {
        program_id: spl_associated_token_account_program_id,
        accounts,
        data: vec![1], // Idempotent instruction discriminator
    }
}

/// Convert solana_sdk::Pubkey to anchor_lang::Pubkey
#[inline]
pub fn to_pubkey(pubkey: &Pubkey) -> anchor_lang::prelude::Pubkey {
    anchor_lang::prelude::Pubkey::new_from_array(pubkey.to_bytes())
}

/// Convert anchor_lang::Pubkey to solana_sdk::Pubkey
#[inline]
pub fn to_address(pubkey: &anchor_lang::prelude::Pubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}

/// Get the appropriate token program based on whether it's Token-2022 or SPL Token
#[inline]
pub fn get_token_program(is_token_2022: bool) -> Pubkey {
    if is_token_2022 {
        constants::TOKEN_PROGRAM_2022
    } else {
        constants::TOKEN_PROGRAM
    }
}

/// Get the appropriate token program AccountMeta based on whether it's Token-2022 or SPL Token
#[inline]
pub fn get_token_program_meta(is_token_2022: bool) -> AccountMeta {
    if is_token_2022 {
        constants::TOKEN_PROGRAM_2022_META
    } else {
        constants::TOKEN_PROGRAM_META
    }
}

/// Calculate minimum output amount with slippage protection
///
/// # Arguments
/// * `amount_out` - The expected output amount without slippage
/// * `slippage_bps` - Slippage tolerance in basis points (100 bps = 1%)
///
/// # Returns
/// The minimum acceptable output amount after applying slippage
#[inline]
pub fn calculate_slippage(amount_out: u64, slippage_bps: u16) -> Result<u64, String> {
    let slippage_factor = 10000 - slippage_bps as u64;
    let min_out = (amount_out as u128 * slippage_factor as u128 / 10000) as u64;

    if min_out == 0 {
        return Err("Minimum output amount is zero after slippage".to_string());
    }

    Ok(min_out)
}

/// Calculate token prices for constant product AMM pools
///
/// This function implements the standard constant product formula (x * y = k)
/// price calculation used by AMM DEXs like Raydium AMM V4, Raydium CPMM, and PumpSwap.
///
/// # Arguments
/// * `base_mint` - The base token mint address
/// * `quote_mint` - The quote token mint address  
/// * `base_reserve` - The base token reserve amount
/// * `quote_reserve` - The quote token reserve amount
/// * `sol_price` - Current SOL price in USD
/// * `base_decimals` - Decimals for the base token
/// * `quote_decimals` - Decimals for the quote token
///
/// # Returns
/// A tuple of (base_price_usd, quote_price_usd)
pub fn calculate_amm_token_prices(
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    base_reserve: u64,
    quote_reserve: u64,
    sol_price: f64,
    base_decimals: u8,
    quote_decimals: u8,
) -> (f64, f64) {
    if quote_reserve == 0 || base_reserve == 0 {
        return (0.0, 0.0);
    }

    let base_token_str = base_mint.to_string();
    let quote_token_str = quote_mint.to_string();

    let is_base_a_base_token = is_base_token(&base_token_str);
    let is_quote_a_base_token = is_base_token(&quote_token_str);

    let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);

    // If quote is a base token (like USDC, SOL), use its price
    if is_quote_a_base_token {
        let quote_price = if quote_token_str == "So11111111111111111111111111111111111111112" {
            sol_price // SOL
        } else {
            1.0 // Assume USDC/USDT are ~$1
        };

        let base_price = (quote_reserve as f64 / base_reserve as f64) * decimal_scale * quote_price;
        (base_price, quote_price)
    } else if is_base_a_base_token {
        // If base is a base token, use its price
        let base_price = if base_token_str == "So11111111111111111111111111111111111111112" {
            sol_price // SOL
        } else {
            1.0 // Assume USDC/USDT are ~$1
        };

        let quote_price =
            (base_reserve as f64 / quote_reserve as f64) * (1.0 / decimal_scale) * base_price;
        (base_price, quote_price)
    } else {
        // Neither token is a base token, assume relative pricing
        let base_price = (quote_reserve as f64 / base_reserve as f64) * decimal_scale * 1.0;
        (base_price, 1.0)
    }
}
