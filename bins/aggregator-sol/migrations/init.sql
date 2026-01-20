-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Table for storing token metadata
CREATE TABLE IF NOT EXISTS tokens (
    address VARCHAR(44) PRIMARY KEY,
    symbol VARCHAR(32),
    name VARCHAR(64),
    decimals SMALLINT NOT NULL,
    is_token2022 BOOLEAN NOT NULL DEFAULT FALSE,
    logo_uri TEXT,
    data JSONB, -- Full serialized token data
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Table for storing pool state
CREATE TABLE IF NOT EXISTS pools (
    address VARCHAR(44) PRIMARY KEY,
    dex_type VARCHAR(32) NOT NULL,
    token_a VARCHAR(44) NOT NULL REFERENCES tokens(address),
    token_b VARCHAR(44) NOT NULL REFERENCES tokens(address),
    data JSONB NOT NULL, -- Full serialized PoolState
    last_updated_ts BIGINT NOT NULL, -- On-chain timestamp/slot
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for efficient pool lookups
CREATE INDEX IF NOT EXISTS idx_pools_dex_type ON pools(dex_type);
CREATE INDEX IF NOT EXISTS idx_pools_token_a ON pools(token_a);
CREATE INDEX IF NOT EXISTS idx_pools_token_b ON pools(token_b);

-- Table for storing arbitrage opportunities
CREATE TABLE IF NOT EXISTS arbitrage_opportunities (
    id BIGSERIAL PRIMARY KEY,
    pair_name VARCHAR(64) NOT NULL,
    token_a VARCHAR(44) NOT NULL,
    token_b VARCHAR(44) NOT NULL,
    profit_amount BIGINT NOT NULL,
    profit_percent DOUBLE PRECISION NOT NULL,
    input_amount BIGINT NOT NULL,
    detected_at BIGINT NOT NULL, -- UNIX timestamp of detection
    execution_status VARCHAR(16) NOT NULL, -- 'NotYet', 'Success', 'Fail'
    error_message TEXT,
    details JSONB, -- Stores full opportunity struct including routes
    is_abnormal BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for arbitrage analytics
CREATE INDEX IF NOT EXISTS idx_arb_detected_at ON arbitrage_opportunities(detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_arb_status ON arbitrage_opportunities(execution_status);
CREATE INDEX IF NOT EXISTS idx_arb_pair_name ON arbitrage_opportunities(pair_name);
