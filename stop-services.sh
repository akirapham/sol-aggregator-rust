#!/bin/bash

# PM2 stop script for amm-eth and arbitrade-eth services

echo "🛑 Stopping Solana Aggregator Services..."

# Stop all PM2 processes
pm2 stop all 2>/dev/null || echo "No processes to stop"

# Delete all PM2 processes
pm2 delete all 2>/dev/null || echo "No processes to delete"

# Kill PM2 daemon
pm2 kill 2>/dev/null || echo "PM2 daemon not running"

echo "✅ All services stopped successfully!"

# Show any remaining processes
echo ""
echo "🔍 Checking for any remaining processes..."
ps aux | grep -E "(amm-eth|arbitrade-eth)" | grep -v grep || echo "No remaining processes found"