use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub const FEE_RECIPIENT: Pubkey = pubkey!("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV");
pub const FEE_RECIPIENT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: FEE_RECIPIENT,
        is_signer: false,
        is_writable: true,
    };
pub const MAYHEM_FEE_RECIPIENT: Pubkey = pubkey!("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS");
pub const MAYHEM_FEE_RECIPIENT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: MAYHEM_FEE_RECIPIENT,
        is_signer: false,
        is_writable: true,
    };
pub const GLOBAL_ACCOUNT: Pubkey = pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const GLOBAL_ACCOUNT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: GLOBAL_ACCOUNT,
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
pub const EVENT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
pub const EVENT_AUTHORITY_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: EVENT_AUTHORITY,
        is_signer: false,
        is_writable: false,
    };
pub const PUMPFUN: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMPFUN_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPFUN,
        is_signer: false,
        is_writable: false,
    };
pub const GLOBAL_VOLUME_ACCUMULATOR: Pubkey =
    pubkey!("Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y");
pub const GLOBAL_VOLUME_ACCUMULATOR_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: GLOBAL_VOLUME_ACCUMULATOR,
        is_signer: false,
        is_writable: true,
    };
pub const FEE_CONFIG: Pubkey = pubkey!("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");
pub const FEE_CONFIG_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: FEE_CONFIG,
        is_signer: false,
        is_writable: false,
    };
pub const FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
pub const FEE_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: FEE_PROGRAM,
        is_signer: false,
        is_writable: false,
    };

pub const WSOL_TOKEN_ACCOUNT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const WSOL_TOKEN_ACCOUNT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: WSOL_TOKEN_ACCOUNT,
        is_signer: false,
        is_writable: false,
    };
pub const USDC_TOKEN_ACCOUNT: Pubkey =
pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
pub const USDC_TOKEN_ACCOUNT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: USDC_TOKEN_ACCOUNT,
        is_signer: false,
        is_writable: false,
    };
pub const DEFAULT_COIN_CREATOR_VAULT_AUTHORITY: Pubkey = 
    pubkey!("8N3GDaZ2iwN65oxVatKTLPNooAVUJTbfiVJ1ahyqwjSk");
pub const ASSOCIATED_TOKEN_PROGRAM: Pubkey =
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const ASSOCIATED_TOKEN_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: ASSOCIATED_TOKEN_PROGRAM,
        is_signer: false,
        is_writable: false,
    };
pub const AMM_PROGRAM: Pubkey = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const AMM_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: AMM_PROGRAM,
        is_signer: false,
        is_writable: false,
        };
