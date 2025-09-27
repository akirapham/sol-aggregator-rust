# Multi-stage Docker build for Solana DEX Aggregator
# Stage 1: Build the application
FROM rust:1.90 AS builder

# Install system dependencies (removed librocksdb-dev since we're using bundled RocksDB)
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
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /usr/src/app

# Copy workspace files first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Copy crate manifests for dependency caching
COPY crates/solana-streamer/Cargo.toml ./crates/solana-streamer/
COPY aggregator/Cargo.toml ./aggregator/

# Create dummy source files to cache dependencies
RUN mkdir -p crates/solana-streamer/src aggregator/src && \
    echo "fn main() {}" > aggregator/src/main.rs && \
    echo "// dummy" > crates/solana-streamer/src/lib.rs

# Copy all source code
COPY crates/ ./crates/
COPY aggregator/ ./aggregator/

# Build the application in release mode
RUN cargo build --release --package aggregator

# Stage 2: Create the runtime image
FROM ubuntu:24.04

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
COPY --from=builder /usr/src/app/target/release/aggregator /app/aggregator

# Copy environment template (will be overridden by mounted .env)
COPY .env.example /app/.env.example

# Change ownership
RUN chown -R aggregator:aggregator /app

# Switch to non-root user
USER aggregator

# Run the application
CMD ["./aggregator"]
