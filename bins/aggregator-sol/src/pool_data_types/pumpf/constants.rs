use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub const FEE_RECIPIENT: Pubkey = pubkey!("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV");
pub const FEE_RECIPIENT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: FEE_RECIPIENT,
        is_signer: false,
        is_writable: false,
    };
pub const MAYHEM_FEE_RECIPIENT: Pubkey = pubkey!("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS");
pub const MAYHEM_FEE_RECIPIENT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: MAYHEM_FEE_RECIPIENT,
        is_signer: false,
        is_writable: false,
    };

// PumpFun constants
pub const PUMPFUN_GLOBAL_ACCOUNT: Pubkey = pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const PUMPFUN_GLOBAL_ACCOUNT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPFUN_GLOBAL_ACCOUNT,
        is_signer: false,
        is_writable: false,
    };

pub const PUMPFUN_EVENT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
pub const PUMPFUN_EVENT_AUTHORITY_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPFUN_EVENT_AUTHORITY,
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

pub const PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR: Pubkey =
    pubkey!("C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw");
pub const PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR,
        is_signer: false,
        is_writable: false,
    };

pub const PUMPFUN_FEE_CONFIG: Pubkey = pubkey!("5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx");
pub const PUMPFUN_FEE_CONFIG_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPFUN_FEE_CONFIG,
        is_signer: false,
        is_writable: false,
    };
pub const PUMPFUN_FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
pub const PUMPFUN_FEE_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPFUN_FEE_PROGRAM,
        is_signer: false,
        is_writable: false,
    };

// PumpSwap constants
pub const PUMPSWAP_GLOBAL_ACCOUNT: Pubkey = pubkey!("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw");
pub const PUMPSWAP_GLOBAL_ACCOUNT_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPSWAP_GLOBAL_ACCOUNT,
        is_signer: false,
        is_writable: false,
    };

pub const PUMPSWAP_EVENT_AUTHORITY: Pubkey =
    pubkey!("GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR");
pub const PUMPSWAP_EVENT_AUTHORITY_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPSWAP_EVENT_AUTHORITY,
        is_signer: false,
        is_writable: false,
    };

pub const PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR: Pubkey =
    pubkey!("C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw");
pub const PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR,
        is_signer: false,
        is_writable: false,
    };

pub const PUMPSWAP_FEE_CONFIG: Pubkey = pubkey!("5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx");
pub const PUMPSWAP_FEE_CONFIG_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPSWAP_FEE_CONFIG,
        is_signer: false,
        is_writable: false,
    };
pub const PUMPSWAP_FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
pub const PUMPSWAP_FEE_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPSWAP_FEE_PROGRAM,
        is_signer: false,
        is_writable: false,
    };
pub const PUMPSWAP_AMM_PROGRAM: Pubkey = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const PUMPSWAP_AMM_PROGRAM_META: solana_sdk::instruction::AccountMeta =
    solana_sdk::instruction::AccountMeta {
        pubkey: PUMPSWAP_AMM_PROGRAM,
        is_signer: false,
        is_writable: false,
    };
