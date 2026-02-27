use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
pub const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";

/// Pre-computed Pubkey constants — zero per-request parsing overhead
pub const WSOL_PUBKEY: Pubkey = Pubkey::from_str_const(WSOL_MINT);
pub const USDC_PUBKEY: Pubkey = Pubkey::from_str_const(USDC_MINT);
pub const USDT_PUBKEY: Pubkey = Pubkey::from_str_const(USDT_MINT);
pub const USD1_PUBKEY: Pubkey = Pubkey::from_str_const(USD1_MINT);

pub fn wsol() -> Pubkey {
    WSOL_PUBKEY
}

/// Pre-computed base token Pubkeys for fast iteration in routing
pub static BASE_TOKEN_PUBKEYS: LazyLock<Vec<Pubkey>> = LazyLock::new(|| {
    vec![WSOL_PUBKEY, USDC_PUBKEY, USDT_PUBKEY, USD1_PUBKEY]
});

pub static BASE_TOKENS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut set = HashSet::new();
    set.insert(WSOL_MINT.to_string());
    set.insert(USDC_MINT.to_string());
    set.insert(USDT_MINT.to_string());
    set.insert(USD1_MINT.to_string());
    set
});

pub fn is_base_token(mint: &str) -> bool {
    BASE_TOKENS.contains(mint)
}

pub fn get_base_token_symbol(mint: Pubkey) -> String {
    match mint.to_string().as_str() {
        WSOL_MINT => "WSOL".to_string(),
        USDC_MINT => "USDC".to_string(),
        USDT_MINT => "USDT".to_string(),
        USD1_MINT => "USD1".to_string(),
        _ => "UNKNOWN".to_string(),
    }
}
