# Dashboard Integration Guide

## How to Connect Live Data to Dashboard

### Current State
- ✅ Dashboard HTML/CSS is complete with example data
- ✅ Routes are added to router
- ⏳ Dashboard is static (shows example data only)
- ⏳ Need to connect to arbitrage monitor for live data

### Three Phases to Production

## Phase 1: Direct Data Embedding (Quick - 1 day)

Replace static example data with live data from your state.

### Implementation:

```rust
// In dashboard.rs
use crate::pool_manager::PoolStateManager;
use crate::arbitrage_monitor::ArbitrageOpportunity;

pub async fn dashboard_page(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, (StatusCode, String)> {
    // Get live data from your state
    let monitored_tokens = {
        let config = state.arbitrage_config.read().unwrap();
        config.monitored_tokens.clone()
    };

    let pool_stats = state.pool_manager.get_stats().await;

    // Get recent opportunities from a channel or in-memory store
    let recent_opportunities = get_recent_opportunities().await; // TODO: implement

    let html = generate_dashboard_html(
        &monitored_tokens,
        &pool_stats,
        &recent_opportunities,
    );

    Ok(Html(html))
}
```

### Changes Needed:

1. **Add opportunity tracking** to AppState:
```rust
pub struct AppState {
    pub aggregator: Arc<DexAggregator>,
    pub arbitrage_config: Arc<RwLock<ArbitrageConfig>>,
    pub recent_opportunities: Arc<Mutex<VecDeque<ArbitrageOpportunity>>>, // NEW
}
```

2. **Populate opportunities** when detected in arbitrage monitor:
```rust
// In arbitrage_monitor.rs when opportunity detected
if let Some(app_state) = opportunities_sink {
    let mut opps = app_state.recent_opportunities.lock().await;
    if opps.len() > 100 {
        opps.pop_front(); // Keep only last 100
    }
    opps.push_back(opportunity);
}
```

3. **Update HTML generation** to use live data:
```rust
fn generate_dashboard_html(
    monitored_tokens: &[MonitoredToken],
    pool_stats: &PoolManagerStats,
    opportunities: &[ArbitrageOpportunity],
) -> String {
    // ... existing CSS ...

    let tokens_rows: String = monitored_tokens
        .iter()
        .enumerate()
        .map(|(i, token)| {
            let opp_count = opportunities
                .iter()
                .filter(|o| o.token == token.address)
                .count();

            format!(
                r#"<tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td class="token-symbol">{}</td>
                    <td><code class="token-address">{}</code></td>
                    <td><span class="token-badge enabled">✅ Enabled</span></td>
                    <td>Blue Chip</td>
                    <td><span class="profit-positive">{}</span></td>
                    <td><span class="profit-positive">0.18%</span></td>
                    <td>
                        <button class="action-btn remove-btn" onclick="removeToken('{}')">Remove</button>
                    </td>
                </tr>"#,
                i + 1,
                token.symbol,
                token.symbol,
                token.address,
                opp_count,
                token.symbol
            )
        })
        .collect();

    // ... format rest of HTML with live data ...
}
```

**Effort:** 1 day
**Result:** Dashboard shows live data (refreshes on page reload)

---

## Phase 2: WebSocket for Real-time Updates (Better - 2 days)

Replace polling with WebSocket for push updates.

### Architecture:
```
Arbitrage Monitor (detects opportunity)
    ↓
Sends to WebSocket channel
    ↓
WebSocket handler broadcasts to all connected clients
    ↓
Browser receives update (no polling!)
    ↓
Updates dashboard DOM
```

### Implementation:

```rust
// In main.rs - setup WebSocket sender
let (ws_tx, _) = tokio::sync::broadcast::channel(1000);

// In arbitrage_monitor - send opportunities to WebSocket
if let Some(ws) = ws_sender.clone() {
    let _ = ws.send(serde_json::json!({
        "type": "opportunity_detected",
        "opportunity": opportunity,
    }));
}

// In api handlers - WebSocket endpoint
pub async fn ws_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
) {
    let mut rx = state.ws_tx.subscribe();

    while let Ok(msg) = rx.recv().await {
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = socket.send(Message::Text(json)).await;
        }
    }
}
```

