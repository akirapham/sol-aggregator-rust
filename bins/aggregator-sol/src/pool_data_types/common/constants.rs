use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
pub const TOKEN_PROGRAM: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: TOKEN_PROGRAM,
        is_signer: false,
        is_writable: false,
    };
pub const TOKEN_PROGRAM_2022: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const TOKEN_PROGRAM_2022_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: TOKEN_PROGRAM_2022,
        is_signer: false,
        is_writable: false,
    };
pub const SPL_MEMO_PROGRAM: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
pub const SPL_MEMO_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: SPL_MEMO_PROGRAM,
        is_signer: false,
        is_writable: false,
    };

pub const SYSTEM_PROGRAM: Pubkey = pubkey!("11111111111111111111111111111111");
pub const SYSTEM_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: SYSTEM_PROGRAM,
        is_signer: false,
        is_writable: false,
    };
pub const WSOL_TOKEN_ACCOUNT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const USDC_TOKEN_ACCOUNT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

