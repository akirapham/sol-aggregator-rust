use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use crate::arbitrage_api::AppState;

/// GET /dashboard - Main dashboard page
pub async fn dashboard_page(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, (StatusCode, String)> {
    // Get stats from database
    let stats = state.db.get_stats().map_err(|e| {
        log::error!("Failed to get stats: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    // Get top opportunities
    let top_opportunities = state.db.get_top_opportunities(10).map_err(|e| {
        log::error!("Failed to get top opportunities: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    // Get recent opportunities
    let recent_opportunities = state.db.get_opportunities(None, Some(20)).map_err(|e| {
        log::error!("Failed to get recent opportunities: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    // Get blacklist
    let blacklist: Vec<String> = state
        .blacklist
        .iter()
        .map(|entry| entry.key().clone())
        .collect();

    let html = generate_dashboard_html(
        &stats,
        &top_opportunities,
        &recent_opportunities,
        &blacklist,
    );

    Ok(Html(html))
}

fn generate_dashboard_html(
    stats: &crate::db::DbStats,
    top_opportunities: &[crate::db::ArbitrageOpportunity],
    recent_opportunities: &[crate::db::ArbitrageOpportunity],
    blacklist: &[String],
) -> String {
    let top_opps_rows: String = top_opportunities
        .iter()
        .enumerate()
        .map(|(i, opp)| {
            format!(
                r#"
                <tr>
                    <td>{}</td>
                    <td><code class="token-address">{}</code></td>
                    <td>{}</td>
                    <td>{}</td>
                    <td class="profit-positive">${:.6}</td>
                    <td>{:.4}%</td>
                    <td>Buy DEX @ ${:.6} → Sell CEX @ ${:.6}</td>
                    <td>
                        <button class="action-btn blacklist-btn" onclick="blacklistToken('{}')">🚫 Blacklist</button>
                        <button class="action-btn delete-btn" onclick="deleteTrades('{}')">🗑️ Delete</button>
                    </td>
                </tr>
                "#,
                i + 1,
                opp.token_address,
                opp.cex_name,
                format_timestamp(opp.timestamp),
                opp.profit_usdt,
                opp.profit_percent,
                opp.dex_price,
                opp.cex_price,
                opp.token_address,
                opp.token_address
            )
        })
        .collect();

    let recent_opps_rows: String = recent_opportunities
        .iter()
        .map(|opp| {
            let profit_class = if opp.profit_usdt > 0.0 {
                "profit-positive"
            } else {
                "profit-negative"
            };
            format!(
                r#"
                <tr>
                    <td>{}</td>
                    <td><code class="token-address">{}</code></td>
                    <td>{}</td>
                    <td class="{}">${:.6}</td>
                    <td>{:.4}%</td>
                    <td>Buy DEX @ ${:.6} → Sell CEX @ ${:.6}</td>
                    <td>
                        <button class="action-btn blacklist-btn" onclick="blacklistToken('{}')">🚫 Blacklist</button>
                        <button class="action-btn delete-btn" onclick="deleteTrades('{}')">🗑️ Delete</button>
                    </td>
                </tr>
                "#,
                format_timestamp(opp.timestamp),
                opp.token_address,
                opp.cex_name,
                profit_class,
                opp.profit_usdt,
                opp.profit_percent,
                opp.dex_price,
                opp.cex_price,
                opp.token_address,
                opp.token_address
            )
        })
        .collect();

    let blacklist_items: String = if blacklist.is_empty() {
        "<li class=\"empty-message\">No blacklisted addresses</li>".to_string()
    } else {
        blacklist
            .iter()
            .map(|addr| {
                format!(
                    r#"<li>
                        <code class="token-address">{}</code>
                        <button class="action-btn remove-btn" onclick="removeFromBlacklist('{}')">✖️ Remove</button>
                    </li>"#,
                    addr, addr
                )
            })
            .collect()
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Arbitrage Dashboard</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}

        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
            padding: 20px;
        }}

        .container {{
            max-width: 1400px;
            margin: 0 auto;
        }}

        h1 {{
            color: white;
            text-align: center;
            margin-bottom: 30px;
            font-size: 2.5em;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.2);
        }}

        .stats-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}

        .stat-card {{
            background: white;
            border-radius: 12px;
            padding: 25px;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
            transition: transform 0.2s;
        }}

        .stat-card:hover {{
            transform: translateY(-5px);
            box-shadow: 0 6px 12px rgba(0,0,0,0.15);
        }}

        .stat-label {{
            color: #666;
            font-size: 0.9em;
            text-transform: uppercase;
            letter-spacing: 1px;
            margin-bottom: 10px;
        }}

        .stat-value {{
            color: #333;
            font-size: 2em;
            font-weight: bold;
        }}

        .stat-value.large {{
            font-size: 2.5em;
            color: #667eea;
        }}

        .section {{
            background: white;
            border-radius: 12px;
            padding: 25px;
            margin-bottom: 30px;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
        }}

        .section h2 {{
            color: #333;
            margin-bottom: 20px;
            padding-bottom: 10px;
            border-bottom: 2px solid #667eea;
        }}

        table {{
            width: 100%;
            border-collapse: collapse;
        }}

        th {{
            background: #f8f9fa;
            color: #333;
            font-weight: 600;
            text-align: left;
            padding: 12px;
            border-bottom: 2px solid #dee2e6;
        }}

        td {{
            padding: 12px;
            border-bottom: 1px solid #dee2e6;
        }}

        tr:hover {{
            background: #f8f9fa;
        }}

        .token-address {{
            font-family: 'Courier New', monospace;
            background: #f0f0f0;
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 0.85em;
            word-break: break-all;
        }}

        .profit-positive {{
            color: #28a745;
            font-weight: bold;
        }}

        .profit-negative {{
            color: #dc3545;
            font-weight: bold;
        }}

        .blacklist-list {{
            list-style: none;
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
            gap: 10px;
        }}

        .blacklist-list li {{
            background: #f8f9fa;
            padding: 10px;
            border-radius: 6px;
            border-left: 4px solid #dc3545;
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 10px;
        }}

        .empty-message {{
            color: #999;
            font-style: italic;
            text-align: center;
            padding: 20px;
            border-left: none !important;
        }}

        .action-btn {{
            padding: 6px 12px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.85em;
            transition: all 0.2s;
            white-space: nowrap;
        }}

        .blacklist-btn {{
            background: #ffc107;
            color: #000;
        }}

        .blacklist-btn:hover {{
            background: #e0a800;
        }}

        .delete-btn {{
            background: #dc3545;
            color: white;
        }}

        .delete-btn:hover {{
            background: #c82333;
        }}

        .remove-btn {{
            background: #6c757d;
            color: white;
        }}

        .remove-btn:hover {{
            background: #5a6268;
        }}

        .manual-form {{
            background: #f8f9fa;
            padding: 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}

        .manual-form h3 {{
            margin-bottom: 15px;
            color: #333;
        }}

        .form-group {{
            display: flex;
            gap: 10px;
            align-items: center;
            flex-wrap: wrap;
        }}

        .form-group input {{
            flex: 1;
            min-width: 300px;
            padding: 10px;
            border: 1px solid #ddd;
            border-radius: 4px;
            font-family: 'Courier New', monospace;
        }}

        .form-group button {{
            padding: 10px 20px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-weight: 600;
            transition: background 0.2s;
        }}

        .add-btn {{
            background: #28a745;
            color: white;
        }}

        .add-btn:hover {{
            background: #218838;
        }}

        .notification {{
            position: fixed;
            top: 20px;
            right: 20px;
            padding: 15px 20px;
            border-radius: 8px;
            color: white;
            font-weight: 600;
            box-shadow: 0 4px 6px rgba(0,0,0,0.2);
            z-index: 1000;
            animation: slideIn 0.3s ease-out;
        }}

        .notification.success {{
            background: #28a745;
        }}

        .notification.error {{
            background: #dc3545;
        }}

        @keyframes slideIn {{
            from {{
                transform: translateX(400px);
                opacity: 0;
            }}
            to {{
                transform: translateX(0);
                opacity: 1;
            }}
        }}

        .refresh-btn {{
            background: #667eea;
            color: white;
            border: none;
            padding: 10px 20px;
            border-radius: 6px;
            cursor: pointer;
            font-size: 1em;
            margin-bottom: 20px;
            transition: background 0.2s;
        }}

        .refresh-btn:hover {{
            background: #5568d3;
        }}

        .timestamp {{
            color: #666;
            font-size: 0.9em;
        }}

        @media (max-width: 768px) {{
            h1 {{
                font-size: 1.8em;
            }}

            .stats-grid {{
                grid-template-columns: 1fr;
            }}

            table {{
                font-size: 0.85em;
            }}

            .section {{
                overflow-x: auto;
            }}
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🚀 Arbitrage Dashboard</h1>

        <button class="refresh-btn" onclick="location.reload()">🔄 Refresh</button>

        <div class="stats-grid">
            <div class="stat-card">
                <div class="stat-label">Total Opportunities</div>
                <div class="stat-value large">{}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Unique Tokens</div>
                <div class="stat-value">{}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Total Profit (USD)</div>
                <div class="stat-value profit-positive">${:.2}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Avg Profit (USD)</div>
                <div class="stat-value">${:.4}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Max Profit (USD)</div>
                <div class="stat-value profit-positive">${:.2}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Blacklisted</div>
                <div class="stat-value">{}</div>
            </div>
        </div>

        <div class="section">
            <h2>📊 Top 10 Most Profitable Opportunities</h2>
            <table>
                <thead>
                    <tr>
                        <th>#</th>
                        <th>Token Address</th>
                        <th>CEX</th>
                        <th>Time</th>
                        <th>Profit (USD)</th>
                        <th>Profit %</th>
                        <th>Route</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
        </div>

        <div class="section">
            <h2>⏰ Recent Opportunities (Last 20)</h2>
            <table>
                <thead>
                    <tr>
                        <th>Time</th>
                        <th>Token Address</th>
                        <th>CEX</th>
                        <th>Profit (USD)</th>
                        <th>Profit %</th>
                        <th>Route</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
        </div>

        <div class="section">
            <h2>🚫 Blacklisted Addresses</h2>
            <div class="manual-form">
                <h3>Add Address to Blacklist</h3>
                <div class="form-group">
                    <input type="text" id="blacklistInput" placeholder="Enter token address (0x...)" />
                    <button class="add-btn" onclick="addToBlacklistManual()">➕ Add to Blacklist</button>
                </div>
            </div>
            <ul class="blacklist-list">
                {}
            </ul>
        </div>
    </div>

    <script>
        // Get auth token from cookie or localStorage
        function getAuthToken() {{
            // Try to get from cookie first
            const cookies = document.cookie.split(';');
            for (let cookie of cookies) {{
                const [name, value] = cookie.trim().split('=');
                if (name === 'auth_token') {{
                    return value;
                }}
            }}
            // Fallback to localStorage
            return localStorage.getItem('auth_token');
        }}

        // Show notification
        function showNotification(message, type = 'success') {{
            const notification = document.createElement('div');
            notification.className = `notification ${{type}}`;
            notification.textContent = message;
            document.body.appendChild(notification);

            setTimeout(() => {{
                notification.remove();
            }}, 3000);
        }}

        // Blacklist a token
        async function blacklistToken(address) {{
            if (!confirm(`Are you sure you want to blacklist token ${{address}}?`)) {{
                return;
            }}

            try {{
                const response = await fetch('/api/blacklist', {{
                    method: 'POST',
                    headers: {{
                        'Content-Type': 'application/json',
                    }},
                    body: JSON.stringify({{ address: address }})
                }});

                const data = await response.json();

                if (response.ok) {{
                    showNotification(data.message || 'Token blacklisted successfully!', 'success');
                    setTimeout(() => location.reload(), 1000);
                }} else {{
                    showNotification(data.error || 'Failed to blacklist token', 'error');
                }}
            }} catch (error) {{
                showNotification('Error: ' + error.message, 'error');
            }}
        }}

        // Delete all trades for a token
        async function deleteTrades(address) {{
            if (!confirm(`Are you sure you want to delete ALL trades for token ${{address}}? This cannot be undone!`)) {{
                return;
            }}

            try {{
                const response = await fetch('/api/opportunities/token', {{
                    method: 'DELETE',
                    headers: {{
                        'Content-Type': 'application/json',
                    }},
                    body: JSON.stringify({{ token_address: address }})
                }});

                const data = await response.json();

                if (response.ok) {{
                    showNotification(data.message || `Deleted ${{data.deleted_count}} trade(s)`, 'success');
                    setTimeout(() => location.reload(), 1000);
                }} else {{
                    showNotification(data.error || 'Failed to delete trades', 'error');
                }}
            }} catch (error) {{
                showNotification('Error: ' + error.message, 'error');
            }}
        }}

        // Remove from blacklist
        async function removeFromBlacklist(address) {{
            if (!confirm(`Remove ${{address}} from blacklist?`)) {{
                return;
            }}

            try {{
                const response = await fetch('/api/blacklist', {{
                    method: 'DELETE',
                    headers: {{
                        'Content-Type': 'application/json',
                    }},
                    body: JSON.stringify({{ address: address }})
                }});

                const data = await response.json();

                if (response.ok) {{
                    showNotification(data.message || 'Removed from blacklist', 'success');
                    setTimeout(() => location.reload(), 1000);
                }} else {{
                    showNotification(data.error || 'Failed to remove from blacklist', 'error');
                }}
            }} catch (error) {{
                showNotification('Error: ' + error.message, 'error');
            }}
        }}

        // Add to blacklist manually
        async function addToBlacklistManual() {{
            const input = document.getElementById('blacklistInput');
            const address = input.value.trim();

            if (!address) {{
                showNotification('Please enter a token address', 'error');
                return;
            }}

            if (!address.startsWith('0x') || address.length !== 42) {{
                showNotification('Invalid Ethereum address format', 'error');
                return;
            }}

            await blacklistToken(address);
            input.value = '';
        }}

        // Allow Enter key to submit
        document.getElementById('blacklistInput').addEventListener('keypress', function(e) {{
            if (e.key === 'Enter') {{
                addToBlacklistManual();
            }}
        }});
    </script>
</body>
</html>"#,
        stats.total_opportunities,
        stats.unique_tokens,
        stats.total_profit_usdt,
        stats.average_profit_usdt,
        stats.max_profit_usdt,
        blacklist.len(),
        top_opps_rows,
        recent_opps_rows,
        blacklist_items
    )
}

fn format_timestamp(timestamp: i64) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let d = UNIX_EPOCH + Duration::from_secs(timestamp as u64);
    let datetime = SystemTime::from(d);

    // Simple formatting - you can enhance this
    match datetime.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let secs = duration.as_secs();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let diff = now.saturating_sub(secs);

            if diff < 60 {
                format!("{}s ago", diff)
            } else if diff < 3600 {
                format!("{}m ago", diff / 60)
            } else if diff < 86400 {
                format!("{}h ago", diff / 3600)
            } else {
                format!("{}d ago", diff / 86400)
            }
        }
        Err(_) => "Unknown".to_string(),
    }
}