### JavaScript changes:
```javascript
// Connect WebSocket
const ws = new WebSocket('ws://localhost:3000/ws/dashboard');

ws.onmessage = (event) => {
    const data = JSON.parse(event.data);

    if (data.type === 'opportunity_detected') {
        // Add to table dynamically
        addOpportunityRow(data.opportunity);
        updateStats(data.opportunity);
    }
};

function addOpportunityRow(opp) {
    const table = document.querySelector('#opportunities table tbody');
    const row = `
        <tr>
            <td>${opp.timestamp}</td>
            <td class="token-symbol">${opp.token_symbol}</td>
            ...
        </tr>
    `;
    table.insertAdjacentHTML('afterbegin', row);

    // Keep only last 50 rows
    if (table.rows.length > 50) {
        table.rows[table.rows.length - 1].remove();
    }
}
```

**Effort:** 2 days
**Result:** Live dashboard, no polling, instant updates

---

## Phase 3: Execution & Profitability Tracking (Full - 3 days)

Add ability to execute opportunities and track results.

### Implementation:

```rust
// Add execution tracking to AppState
pub struct AppState {
    pub aggregator: Arc<DexAggregator>,
    pub arbitrage_config: Arc<RwLock<ArbitrageConfig>>,
    pub recent_opportunities: Arc<Mutex<VecDeque<ArbitrageOpportunity>>>,
    pub executed_trades: Arc<Mutex<VecDeque<ExecutedTrade>>>, // NEW
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutedTrade {
    pub signature: String,
    pub opportunity: ArbitrageOpportunity,
    pub status: TradeStatus,
    pub actual_profit: Option<u64>,
    pub executed_at: u64,
    pub confirmed_at: Option<u64>,
}

// API endpoint to execute
pub async fn execute_opportunity(
    State(state): State<Arc<AppState>>,
    Json(opp): Json<ArbitrageOpportunity>,
) -> Result<Json<ExecutionResponse>, StatusCode> {
    // Build transaction
    let tx = build_arbitrage_tx(&state.aggregator, &opp).await?;

    // Send via Jito
    let signature = send_jito_bundle(tx).await?;

    // Track execution
    let executed = ExecutedTrade {
        signature: signature.to_string(),
        opportunity: opp.clone(),
        status: TradeStatus::Pending,
        actual_profit: None,
        executed_at: current_time(),
        confirmed_at: None,
    };

    {
        let mut trades = state.executed_trades.lock().await;
        trades.push_back(executed.clone());
    }

    Ok(Json(ExecutionResponse {
        signature,
        status: "pending",
    }))
}
```

### Dashboard updates:

```html
<!-- Add Profitability section -->
<div class="section">
    <h2>📈 Executed Trades & Profitability</h2>
    <table>
        <thead>
            <tr>
                <th>Time</th>
                <th>Token</th>
                <th>Expected Profit</th>
                <th>Actual Profit</th>
                <th>Status</th>
                <th>Signature</th>
            </tr>
        </thead>
        <tbody id="executed-trades">
            <!-- Populated by JavaScript -->
        </tbody>
    </table>
</div>

<script>
// Listen for executed trades via WebSocket
ws.onmessage = (event) => {
    const data = JSON.parse(event.data);

    if (data.type === 'trade_executed') {
        addExecutedTradeRow(data.trade);
    }
    else if (data.type === 'trade_confirmed') {
        updateTradeStatus(data.signature, data.status);
    }
};
</script>
```

**Effort:** 3 days
**Result:** Full dashboard with execution and profitability tracking

---

## Quick Start: Connect Phase 1 Today

If you want to get dashboard showing live data immediately:

