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

# Copy Cargo files first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY bins ./bins

# Build dependencies first (this layer caches dependencies)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin aggregator-sol && \
    cp /app/target/release/aggregator-sol /aggregator-sol

# Stage 2: Create the runtime image
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install runtime dependencies including curl for health checks
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
  curl \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /aggregator-sol /usr/local/bin/aggregator-sol

# Copy environment template
COPY .env.example /app/.env.example

# Run the application
ENTRYPOINT ["/usr/local/bin/aggregator-sol"]
