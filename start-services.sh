#!/bin/bash

# PM2 startup script for amm-eth and arbitrade-eth services
# This script manages both the DEX listener and arbitrage detection services

set -e

echo "🚀 Starting Solana Aggregator Services with PM2..."

# Set working directory to project root (where Cargo.toml is located)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Check if we're already in the project root (script is in project root)
if [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
    PROJECT_ROOT="$SCRIPT_DIR"
# Otherwise, check parent directory
elif [ -f "$SCRIPT_DIR/../Cargo.toml" ]; then
    PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
else
    echo "❌ Error: Cannot find Cargo.toml in $SCRIPT_DIR or $SCRIPT_DIR/.."
    echo "Please ensure this script is located in the project root directory or one level down."
    exit 1
fi

cd "$PROJECT_ROOT"
echo "📁 Working directory: $PROJECT_ROOT"

# Load environment variables if .env exists
if [ -f ".env" ]; then
    echo "📄 Loading environment variables from .env"
    export $(grep -v '^#' .env | xargs)
fi

# Set default environment variables if not already set
export DEX_PRICE_STREAM="${DEX_PRICE_STREAM:-ws://localhost:8080}"
export ARBITRADE_PORT="${ARBITRADE_PORT:-3001}"
export MIN_PERCENT_DIFF="${MIN_PERCENT_DIFF:-2.0}"
export ARB_SIMULATION_USDT="${ARB_SIMULATION_USDT:-400.0}"
export ARB_COOLDOWN_SECS="${ARB_COOLDOWN_SECS:-3600}"

echo "🔧 Environment Configuration:"
echo "  DEX_PRICE_STREAM: $DEX_PRICE_STREAM"
echo "  ARBITRADE_PORT: $ARBITRADE_PORT"
echo "  MIN_PERCENT_DIFF: $MIN_PERCENT_DIFF"
echo "  ARB_SIMULATION_USDT: $ARB_SIMULATION_USDT"
echo "  ARB_COOLDOWN_SECS: $ARB_COOLDOWN_SECS"

# Function to check if a port is available
check_port() {
    local port=$1
    if lsof -Pi :$port -sTCP:LISTEN -t >/dev/null ; then
        echo "❌ Port $port is already in use"
        return 1
    else
        echo "✅ Port $port is available"
        return 0
    fi
}

# Function to extract port from websocket URL
get_port_from_ws_url() {
    local url=$1
    # Extract port from ws://localhost:PORT format
    echo "$url" | sed -n 's|.*:\([0-9]*\).*|\1|p'
}

# Check if required ports are available
echo "🔍 Checking port availability..."

# Extract port from DEX_PRICE_STREAM URL (default: 8080)
DEX_WS_PORT=$(get_port_from_ws_url "$DEX_PRICE_STREAM")
if [ -n "$DEX_WS_PORT" ]; then
    check_port "$DEX_WS_PORT" || echo "⚠️  Warning: DEX price stream port $DEX_WS_PORT may be in use"
else
    echo "⚠️  Warning: Could not extract port from DEX_PRICE_STREAM=$DEX_PRICE_STREAM"
fi

check_port $ARBITRADE_PORT || exit 1

# Build the Rust binaries if they don't exist
if [ ! -f "target/release/amm-eth" ]; then
    echo "🔨 Building amm-eth..."
    cargo build --release -p amm-eth
fi

if [ ! -f "target/release/arbitrade-eth" ]; then
    echo "🔨 Building arbitrade-eth..."
    cargo build --release -p arbitrade-eth
fi

# Stop any existing amm-eth and arbitrade-eth PM2 processes
echo "🛑 Stopping existing amm-eth and arbitrade-eth services..."
pm2 delete amm-eth 2>/dev/null || true
pm2 delete arbitrade-eth 2>/dev/null || true

# Start amm-eth service (DEX listener)
echo "📡 Starting amm-eth (DEX listener)..."
pm2 start ecosystem.config.js --only amm-eth 2>/dev/null || pm2 start target/release/amm-eth --name "amm-eth" \
    --log-date-format "YYYY-MM-DD HH:mm:ss Z" \
    --merge-logs \
    --log-file logs/amm-eth.log \
    --out-file logs/amm-eth-out.log \
    --error-file logs/amm-eth-error.log \
    --restart-delay 4000 \
    --max-restarts 10 \
    --env DEX_PRICE_STREAM="$DEX_PRICE_STREAM"

# Wait a moment for amm-eth to initialize
sleep 3

# Start arbitrade-eth service (arbitrage detection)
echo "💰 Starting arbitrade-eth (arbitrage detection)..."
pm2 start ecosystem.config.js --only arbitrade-eth 2>/dev/null || pm2 start target/release/arbitrade-eth --name "arbitrade-eth" \
    --log-date-format "YYYY-MM-DD HH:mm:ss Z" \
    --merge-logs \
    --log-file logs/arbitrade-eth.log \
    --out-file logs/arbitrade-eth-out.log \
    --error-file logs/arbitrade-eth-error.log \
    --restart-delay 4000 \
    --max-restarts 10 \
    --env ARBITRADE_PORT="$ARBITRADE_PORT" \
    --env MIN_PERCENT_DIFF="$MIN_PERCENT_DIFF" \
    --env ARB_SIMULATION_USDT="$ARB_SIMULATION_USDT" \
    --env ARB_COOLDOWN_SECS="$ARB_COOLDOWN_SECS"

# Wait for services to start
sleep 5

# Display PM2 status
echo "📊 PM2 Status:"
pm2 list

# Display logs location
echo ""
echo "📝 Log files location:"
echo "  amm-eth: logs/amm-eth.log"
echo "  arbitrade-eth: logs/arbitrade-eth.log"
echo ""
echo "🔍 View logs with:"
echo "  pm2 logs amm-eth"
echo "  pm2 logs arbitrade-eth"
echo "  pm2 logs"
echo ""
echo "🛑 Stop services with:"
echo "  pm2 stop all"
echo "  pm2 delete all"
echo ""
echo "✅ Services started successfully!"
echo "🌐 Arbitrage API available at: http://localhost:$ARBITRADE_PORT/api/"
