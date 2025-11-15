#!/bin/bash

# PM2 stop script for amm-eth and arbitrade-eth services

echo "🛑 Stopping Solana Aggregator Services..."

# Stop specific amm-eth and arbitrade-eth PM2 processes
echo "🛑 Stopping amm-eth and arbitrade-eth services..."
pm2 stop amm-eth 2>/dev/null || echo "amm-eth service not running"
pm2 stop arbitrade-eth 2>/dev/null || echo "arbitrade-eth service not running"

# Delete specific PM2 processes
pm2 delete amm-eth 2>/dev/null || echo "amm-eth process not found"
pm2 delete arbitrade-eth 2>/dev/null || echo "arbitrade-eth process not found"

echo "✅ All services stopped successfully!"

# Show any remaining processes
echo ""
echo "🔍 Checking for any remaining processes..."
ps aux | grep -E "(amm-eth|arbitrade-eth)" | grep -v grep || echo "No remaining processes found"
