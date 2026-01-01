# Environment Variables Catalog

Complete list of all environment variables needed for `amm-eth` and `arbitrade-eth` services, categorized as ConfigMap vs Secrets.

---

## 📋 Summary Table

| Service | ConfigMap | Secrets |
|---------|-----------|---------|
| **amm-eth** | 4 vars | 2 vars |
| **arbitrade-eth** | 15 vars | 17 vars |
| **TOTAL** | **19 vars** | **19 vars** |

---

# AMM-ETH Service

## ConfigMap Variables (Public, Non-Sensitive)

| Variable | Type | Default | Purpose | Example |
|----------|------|---------|---------|---------|
| `RUST_LOG` | string | `info` | Logging level | `info` or `debug` |
| `ETH_PRICE_WS_PORT` | port | `8080` | WebSocket port for price updates | `8080` |
| `ETH_API_PORT` | port | `2222` | HTTP API port for internal endpoints | `2222` |
| `UNISWAP_V4_SUBGRAPH_URL` | URL | (required) | GraphQL endpoint for Uniswap V4 data | `https://subgraph.satsuma-prod.com/3b2...` |

**Why ConfigMap?**
- ✅ Port numbers are not secrets
- ✅ Log levels are operational settings
- ✅ Subgraph URL is public

---

## Secrets Variables (Sensitive, Private)

| Variable | Type | Required? | Purpose | Example |
|----------|------|-----------|---------|---------|
| `ETH_RPC_URL` | URL | ✅ Yes | Ethereum RPC endpoint (contains API key) | `https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY` |
| `ETH_WEBSOCKET_URL` | URL | ✅ Yes | ETH node WebSocket (contains API key) | `wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY` |

**Why Secrets?**
- ✅ RPC URLs contain API credentials from Alchemy/Infura/etc
- ✅ Even though they're "just URLs", they authenticate requests
- ✅ Never expose in logs or version control

---

## Example amm-eth ConfigMap

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: amm-eth-config
  namespace: aggregators
data:
  RUST_LOG: "info"
  ETH_PRICE_WS_PORT: "8080"
  ETH_API_PORT: "2222"
  UNISWAP_V4_SUBGRAPH_URL: "https://subgraph.satsuma-prod.com/3b2..."
```

---

## Example amm-eth Secret

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: amm-eth-secrets
  namespace: aggregators
type: Opaque
stringData:
  ETH_RPC_URL: "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"
  ETH_WEBSOCKET_URL: "wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"
```

---

---

# ARBITRADE-ETH Service

## ConfigMap Variables (Public, Non-Sensitive)

| Variable | Type | Default | Purpose | Example |
|----------|------|---------|---------|---------|
| `RUST_LOG` | string | `info` | Logging level | `info` or `debug` |
| `ARBITRADE_PORT` | port | `3001` | HTTP API port | `3001` |
| `DEX_PRICE_STREAM` | URL | (required) | WebSocket to amm-eth | `ws://amm-eth-service:8080` |
| `DEX_SUBSCRIPTION_TOPIC` | string | `token_price` | Topic name for WebSocket | `token_price` |
| `DEX_RECONNECT_DELAY_SECS` | number | `5` | Reconnect delay if connection drops | `5` |
| `DEX_PING_INTERVAL_SECS` | number | `30` | Ping interval to keep connection alive | `30` |
| `DEX_BATCH_SIZE` | number | `100` | Number of price updates to batch | `100` |
| `DEX_BATCH_TIMEOUT_MS` | number | `1000` | Max wait time for batch before sending | `1000` |
| `MIN_PERCENT_DIFF` | float | (required) | Min % difference between CEX and DEX | `0.5` |
| `ARB_AMOUNT_USDT` | float | (required) | USDT amount to arbitrage per trade | `1000` |
| `ARB_COOLDOWN_SECS` | number | `60` | Cooldown between trades | `60` |
| `ENABLED_CEXES` | string | (optional) | Comma-separated list of CEXes to enable | `MEXC,BYBIT,KUCOIN` |
| `DISABLED_CEXES` | string | (optional) | Comma-separated list of CEXes to disable | `BITGET,GATE` |
| `KYBER_CLIENT_ID` | string | `my-trade-eth` | Client ID for KyberSwap API | `my-trade-eth` |

