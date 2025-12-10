use crate::api::AppState;
use axum::{extract::State, response::Html};
use std::sync::Arc;

/// GET /dashboard - Solana DEX Arbitrage Dashboard
pub async fn dashboard_page(State(state): State<Arc<AppState>>) -> Html<String> {
    let html = generate_dashboard_html(&state).await;
    Html(html)
}

async fn generate_dashboard_html(state: &AppState) -> String {
    // Get real data from state
    let pool_stats = state.aggregator.get_pool_manager().get_stats().await;
    let arb_config = state.arbitrage_config.read().unwrap();
    let monitored_count = arb_config.monitored_tokens.len();

    // Get arbitrage stats
    let (total_opportunities, recent_profit_usdc, all_time_profit_usdc, top_opportunities_html) =
        if let Some(monitor) = &state.arbitrage_monitor {
            let all_opportunities = monitor.get_recent_opportunities(1000);
            let total = all_opportunities.len();
            let recent_profit: u64 = all_opportunities
                .iter()
                .take(10)
                .map(|o| o.profit_amount)
                .sum();
            let profit_usdc = recent_profit as f64 / 1_000_000.0;

            // Calculate all-time profit
            let all_time_profit: u64 = all_opportunities.iter().map(|o| o.profit_amount).sum();
            let all_time_profit_usdc = all_time_profit as f64 / 1_000_000.0;

            // Generate opportunities table
            let top_20 = all_opportunities.iter().take(20);
            let mut opp_html = String::new();
            for opp in top_20 {
                // Format timestamp as human readable (already in seconds)
                let timestamp =
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(opp.detected_at);
                let datetime = format!("{:?}", timestamp);

                let profit_pct = if opp.input_amount > 0 {
                    (opp.profit_amount as f64 / opp.input_amount as f64) * 100.0
                } else {
                    0.0
                };

                // Format status badge
                let status_html = match opp.execution_status {
                    crate::arbitrage_monitor::ExecutionStatus::NotYet => {
                        "<span class='status-badge status-pending'>⏳ Pending</span>".to_string()
                    }
                    crate::arbitrage_monitor::ExecutionStatus::Success => {
                        "<span class='status-badge status-success'>✅ Success</span>".to_string()
                    }
                    crate::arbitrage_monitor::ExecutionStatus::Fail => {
                        let error_text = opp.error_message.as_deref().unwrap_or("Unknown error");
                        let escaped_error = error_text.replace("'", "\\'");
                        format!(
                            "<div class='tooltip' onclick=\"copyToClipboard('{}')\" title='Click to copy error'>\
                                <span class='status-badge status-fail'>❌ Failed</span>\
                                <span class='tooltiptext'>{}</span>\
                            </div>",
                            escaped_error, error_text
                        )
                    }
                };

                opp_html.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}→{}</td><td>${:.2}</td><td>{:.2}%</td><td>{}</td></tr>",
                    datetime,
                    &opp.pair_name,
                    opp.token_a.chars().take(6).collect::<String>(),
                    opp.token_b.chars().take(6).collect::<String>(),
                    opp.profit_amount as f64 / 1_000_000.0,
                    profit_pct,
                    status_html,
                ));
            }

            if opp_html.is_empty() {
                opp_html =
                    "<tr><td colspan='6' class='no-data'>No opportunities found yet</td></tr>"
                        .to_string();
            }

            (total, profit_usdc, all_time_profit_usdc, opp_html)
        } else {
            (
                0,
                0.0,
                0.0,
                "<tr><td colspan='6' class='no-data'>Arbitrage monitor not available</td></tr>"
                    .to_string(),
            )
        };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Solana DEX Arbitrage Dashboard</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}

        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #14f195 0%, #9945ff 100%);
            min-height: 100vh;
            padding: 20px;
        }}

        .container {{
            max-width: 1200px;
            margin: 0 auto;
        }}

        .header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 30px;
            background: white;
            padding: 20px;
            border-radius: 12px;
            box-shadow: 0 4px 15px rgba(0, 0, 0, 0.1);
        }}

        h1 {{
            color: #9945ff;
            margin: 0;
        }}

        .header button {{
            padding: 8px 16px;
            background: linear-gradient(135deg, #14f195 0%, #9945ff 100%);
            color: white;
            border: none;
            border-radius: 6px;
            cursor: pointer;
            font-weight: 600;
        }}

        .stats {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}

        .stat-card {{
            background: white;
            padding: 20px;
            border-radius: 12px;
            box-shadow: 0 4px 15px rgba(0, 0, 0, 0.1);
        }}

        .stat-label {{
            color: #666;
            font-size: 12px;
            text-transform: uppercase;
            margin-bottom: 10px;
        }}

        .stat-value {{
            font-size: 28px;
            font-weight: 700;
            color: #9945ff;
        }}

        .section {{
            background: white;
            padding: 20px;
            border-radius: 12px;
            box-shadow: 0 4px 15px rgba(0, 0, 0, 0.1);
            margin-bottom: 20px;
        }}

        .section h2 {{
            color: #9945ff;
            margin-bottom: 20px;
            border-bottom: 2px solid #14f195;
            padding-bottom: 10px;
        }}

        table {{
            width: 100%;
            border-collapse: collapse;
        }}

        th {{
            text-align: left;
            padding: 12px;
            background: #f8f9fa;
            border-bottom: 2px solid #e0e0e0;
            font-weight: 600;
        }}

        td {{
            padding: 12px;
            border-bottom: 1px solid #e0e0e0;
        }}

        tr:hover {{
            background: #f8f9fa;
        }}

        .no-data {{
            text-align: center;
            padding: 40px;
            color: #999;
        }}

        .status-badge {{
            padding: 4px 12px;
            border-radius: 12px;
            font-size: 12px;
            font-weight: 600;
            display: inline-block;
        }}

        .status-pending {{
            background: #fff3cd;
            color: #856404;
        }}

        .status-success {{
            background: #d4edda;
            color: #155724;
        }}

        .status-fail {{
            background: #f8d7da;
            color: #721c24;
        }}

        .tooltip {{
            position: relative;
            display: inline-block;
            cursor: pointer;
        }}

        .tooltip .tooltiptext {{
            visibility: hidden;
            width: 250px;
            background-color: #333;
            color: #fff;
            text-align: left;
            border-radius: 6px;
            padding: 8px 12px;
            position: absolute;
            z-index: 1;
            bottom: 125%; /* Position above */
            left: 50%;
            margin-left: -125px;
            opacity: 0;
            transition: opacity 0.3s;
            font-size: 11px;
            line-height: 1.4;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
            white-space: normal;
        }}

        .tooltip .tooltiptext::after {{
            content: "";
            position: absolute;
            top: 100%;
            left: 50%;
            margin-left: -5px;
            border-width: 5px;
            border-style: solid;
            border-color: #333 transparent transparent transparent;
        }}

        .tooltip:hover .tooltiptext {{
            visibility: visible;
            opacity: 0.95;
        }}


        .footer {{
            text-align: center;
            color: white;
            margin-top: 40px;
            opacity: 0.8;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 Solana DEX Arbitrage Dashboard</h1>
            <button onclick="logout()">🚪 Logout</button>
        </div>

        <div class="stats">
            <div class="stat-card">
                <div class="stat-label">Monitored Tokens</div>
                <div class="stat-value">{}</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Total Pools</div>
                <div class="stat-value">{}</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Total Pairs</div>
                <div class="stat-value">{}</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Total Tokens</div>
                <div class="stat-value">{}</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Opportunities Found</div>
                <div class="stat-value">{}</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">Recent Profit (10)</div>
                <div class="stat-value">${:.2}</div>
            </div>

            <div class="stat-card">
                <div class="stat-label">All-Time Profit</div>
                <div class="stat-value">${:.2}</div>
            </div>
        </div>

        <div class="section">
            <h2>📊 Recent Arbitrage Opportunities</h2>
            <table>
                <thead>
                    <tr>
                        <th>Time</th>
                        <th>Pair</th>
                        <th>Route</th>
                        <th>Profit</th>
                        <th>Profit %</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
        </div>

        <div class="section">
            <h2>📋 Monitored Tokens</h2>
            <table>
                <thead>
                    <tr>
                        <th>Token</th>
                        <th>Address</th>
                    </tr>
                </thead>
                <tbody id="tokens-tbody">
                </tbody>
            </table>
            <div id="no-tokens" class="no-data" style="display:none;">No monitored tokens found</div>
        </div>

        <div class="footer">
            <p>⚡ Real-time monitoring | Event-driven architecture</p>
            <p>Last updated: <span id="update-time">just now</span></p>
        </div>
    </div>

    <script>
        function logout() {{
            sessionStorage.removeItem('dashboard_auth');
            window.location.href = '/dashboard';
        }}

        function copyToClipboard(text) {{
            navigator.clipboard.writeText(text).then(function() {{
                // Optional: visual feedback
                console.log('Copied to clipboard');
            }}, function(err) {{
                console.error('Could not copy text: ', err);
            }});
        }}

        document.getElementById('update-time').textContent = new Date().toLocaleTimeString();
    </script>
</body>
</html>"#,
        monitored_count,
        pool_stats.total_pools,
        pool_stats.total_pairs,
        pool_stats.total_tokens,
        total_opportunities,
        recent_profit_usdc,
        all_time_profit_usdc,
        top_opportunities_html,
    )
}