### 1. Add to AppState in main.rs:
```rust
use std::collections::VecDeque;
use tokio::sync::Mutex;

pub struct AppState {
    pub aggregator: Arc<DexAggregator>,
    pub arbitrage_config: Arc<RwLock<ArbitrageConfig>>,
    pub recent_opportunities: Arc<Mutex<VecDeque<ArbitrageOpportunity>>>,
}

let state = AppState {
    aggregator: aggregator.clone(),
    arbitrage_config: arc_config.clone(),
    recent_opportunities: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
};
```

### 2. Update arbitrage_monitor.rs:
```rust
// When opportunity detected, add to dashboard
if let Some(app_state) = dashboard_state.clone() {
    let mut opps = app_state.recent_opportunities.lock().await;
    if opps.len() >= 100 {
        opps.pop_front();
    }
    opps.push_back(opportunity.clone());
}
```

### 3. Update dashboard generation:
```rust
pub async fn dashboard_page(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, (StatusCode, String)> {
    let opps = state.recent_opportunities.lock().await;
    let opportunities: Vec<_> = opps.iter().cloned().collect();

    let html = generate_dashboard_html(&opportunities);
    Ok(Html(html))
}
```

### 4. Update HTML generation to use live data:
```rust
fn generate_dashboard_html(opportunities: &[ArbitrageOpportunity]) -> String {
    let opps_rows: String = opportunities
        .iter()
        .map(|opp| {
            format!(
                r#"<tr>
                    <td>{}</td>
                    <td class="token-symbol">{}</td>
                    <td>{:.4}%</td>
                    <td><span class="arbitrage-badge">⏳ Pending</span></td>
                </tr>"#,
                format_time(opp.detected_at),
                opp.token_symbol,
                opp.profit_percent,
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
        ... existing HTML ...
        <tbody>
            {}
        </tbody>
        ... rest of HTML ..."#,
        opps_rows
    )
}
```

---

## Next Steps

1. **Pick a phase** (1, 2, or 3)
2. **Create ArbitrageOpportunity struct** if not exists (for dashboard)
3. **Update AppState** with opportunity tracking
4. **Connect monitor** to populate opportunities
5. **Test dashboard** at `http://localhost:3000/dashboard`

---

## API Endpoints Needed

### For Phase 1:
- `GET /dashboard` - Done ✅

### For Phase 2:
- `GET /ws/dashboard` - WebSocket upgrade endpoint
- `GET /api/opportunities?limit=50` - REST fallback for recent opps

### For Phase 3:
- `POST /api/execute` - Execute an opportunity
- `GET /api/trades` - Get executed trades
- `GET /api/trades/{signature}` - Get trade status

---

## File Locations

```
aggregator-sol/src/
├── api/
│   ├── mod.rs           ← Add routes
│   ├── dashboard.rs     ← Dashboard handler
│   ├── handlers.rs      ← API handlers
│   └── dto.rs           ← Data types
├── arbitrage_monitor.rs ← Send opportunities to dashboard
├── main.rs              ← Create AppState and routes
└── ...
```

---

## Testing

### Phase 1 - Direct:
```bash
curl http://localhost:3000/dashboard
# Should show dashboard with mock data
```

### Phase 2 - WebSocket:
```bash
# Open browser console, run:
const ws = new WebSocket('ws://localhost:3000/ws/dashboard');
ws.onmessage = (e) => console.log('Received:', e.data);
```

### Phase 3 - Execute:
```bash
curl -X POST http://localhost:3000/api/execute \
  -H "Content-Type: application/json" \
  -d '{
    "token": "SOL_ADDRESS",
    "forward_pool": "POOL_ADDR",
    ...
  }'
```

---

## Summary

| Phase | Features | Effort | Timeline |
|-------|----------|--------|----------|
| 1 | Live data, refresh on load | 1 day | Today |
| 2 | WebSocket real-time updates | 2 days | This week |
| 3 | Execution & profitability | 3 days | Next week |

Start with Phase 1 to see live data immediately, then upgrade to WebSocket for real-time updates!