**Why ConfigMap?**
- ✅ Operational settings and configuration
- ✅ No authentication credentials in this list

---

## Secrets Variables (Sensitive, Private)

### Dashboard Credentials

| Variable | Type | Required? | Purpose | Example |
|----------|------|-----------|---------|---------|
| `DASHBOARD_USERNAME` | string | ✅ Yes | API auth username | `admin` |
| `DASHBOARD_PASSWORD` | string | ✅ Yes | API auth password | `super-secure-password` |

**Why Secrets?**
- ✅ Passwords MUST be in Secrets (never in ConfigMap)
- ✅ Authentication credentials are sensitive

---

### CEX API Keys & Secrets

#### MEXC

| Variable | Type | Required? | Purpose |
|----------|------|-----------|---------|
| `MEXC_API_KEY` | string | ❌ If using MEXC | API key from MEXC |
| `MEXC_API_SECRET` | string | ❌ If using MEXC | API secret from MEXC |
| `MEXC_ERC20_DEPOSIT_ADDRESS` | address | ❌ Optional | Deposit address for receiving tokens |

#### Bybit

| Variable | Type | Required? | Purpose |
|----------|------|-----------|---------|
| `BYBIT_API_KEY` | string | ❌ If using Bybit | API key from Bybit |
| `BYBIT_API_SECRET` | string | ❌ If using Bybit | API secret from Bybit |
| `BYBIT_ERC20_DEPOSIT_ADDRESS` | address | ❌ Optional | Deposit address for receiving tokens |

#### KuCoin

| Variable | Type | Required? | Purpose |
|----------|------|-----------|---------|
| `KUCOIN_API_KEY` | string | ❌ If using KuCoin | API key from KuCoin |
| `KUCOIN_API_SECRET` | string | ❌ If using KuCoin | API secret from KuCoin |
| `KUCOIN_API_PASSPHRASE` | string | ❌ If using KuCoin | API passphrase from KuCoin |
| `KUCOIN_ERC20_DEPOSIT_ADDRESS` | address | ❌ Optional | Deposit address for receiving tokens |

#### Bitget

| Variable | Type | Required? | Purpose |
|----------|------|-----------|---------|
| `BITGET_API_KEY` | string | ❌ If using Bitget | API key from Bitget |
| `BITGET_API_SECRET` | string | ❌ If using Bitget | API secret from Bitget |
| `BITGET_API_PASSPHRASE` | string | ❌ If using Bitget | API passphrase from Bitget |
| `BITGET_ERC20_DEPOSIT_ADDRESS` | address | ❌ Optional | Deposit address for receiving tokens |

#### Gate.io

| Variable | Type | Purpose |
|----------|------|---------|
| `GATE_API_KEY` | string | API key from Gate.io |
| `GATE_API_SECRET` | string | API secret from Gate.io |

**Note:** Gate.io doesn't have a passphrase, just key and secret.

---

### Blockchain/RPC Credentials

| Variable | Type | Required? | Purpose | Example |
|----------|------|-----------|---------|---------|
| `ETH_RPC_URL` | URL | ✅ Yes | Ethereum RPC endpoint (contains API key) | `https://eth-mainnet.g.alchemy.com/v2/xxx` |
| `ETH_PRIVATE_KEY` | hex | ❌ Optional | Private key for executing trades | `0x123abc...` |

**Why Secrets?**
- ✅ RPC URLs contain API credentials from Alchemy/Infura
- ✅ Private keys MUST be in Secrets (never in ConfigMap!)
- ✅ Both are sensitive credentials

---

## Summary: arbitrade-eth Categorization

