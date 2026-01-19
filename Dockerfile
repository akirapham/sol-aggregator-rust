# Multi-stage Docker build for Solana DEX Aggregator with optimized caching
# Stage 1: Build the application
FROM rust:1.90 AS builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    git \
    clang \
    cmake \
    libgflags-dev \
    libsnappy-dev \
    zlib1g-dev \
    libbz2-dev \
    liblz4-dev \
  libzstd-dev \
  ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /usr/src/app

# Copy workspace root Cargo files first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy all crate Cargo.toml files (preserved for build order)
COPY crates/ ./crates/
COPY aggregator-sol/ ./aggregator-sol/
COPY arbitrade/ ./arbitrade/
COPY amm-eth/ ./amm-eth/
COPY arbitrade-eth/ ./arbitrade-eth/
COPY arbitrade-dex-eth/ ./arbitrade-dex-eth/

# Build with layer caching for dependencies
# This will reuse the cargo registry and build cache if only source code changes
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/src/app/target \
  cargo build --release --package aggregator-sol --bin aggregator-sol && \
  cp /usr/src/app/target/release/aggregator-sol /aggregator-sol

# Stage 2: Create the runtime image
FROM ubuntu:24.04 AS runtime

# Install runtime dependencies including curl for health checks
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
  curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -r -u 1001 -m -c "aggregator user" -s /bin/bash aggregator && \
    mkdir -p /app && \
    chown -R aggregator:aggregator /app

# Set working directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /aggregator-sol /usr/local/bin/aggregator-sol

# Copy environment template
COPY .env.example /app/.env.example

# Change ownership
RUN chown -R aggregator:aggregator /app

# Switch to non-root user
USER aggregator

# Run the application
ENTRYPOINT ["/usr/local/bin/aggregator-sol"]
