use crate::api::AppState;
use axum::{extract::State, http::StatusCode, response::Html};
use std::sync::Arc;

// This would be your AppState struct
// For now, we'll define what it should contain
pub struct DashboardState {
    pub arbitrage_monitor: Arc<crate::arbitrage_monitor::ArbitrageMonitor>,
    pub pool_manager: Arc<crate::pool_manager::PoolStateManager>,
}

/// GET /dashboard - Solana DEX Arbitrage Dashboard
pub async fn dashboard_page(
    State(_state): State<Arc<AppState>>,
) -> Result<Html<String>, (StatusCode, String)> {
    // TODO: Get stats from your database or in-memory state
    // For now, we'll render the HTML template
    let html = generate_dashboard_html();
    Ok(Html(html))
}

fn generate_dashboard_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Solana DEX Arbitrage Dashboard</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }

        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            background: linear-gradient(135deg, #14f195 0%, #9945ff 100%);
            min-height: 100vh;
            padding: 20px;
        }

        .container {
            max-width: 1600px;
            margin: 0 auto;
        }

        .header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 30px;
            flex-wrap: wrap;
            gap: 20px;
        }

        h1 {
            color: white;
            font-size: 2.5em;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.2);
            flex: 1;
            min-width: 300px;
        }

        .status-badge {
            background: rgba(255,255,255,0.2);
            color: white;
            padding: 10px 20px;
            border-radius: 20px;
            font-weight: 600;
            font-size: 0.95em;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .status-badge.active {
            background: #14f195;
            color: #000;
        }

        .status-badge.inactive {
            background: #ff6b6b;
            color: white;
        }

        .status-dot {
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            animation: pulse 2s infinite;
        }

        @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.5; }
        }

        .stats-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }

        .stat-card {
            background: rgba(255,255,255,0.95);
            border-radius: 12px;
            padding: 25px;
            box-shadow: 0 8px 16px rgba(0,0,0,0.15);
            transition: all 0.3s;
            border-top: 4px solid #14f195;
        }

        .stat-card:hover {
            transform: translateY(-5px);
            box-shadow: 0 12px 24px rgba(0,0,0,0.2);
        }

        .stat-card.warning {
            border-top-color: #ffc107;
        }

        .stat-card.danger {
            border-top-color: #ff6b6b;
        }

        .stat-label {
            color: #666;
            font-size: 0.85em;
            text-transform: uppercase;
            letter-spacing: 1px;
            margin-bottom: 10px;
            font-weight: 600;
        }

        .stat-value {
            font-size: 2.2em;
            font-weight: 900;
            color: #14f195;
            margin-bottom: 5px;
        }

        .stat-sublabel {
            color: #999;
            font-size: 0.8em;
        }

        .section {
            background: rgba(255,255,255,0.95);
            border-radius: 12px;
            padding: 25px;
            margin-bottom: 30px;
            box-shadow: 0 8px 16px rgba(0,0,0,0.15);
        }

        .section h2 {
            color: #333;
            margin-bottom: 20px;
            padding-bottom: 15px;
            border-bottom: 3px solid #14f195;
            display: flex;
            align-items: center;
            gap: 10px;
        }

        .section h3 {
            color: #555;
            margin: 20px 0 15px 0;
            font-size: 1.1em;
        }

        .grid-2 {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
        }

        .dex-card {
            background: #f8f9fa;
            border-radius: 8px;
            padding: 20px;
            border-left: 4px solid #14f195;
            transition: all 0.2s;
        }

        .dex-card:hover {
            transform: translateX(5px);
            box-shadow: 0 4px 12px rgba(0,0,0,0.1);
        }

        .dex-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 15px;
            padding-bottom: 10px;
            border-bottom: 1px solid #dee2e6;
        }

        .dex-name {
            font-size: 1.2em;
            font-weight: bold;
            color: #333;
        }

        .dex-pool-count {
            background: #14f195;
            color: #000;
            padding: 4px 12px;
            border-radius: 20px;
            font-size: 0.9em;
            font-weight: 600;
        }

        .dex-metrics {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 10px;
        }

        .metric {
            text-align: left;
        }

        .metric-label {
            color: #666;
            font-size: 0.8em;
            margin-bottom: 3px;
            text-transform: uppercase;
        }

        .metric-value {
            font-size: 1.3em;
            font-weight: 600;
            color: #333;
        }

        .metric-value.positive {
            color: #14f195;
        }

        .metric-value.negative {
            color: #ff6b6b;
        }

        .monitored-tokens {
            display: flex;
            flex-wrap: wrap;
            gap: 8px;
            margin-top: 15px;
        }

        .token-badge {
            background: white;
            border: 2px solid #14f195;
            color: #333;
            padding: 6px 12px;
            border-radius: 20px;
            font-size: 0.9em;
            font-weight: 600;
            display: flex;
            align-items: center;
            gap: 6px;
        }

        .token-badge.enabled {
            background: #14f195;
            color: #000;
        }

        .token-badge.disabled {
            opacity: 0.5;
            border-color: #ccc;
        }

        table {
            width: 100%;
            border-collapse: collapse;
            margin-top: 15px;
        }

        th {
            background: #f0f0f0;
            color: #333;
            font-weight: 600;
            text-align: left;
            padding: 12px;
            border-bottom: 2px solid #dee2e6;
            font-size: 0.9em;
            text-transform: uppercase;
        }

        td {
            padding: 12px;
            border-bottom: 1px solid #dee2e6;
        }

        tr:hover {
            background: #f8f9fa;
        }

        .token-address {
            font-family: 'Courier New', monospace;
            background: #f0f0f0;
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 0.85em;
            word-break: break-all;
            max-width: 300px;
            overflow: hidden;
            text-overflow: ellipsis;
        }

        .token-symbol {
            background: #14f195;
            color: #000;
            padding: 4px 8px;
            border-radius: 4px;
            font-weight: 600;
            font-size: 0.9em;
        }

        .price-comparison {
            display: grid;
            grid-template-columns: 1fr auto 1fr;
            align-items: center;
            gap: 10px;
            font-size: 0.9em;
        }

        .price-pool {
            background: #f0f0f0;
            padding: 8px 12px;
            border-radius: 6px;
            display: flex;
            flex-direction: column;
            gap: 4px;
        }

        .price-dex {
            font-weight: 600;
            color: #333;
        }

        .price-value {
            color: #666;
            font-family: 'Courier New', monospace;
        }

        .price-arrow {
            text-align: center;
            font-size: 1.2em;
            color: #14f195;
            font-weight: bold;
        }

        .profit-positive {
            color: #28a745;
            font-weight: bold;
        }

        .profit-negative {
            color: #dc3545;
            font-weight: bold;
        }

        .profit-neutral {
            color: #999;
        }

        .action-btn {
            padding: 6px 12px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.85em;
            transition: all 0.2s;
            white-space: nowrap;
            font-weight: 600;
        }

        .action-btn:hover {
            transform: scale(1.05);
        }

        .monitor-btn {
            background: #14f195;
            color: #000;
        }

        .monitor-btn:hover {
            background: #0dd185;
        }

        .remove-btn {
            background: #dc3545;
            color: white;
        }

        .remove-btn:hover {
            background: #c82333;
        }

        .update-btn {
            background: #007bff;
            color: white;
        }

        .update-btn:hover {
            background: #0056b3;
        }

        .opportunities-table {
            margin-top: 20px;
        }

        .opp-row {
            display: table-row;
        }

        .opp-row:hover {
            background: #f8f9fa;
        }

        .arbitrage-badge {
            background: #ffc107;
            color: #000;
            padding: 4px 12px;
            border-radius: 20px;
            font-weight: 600;
            font-size: 0.85em;
        }

        .no-data {
            text-align: center;
            padding: 40px 20px;
            color: #999;
        }

        .no-data-icon {
            font-size: 3em;
            margin-bottom: 15px;
            opacity: 0.5;
        }

        .controls {
            display: flex;
            gap: 10px;
            flex-wrap: wrap;
            margin-bottom: 20px;
        }

        .filter-select {
            padding: 8px 12px;
            border: 2px solid #14f195;
            border-radius: 6px;
            background: white;
            color: #333;
            font-weight: 600;
            cursor: pointer;
            transition: all 0.2s;
        }

        .filter-select:hover {
            background: #14f195;
            color: #000;
        }

        .refresh-btn {
            padding: 8px 16px;
            background: #14f195;
            color: #000;
            border: none;
            border-radius: 6px;
            font-weight: 600;
            cursor: pointer;
            transition: all 0.2s;
        }

        .refresh-btn:hover {
            transform: scale(1.05);
        }

        .refresh-btn.loading {
            animation: spin 1s linear infinite;
        }

        @keyframes spin {
            from { transform: rotate(0deg); }
            to { transform: rotate(360deg); }
        }

        .sidebar {
            position: fixed;
            right: 20px;
            top: 20px;
            background: rgba(255,255,255,0.95);
            border-radius: 12px;
            padding: 20px;
            width: 280px;
            box-shadow: 0 8px 16px rgba(0,0,0,0.15);
            max-height: 90vh;
            overflow-y: auto;
        }

        .sidebar-title {
            font-size: 1.1em;
            font-weight: bold;
            color: #333;
            margin-bottom: 15px;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .sidebar-item {
            padding: 12px;
            background: #f8f9fa;
            border-radius: 6px;
            margin-bottom: 10px;
            font-size: 0.9em;
            border-left: 3px solid #14f195;
        }

        .sidebar-item.active {
            background: #e7f9f5;
            font-weight: 600;
        }

        @media (max-width: 1200px) {
            .sidebar {
                position: static;
                width: 100%;
                max-height: none;
            }
        }

        footer {
            text-align: center;
            color: rgba(255,255,255,0.7);
            padding-top: 30px;
            font-size: 0.9em;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 Solana DEX Arbitrage Dashboard</h1>
            <div class="status-badge active">
                <span class="status-dot"></span>
                LIVE MONITORING
            </div>
        </div>

        <!-- Key Metrics -->
        <div class="stats-grid">
            <div class="stat-card">
                <div class="stat-label">Active Monitors</div>
                <div class="stat-value" id="active-monitors">25</div>
                <div class="stat-sublabel">tokens tracked</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">DEXes Supported</div>
                <div class="stat-value" id="dexes-count">4</div>
                <div class="stat-sublabel">Orca, Raydium, Phoenix, Marinade</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Opportunities Detected</div>
                <div class="stat-value" id="opportunities-count">0</div>
                <div class="stat-sublabel">this session</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Estimated Profit</div>
                <div class="stat-value" id="estimated-profit">$0.00</div>
                <div class="stat-sublabel">unrealized</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Best Opportunity</div>
                <div class="stat-value" id="best-profit" style="font-size: 1.8em; color: #28a745;">0.0%</div>
                <div class="stat-sublabel">profit margin</div>
            </div>

            <div class="stat-card warning">
                <div class="stat-label">Monitor Latency</div>
                <div class="stat-value" id="latency" style="color: #ffc107;">0ms</div>
                <div class="stat-sublabel">avg detection time</div>
            </div>
        </div>

        <!-- DEX Status -->
        <div class="section">
            <h2>📊 DEX Pool Status</h2>
            <div class="grid-2" id="dex-status">
                <div class="dex-card">
                    <div class="dex-header">
                        <span class="dex-name">🐋 Orca Whirlpool</span>
                        <span class="dex-pool-count">2.3k pools</span>
                    </div>
                    <div class="dex-metrics">
                        <div class="metric">
                            <div class="metric-label">Total Liquidity</div>
                            <div class="metric-value">$15.2M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">24h Volume</div>
                            <div class="metric-value">$287M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Mispriced Pools</div>
                            <div class="metric-value positive">3</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Spread Avg</div>
                            <div class="metric-value">0.12%</div>
                        </div>
                    </div>
                </div>

                <div class="dex-card">
                    <div class="dex-header">
                        <span class="dex-name">⚡ Raydium CLMM</span>
                        <span class="dex-pool-count">1.8k pools</span>
                    </div>
                    <div class="dex-metrics">
                        <div class="metric">
                            <div class="metric-label">Total Liquidity</div>
                            <div class="metric-value">$12.8M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">24h Volume</div>
                            <div class="metric-value">$456M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Mispriced Pools</div>
                            <div class="metric-value positive">5</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Spread Avg</div>
                            <div class="metric-value">0.08%</div>
                        </div>
                    </div>
                </div>

                <div class="dex-card">
                    <div class="dex-header">
                        <span class="dex-name">🔥 Phoenix</span>
                        <span class="dex-pool-count">1.2k pools</span>
                    </div>
                    <div class="dex-metrics">
                        <div class="metric">
                            <div class="metric-label">Total Liquidity</div>
                            <div class="metric-value">$8.5M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">24h Volume</div>
                            <div class="metric-value">$125M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Mispriced Pools</div>
                            <div class="metric-value positive">2</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Spread Avg</div>
                            <div class="metric-value">0.15%</div>
                        </div>
                    </div>
                </div>

                <div class="dex-card">
                    <div class="dex-header">
                        <span class="dex-name">🏦 Marinade</span>
                        <span class="dex-pool-count">856 pools</span>
                    </div>
                    <div class="dex-metrics">
                        <div class="metric">
                            <div class="metric-label">Total Liquidity</div>
                            <div class="metric-value">$6.2M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">24h Volume</div>
                            <div class="metric-value">$89M</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Mispriced Pools</div>
                            <div class="metric-value negative">1</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Spread Avg</div>
                            <div class="metric-value">0.22%</div>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Monitored Tokens -->
        <div class="section">
            <h2>🎯 Monitored Tokens</h2>
            <div class="controls">
                <button class="refresh-btn" onclick="refreshTokens()">🔄 Refresh</button>
                <select class="filter-select" id="token-filter" onchange="filterTokens()">
                    <option value="all">All Tokens</option>
                    <option value="enabled">Enabled Only</option>
                    <option value="lst">LST Tokens</option>
                    <option value="bluechip">Blue Chip</option>
                    <option value="meme">Meme Coins</option>
                </select>
            </div>

            <table id="monitored-tokens-table">
                <thead>
                    <tr>
                        <th>#</th>
                        <th>Token</th>
                        <th>Symbol</th>
                        <th>Address</th>
                        <th>Status</th>
                        <th>Category</th>
                        <th>Opportunities</th>
                        <th>Best Spread</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    <tr>
                        <td>1</td>
                        <td>Solana</td>
                        <td class="token-symbol">SOL</td>
                        <td><code class="token-address">So11111111111111111111111111111111111111112</code></td>
                        <td><span class="token-badge enabled">✅ Enabled</span></td>
                        <td>Blue Chip</td>
                        <td><span class="profit-positive">12</span></td>
                        <td><span class="profit-positive">0.18%</span></td>
                        <td>
                            <button class="action-btn remove-btn" onclick="removeToken('SOL')">Remove</button>
                        </td>
                    </tr>
                    <tr>
                        <td>2</td>
                        <td>Jupiter</td>
                        <td class="token-symbol">JUP</td>
                        <td><code class="token-address">JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN</code></td>
                        <td><span class="token-badge enabled">✅ Enabled</span></td>
                        <td>Blue Chip</td>
                        <td><span class="profit-positive">8</span></td>
                        <td><span class="profit-positive">0.22%</span></td>
                        <td>
                            <button class="action-btn remove-btn" onclick="removeToken('JUP')">Remove</button>
                        </td>
                    </tr>
                    <tr>
                        <td>3</td>
                        <td>Marinade SOL</td>
                        <td class="token-symbol">mSOL</td>
                        <td><code class="token-address">mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So</code></td>
                        <td><span class="token-badge enabled">✅ Enabled</span></td>
                        <td>LST</td>
                        <td><span class="profit-positive">5</span></td>
                        <td><span class="profit-neutral">0.05%</span></td>
                        <td>
                            <button class="action-btn remove-btn" onclick="removeToken('mSOL')">Remove</button>
                        </td>
                    </tr>
                    <tr>
                        <td>4</td>
                        <td>dogwifhat</td>
                        <td class="token-symbol">WIF</td>
                        <td><code class="token-address">EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm</code></td>
                        <td><span class="token-badge enabled">✅ Enabled</span></td>
                        <td>Meme</td>
                        <td><span class="profit-positive">3</span></td>
                        <td><span class="profit-positive">0.35%</span></td>
                        <td>
                            <button class="action-btn remove-btn" onclick="removeToken('WIF')">Remove</button>
                        </td>
                    </tr>
                    <tr>
                        <td>5</td>
                        <td>USDT (Bridged)</td>
                        <td class="token-symbol">USDT</td>
                        <td><code class="token-address">Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB</code></td>
                        <td><span class="token-badge disabled">❌ Disabled</span></td>
                        <td>Stablecoin</td>
                        <td><span class="profit-neutral">0</span></td>
                        <td><span class="profit-neutral">0.00%</span></td>
                        <td>
                            <button class="action-btn monitor-btn" onclick="enableToken('USDT')">Enable</button>
                        </td>
                    </tr>
                </tbody>
            </table>
        </div>

        <!-- Recent Arbitrage Opportunities -->
        <div class="section">
            <h2>💰 Recent Arbitrage Opportunities</h2>
            <div class="controls">
                <button class="refresh-btn" onclick="refreshOpportunities()">🔄 Refresh</button>
                <select class="filter-select" id="profit-filter" onchange="filterOpportunities()">
                    <option value="all">All</option>
                    <option value="positive">Profitable Only</option>
                    <option value="above-50bps">Above 50bps</option>
                    <option value="above-100bps">Above 100bps</option>
                </select>
            </div>

            <table class="opportunities-table">
                <thead>
                    <tr>
                        <th>Time</th>
                        <th>Token</th>
                        <th>Forward Route</th>
                        <th>Reverse Route</th>
                        <th>Forward Output</th>
                        <th>Reverse Output</th>
                        <th>Profit</th>
                        <th>Margin</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
                    <tr>
                        <td>2025-10-25 14:32:45</td>
                        <td class="token-symbol">SOL</td>
                        <td>
                            <div class="price-comparison">
                                <div class="price-pool">
                                    <span class="price-dex">Orca</span>
                                    <span class="price-value">100 USDC</span>
                                </div>
                                <div class="price-arrow">→</div>
                                <div class="price-pool">
                                    <span class="price-dex">Raydium</span>
                                    <span class="price-value">101 USDC</span>
                                </div>
                            </div>
                        </td>
                        <td>
                            <div class="price-comparison">
                                <div class="price-pool">
                                    <span class="price-dex">Raydium</span>
                                    <span class="price-value">101.5 USDC</span>
                                </div>
                                <div class="price-arrow">→</div>
                                <div class="price-pool">
                                    <span class="price-dex">Orca</span>
                                    <span class="price-value">102 USDC</span>
                                </div>
                            </div>
                        </td>
                        <td>9,900 SOL</td>
                        <td>9,803 SOL</td>
                        <td class="profit-positive">~$200</td>
                        <td class="profit-positive">0.18%</td>
                        <td><span class="arbitrage-badge">⏳ Pending</span></td>
                    </tr>
                    <tr>
                        <td>2025-10-25 14:31:22</td>
                        <td class="token-symbol">JUP</td>
                        <td>
                            <div class="price-comparison">
                                <div class="price-pool">
                                    <span class="price-dex">Marinade</span>
                                    <span class="price-value">2.45 USDC</span>
                                </div>
                                <div class="price-arrow">→</div>
                                <div class="price-pool">
                                    <span class="price-dex">Phoenix</span>
                                    <span class="price-value">2.48 USDC</span>
                                </div>
                            </div>
                        </td>
                        <td>
                            <div class="price-comparison">
                                <div class="price-pool">
                                    <span class="price-dex">Phoenix</span>
                                    <span class="price-value">2.50 USDC</span>
                                </div>
                                <div class="price-arrow">→</div>
                                <div class="price-pool">
                                    <span class="price-dex">Orca</span>
                                    <span class="price-value">2.51 USDC</span>
                                </div>
                            </div>
                        </td>
                        <td>40,816 JUP</td>
                        <td>39,840 JUP</td>
                        <td class="profit-positive">~$98</td>
                        <td class="profit-positive">0.22%</td>
                        <td><span class="arbitrage-badge">⏳ Pending</span></td>
                    </tr>
                </tbody>
            </table>

            <div id="no-opportunities" style="display: none;" class="no-data">
                <div class="no-data-icon">📭</div>
                <p>No arbitrage opportunities detected yet</p>
                <p style="font-size: 0.9em; margin-top: 10px;">Keep monitoring for price discrepancies between DEX pools</p>
            </div>
        </div>

        <footer>
            <p>🚀 Solana DEX Arbitrage Aggregator | Real-time monitoring | Powered by Event-Driven Architecture</p>
            <p style="margin-top: 10px; opacity: 0.7;">Last Updated: <span id="last-update">2025-10-25 14:35:00 UTC</span> | Uptime: <span id="uptime">12h 45m</span></p>
        </footer>
    </div>

    <script>
        // Auto-refresh data every 5 seconds
        setInterval(() => {
            refreshData();
        }, 5000);

        async function refreshData() {
            // TODO: Implement API calls to fetch live data
            console.log('Refreshing dashboard data...');

            // Update last update time
            const now = new Date().toLocaleString('en-US', {
                timeZone: 'UTC',
                year: 'numeric',
                month: '2-digit',
                day: '2-digit',
                hour: '2-digit',
                minute: '2-digit',
                second: '2-digit'
            });
            document.getElementById('last-update').textContent = now + ' UTC';
        }

        function refreshTokens() {
            const btn = event.target;
            btn.classList.add('loading');
            setTimeout(() => {
                btn.classList.remove('loading');
            }, 1000);
            console.log('Refreshing tokens...');
        }

        function refreshOpportunities() {
            const btn = event.target;
            btn.classList.add('loading');
            setTimeout(() => {
                btn.classList.remove('loading');
            }, 1000);
            console.log('Refreshing opportunities...');
        }

        function filterTokens() {
            const filter = document.getElementById('token-filter').value;
            console.log('Filtering tokens:', filter);
        }

        function filterOpportunities() {
            const filter = document.getElementById('profit-filter').value;
            console.log('Filtering opportunities:', filter);
        }

        function removeToken(symbol) {
            if (confirm(`Remove ${symbol} from monitoring?`)) {
                console.log('Removing token:', symbol);
                // TODO: Call API to remove token
            }
        }

        function enableToken(symbol) {
            console.log('Enabling token:', symbol);
            // TODO: Call API to enable token
        }
    </script>
</body>
</html>"#.to_string()
}