### ConfigMap (15 variables)
```
RUST_LOG
ARBITRADE_PORT
DEX_PRICE_STREAM
DEX_SUBSCRIPTION_TOPIC
DEX_RECONNECT_DELAY_SECS
DEX_PING_INTERVAL_SECS
DEX_BATCH_SIZE
DEX_BATCH_TIMEOUT_MS
MIN_PERCENT_DIFF
ARB_AMOUNT_USDT
ARB_COOLDOWN_SECS
ENABLED_CEXES
DISABLED_CEXES
KYBER_CLIENT_ID
```

### Secrets (17 variables minimum)
```
Blockchain/RPC:
ETH_RPC_URL
ETH_PRIVATE_KEY (optional)

Dashboard Credentials:
DASHBOARD_USERNAME
DASHBOARD_PASSWORD

CEX Credentials (5 exchanges × 2-3 fields each):
MEXC_API_KEY, MEXC_API_SECRET, MEXC_ERC20_DEPOSIT_ADDRESS
BYBIT_API_KEY, BYBIT_API_SECRET, BYBIT_ERC20_DEPOSIT_ADDRESS
KUCOIN_API_KEY, KUCOIN_API_SECRET, KUCOIN_API_PASSPHRASE, KUCOIN_ERC20_DEPOSIT_ADDRESS
BITGET_API_KEY, BITGET_API_SECRET, BITGET_API_PASSPHRASE, BITGET_ERC20_DEPOSIT_ADDRESS
GATE_API_KEY, GATE_API_SECRET
```

---

## Example arbitrade-eth ConfigMap

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: arbitrade-eth-config
  namespace: aggregators
data:
  RUST_LOG: "info"
  ARBITRADE_PORT: "3001"
  DEX_PRICE_STREAM: "ws://amm-eth-service:8080"
  DEX_SUBSCRIPTION_TOPIC: "token_price"
  DEX_RECONNECT_DELAY_SECS: "5"
  DEX_PING_INTERVAL_SECS: "30"
  DEX_BATCH_SIZE: "100"
  DEX_BATCH_TIMEOUT_MS: "1000"
  MIN_PERCENT_DIFF: "0.5"
  ARB_AMOUNT_USDT: "1000"
  ARB_COOLDOWN_SECS: "60"
  ENABLED_CEXES: "MEXC,BYBIT,KUCOIN"
  DISABLED_CEXES: "BITGET,GATE"
  KYBER_CLIENT_ID: "my-trade-eth"
```

---

## Example arbitrade-eth Secret

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: arbitrade-eth-secrets
  namespace: aggregators
type: Opaque
stringData:
  # Blockchain/RPC (contains API credentials)
  ETH_RPC_URL: "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
  ETH_PRIVATE_KEY: "0x123abc..."

  # Dashboard credentials
  DASHBOARD_USERNAME: "admin"
  DASHBOARD_PASSWORD: "super-secure-password"

  # MEXC
  MEXC_API_KEY: "your-mexc-key"
  MEXC_API_SECRET: "your-mexc-secret"
  MEXC_ERC20_DEPOSIT_ADDRESS: "0x1234..."

  # Bybit
  BYBIT_API_KEY: "your-bybit-key"
  BYBIT_API_SECRET: "your-bybit-secret"
  BYBIT_ERC20_DEPOSIT_ADDRESS: "0x5678..."

  # KuCoin
  KUCOIN_API_KEY: "your-kucoin-key"
  KUCOIN_API_SECRET: "your-kucoin-secret"
  KUCOIN_API_PASSPHRASE: "your-kucoin-passphrase"
  KUCOIN_ERC20_DEPOSIT_ADDRESS: "0xabcd..."

  # Bitget
  BITGET_API_KEY: "your-bitget-key"
  BITGET_API_SECRET: "your-bitget-secret"
  BITGET_API_PASSPHRASE: "your-bitget-passphrase"
  BITGET_ERC20_DEPOSIT_ADDRESS: "0xef01..."

  # Gate.io
  GATE_API_KEY: "your-gate-key"
  GATE_API_SECRET: "your-gate-secret"
```

---

# 🔐 Security Best Practices

