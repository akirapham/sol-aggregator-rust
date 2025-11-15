use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
pub const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";
pub fn wsol() -> Pubkey {
    Pubkey::from_str_const(WSOL_MINT)
}

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