## What Should Go in ConfigMap?
- ✅ Port numbers
- ✅ Public URLs
- ✅ Log levels
- ✅ Non-sensitive configuration
- ✅ Feature flags (ENABLED_CEXES, DISABLED_CEXES)
- ✅ Batch sizes, timeouts, thresholds

## What Should Go in Secrets?
- ✅ API keys
- ✅ API secrets
- ✅ Passwords
- ✅ Private keys
- ✅ Authentication credentials
- ✅ RPC URLs that contain API keys
- ✅ Anything that shouldn't appear in `kubectl describe pod`

## What Should Go in Git?
- ✅ ConfigMap YAML files (no secrets)
- ❌ Secret YAML files (create manually or use sealed secrets)
- ✅ Deployment YAML files
- ✅ Service/PVC YAML files

## Secure Workflow

```bash
# Step 1: Store ConfigMaps in Git
git add k8s-manifests/amm-eth-configmap.yaml
git add k8s-manifests/arbitrade-eth-configmap.yaml

# Step 2: Create Secrets manually (NOT in Git)
kubectl create secret generic amm-eth-secrets \
  -n aggregators \
  --from-literal=ETH_RPC_URL=https://...

kubectl create secret generic arbitrade-eth-secrets \
  -n aggregators \
  --from-literal=DASHBOARD_PASSWORD=xxx \
  --from-literal=MEXC_API_KEY=yyy \
  # ... all other secrets

# Step 3: Make sure .gitignore includes secrets
echo "*-secrets.yaml" >> .gitignore

# Step 4: Document the secrets (but not values!)
# In a SECRETS.example file:
# DASHBOARD_PASSWORD=your-password-here
# MEXC_API_KEY=your-key-here
# (This file is committed but has placeholder values)
```

---

# 📝 How to Use These Catalogs

## For Kubernetes Deployment

1. **Create ConfigMaps** (safe to commit):
   ```bash
   kubectl apply -f k8s-manifests/amm-eth-configmap.yaml
   kubectl apply -f k8s-manifests/arbitrade-eth-configmap.yaml
   ```

2. **Create Secrets** (do NOT commit):
   ```bash
   kubectl create secret generic amm-eth-secrets \
     -n aggregators \
     --from-literal=ETH_RPC_URL="..."

   kubectl create secret generic arbitrade-eth-secrets \
     -n aggregators \
     --from-literal=DASHBOARD_PASSWORD="..." \
     --from-literal=MEXC_API_KEY="..." \
     # ... all others
   ```

3. **Reference in Deployments**:
   ```yaml
   # In deployment YAML:
   envFrom:
   - configMapRef:
       name: amm-eth-config
   - secretRef:
       name: amm-eth-secrets
   ```

---

# 🚀 Next Steps

1. Create the ConfigMap YAML files (safe, can commit)
2. Create the Secret YAML files locally (DO NOT commit)
3. Apply both to your k3s cluster
4. Update deployment manifests to reference them
5. Verify with: `kubectl exec -it pod -- env | sort`

---

# ✅ Verification Checklist

After deploying:

```bash
# Check ConfigMap was created
kubectl get configmap -n aggregators
kubectl describe cm amm-eth-config -n aggregators

# Check Secret was created (values are hidden)
kubectl get secrets -n aggregators
kubectl describe secret arbitrade-eth-secrets -n aggregators

# Verify pod has all env vars
kubectl exec -it amm-eth-xxxxx -n aggregators -- env | grep -E "ETH_RPC|RUST_LOG"

# Verify secret values are NOT visible in pod description
kubectl describe pod amm-eth-xxxxx -n aggregators
# (You should NOT see the actual secret values)

# Verify secret values ARE accessible inside container
kubectl exec -it amm-eth-xxxxx -n aggregators -- bash
$ echo $ETH_RPC_URL   # Should print the actual RPC URL
$ echo $DASHBOARD_PASSWORD  # Should print the actual password
```

---

This catalog ensures:
- ✅ No secrets in Git
- ✅ All required variables documented
- ✅ Clear categorization for each service
- ✅ Security best practices followed
