# Complete k3s Setup Guide for Aggregators with CI/CD

> This guide shows you exactly what to do on your **local machine** vs your **dedicated server**, with detailed explanations.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     YOUR SETUP                                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  LOCAL MACHINE                                                    │
│  ┌──────────────────────────────────────────────────────┐       │
│  │ • Git repo (sol-agg-rust)                            │       │
│  │ • Write code & push                                  │       │
│  │ • kubectl commands (manage cluster remotely)         │       │
│  └──────────────────────────────────────────────────────┘       │
│           │                                                       │
│           │ git push                                              │
│           ▼                                                       │
│  ┌──────────────────────────────────────────────────────┐       │
│  │ GITHUB ACTIONS (Cloud)                              │       │
│  │ • Build Docker images                               │       │
│  │ • Run tests (fmt, clippy, build)                    │       │
│  │ • Push to Docker registry                           │       │
│  │ • Deploy to k3s cluster                             │       │
│  └──────────────────────────────────────────────────────┘       │
│           │                                                       │
│           │ kubectl apply                                         │
│           ▼                                                       │
│  DEDICATED SERVER (Your Cloud)                                    │
│  ┌──────────────────────────────────────────────────────┐       │
│  │ k3s CLUSTER                                          │       │
│  │ ┌────────────────────────────────────────────────┐  │       │
│  │ │ amm-eth Pod (Port 8080)                        │  │       │
│  │ │ • Listens for DEX price updates                │  │       │
│  │ │ • Stores in RocksDB (/app/rocksdb_data)       │  │       │
│  │ └────────────────────────────────────────────────┘  │       │
│  │                                                      │       │
│  │ ┌────────────────────────────────────────────────┐  │       │
│  │ │ arbitrade-eth Pod (Port 3001)                  │  │       │
│  │ │ • Connects to amm-eth for prices               │  │       │
│  │ │ • Exposes API on port 3001                     │  │       │
│  │ │ • Stores results in RocksDB                    │  │       │
│  │ └────────────────────────────────────────────────┘  │       │
│  │                                                      │       │
│  │ Docker Registry (localhost:5000)                     │       │
│  │ • Stores your images locally                         │       │
│  └──────────────────────────────────────────────────────┘       │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

# PHASE 1: SERVER SETUP (Do this ONCE on your dedicated server)

## Step 1: Prerequisites on Your Dedicated Server

**Location:** Your dedicated server
**Time:** ~5 minutes

```bash
# SSH into your server
ssh root@your-server-ip

# Update system
apt-get update && apt-get upgrade -y

# Install Docker (required for k3s and local registry)
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# Verify Docker is running
docker --version
sudo systemctl start docker
sudo systemctl enable docker

# Allow non-root Docker access (optional but recommended)
sudo usermod -aG docker $USER
newgrp docker
```

**Explanation:**
- Docker is needed because k3s runs containers, and you'll need a local image registry
- We're updating the system to ensure all dependencies are available

---

## Step 2: Install k3s on Your Dedicated Server

**Location:** Your dedicated server
**Time:** ~2 minutes

```bash
# Install k3s (single-node cluster)
curl -sfL https://get.k3s.io | sh -

# Wait for k3s to be ready (should take ~30 seconds)
sudo k3s kubectl get nodes

# You should see:
# NAME        STATUS   ROLES                  AGE    VERSION
# your-server Ready    control-plane,master   10s    v1.xx.x
```

**Explanation:**
- This installs k3s in single-node mode (1 server = control plane + workers combined)
- k3s auto-starts on system boot
- The `curl | sh` script is safe for production - it's from the official k3s project

---

## Step 3: Configure k3s Access

**Location:** Your dedicated server
**Time:** ~3 minutes

```bash
# k3s stores its config in /etc/rancher/k3s/k3s.yaml
# We need to make it accessible to your local machine

# First, make the config readable
sudo chmod 644 /etc/rancher/k3s/k3s.yaml

# Copy the kubeconfig to your home directory
sudo cp /etc/rancher/k3s/k3s.yaml ~/k3s-config.yaml
sudo chown $USER:$USER ~/k3s-config.yaml

# View it to copy later
cat ~/k3s-config.yaml
```

**Output will look like:**
```yaml
apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: LS0tLS1CRUdJTi...
    server: https://127.0.0.1:6443
  name: default
contexts:
- context:
    cluster: default
    user: admin@default
  name: default
current-context: default
kind: Config
preferences: {}
users:
- name: admin@default
  user:
    client-certificate-data: LS0tLS1CRUdJTi...
    client-key-data: LS0tLS1CRUdJTi...
```

**Explanation:**
- kubeconfig is how kubectl knows how to connect to your cluster
- We need to modify the `server:` field to use your server's IP instead of `127.0.0.1`

---

## Step 4: Update kubeconfig for Remote Access

**Location:** Your dedicated server
**Time:** ~2 minutes

```bash
# Get your server's IP address
SERVER_IP=$(hostname -I | awk '{print $1}')
echo "Your server IP is: $SERVER_IP"

# Update the kubeconfig to use your server IP instead of localhost
sed -i "s/127.0.0.1/$SERVER_IP/g" ~/k3s-config.yaml

# Verify the change
cat ~/k3s-config.yaml | grep "server:"
# Should show: server: https://YOUR_SERVER_IP:6443
```

**Explanation:**
- The default config points to `127.0.0.1` (localhost), which only works on the server itself
- We replace it with your actual server IP so you can access it from your local machine
- k3s API server runs on port 6443

---

## Step 5: Set Up Local Docker Registry on Server

**Location:** Your dedicated server
**Time:** ~2 minutes

```bash
# Start a Docker registry container
docker run -d \
  -p 5000:5000 \
  --restart=always \
  --name registry \
  registry:2

# Verify it's running
docker ps | grep registry
```

**Explanation:**
- This Docker registry will store your Docker images
- Your k3s cluster will pull images from `localhost:5000/amm-eth:latest`
- GitHub Actions will push built images here
- `--restart=always` means it auto-starts when the server reboots

---

## Step 6: Configure k3s to Trust the Local Registry

**Location:** Your dedicated server
**Time:** ~2 minutes

```bash
# Create registries config for k3s
sudo mkdir -p /etc/rancher/k3s
sudo tee /etc/rancher/k3s/registries.yaml > /dev/null <<EOF
mirrors:
  localhost:5000:
    endpoint:
      - "http://localhost:5000"
EOF

# Restart k3s to apply the config
sudo systemctl restart k3s

# Wait for it to come back up
sleep 10
sudo k3s kubectl get nodes
```

**Explanation:**
- k3s by default trusts Docker Hub only
- This config tells k3s: "Allow insecure (HTTP, not HTTPS) access to localhost:5000"
- Without this, k3s won't be able to pull images from your local registry

---

## Step 7: Create Kubernetes Namespace

**Location:** Your dedicated server
**Time:** ~1 minute

```bash
# Create a namespace for your aggregators (organizational, like a folder)
sudo k3s kubectl create namespace aggregators

# Verify
sudo k3s kubectl get namespaces
```

**Explanation:**
- Namespaces are logical isolation in Kubernetes
- All your aggregator pods will run in the `aggregators` namespace
- Good practice: keep system stuff separate from your apps

---

## Phase 1 Summary

**On your server, you now have:**
✅ k3s cluster running and accessible
✅ kubeconfig file ready to copy to local machine
✅ Docker registry at `localhost:5000`
✅ `aggregators` namespace created

**Next:** Copy kubeconfig to local machine

---

# PHASE 2: LOCAL MACHINE SETUP

## Step 8: Copy kubeconfig to Local Machine

**Location:** Your LOCAL machine
**Time:** ~2 minutes

```bash
# On your LOCAL machine, download the kubeconfig from your server
scp root@your-server-ip:~/k3s-config.yaml ~/.kube/k3s-config.yaml

# If ~/.kube doesn't exist, create it
mkdir -p ~/.kube

# Tell kubectl to use this config
export KUBECONFIG=~/.kube/k3s-config.yaml

# Verify you can access the cluster
kubectl get nodes
# Should output:
# NAME        STATUS   ROLES                  AGE    VERSION
# your-server Ready    control-plane,master   3m     v1.xx.x
```

**Explanation:**
- `scp` = secure copy (SSH file transfer)
- We put it in `~/.kube/` which is the standard Kubernetes config directory
- KUBECONFIG env var tells kubectl which cluster to connect to

---

## Step 9: Make kubectl Access Permanent

**Location:** Your LOCAL machine
**Time:** ~1 minute

```bash
# Add this to your ~/.zshrc or ~/.bash_profile
echo 'export KUBECONFIG=~/.kube/k3s-config.yaml' >> ~/.zshrc

# Reload shell
source ~/.zshrc

# Now you can use kubectl without the export
kubectl get nodes
```

**Explanation:**
- Without this, you'd have to set KUBECONFIG every time you open a new terminal
- We're adding it to your shell config so it auto-loads

---

## Step 10: Install kubectl on Local Machine (if not already installed)

**Location:** Your LOCAL machine
**Time:** ~3 minutes

```bash
# macOS
brew install kubectl

# Linux
curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl"
sudo install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl

# Verify
kubectl version
```

---

# PHASE 3: CREATE KUBERNETES MANIFESTS

## Step 11: Create Deployment Manifests (On Local Machine)

**Location:** Your LOCAL machine or in your git repo
**Time:** ~10 minutes

Create a directory for k3s configs:

```bash
mkdir -p k8s-manifests
cd k8s-manifests
```

### File: `k8s-manifests/namespace.yaml`

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: aggregators
  labels:
    name: aggregators
```

### File: `k8s-manifests/amm-eth-configmap.yaml`

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

**Variables:**
- `RUST_LOG` - Logging level (non-sensitive)
- `ETH_PRICE_WS_PORT` - WebSocket port (non-sensitive)
- `ETH_API_PORT` - API port (non-sensitive)
- `UNISWAP_V4_SUBGRAPH_URL` - Public GraphQL endpoint (non-sensitive)

### File: `k8s-manifests/amm-eth-deployment.yaml`

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: amm-eth
  namespace: aggregators
  labels:
    app: amm-eth
spec:
  replicas: 1
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app: amm-eth
  template:
    metadata:
      labels:
        app: amm-eth
    spec:
      containers:
      - name: amm-eth
        image: localhost:5000/amm-eth:latest  # Pull from local registry!
        imagePullPolicy: Always  # Always pull latest image
        ports:
        - containerPort: 8080
          name: ws
          protocol: TCP
        envFrom:
        # Load non-sensitive config from ConfigMap
        - configMapRef:
            name: amm-eth-config
        # Load sensitive secrets from Kubernetes Secret
        - secretRef:
            name: amm-eth-secrets
        resources:
          requests:
            memory: "256Mi"
            cpu: "250m"
          limits:
            memory: "512Mi"
            cpu: "500m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 5
        volumeMounts:
        - name: rocksdb-data
          mountPath: /app/rocksdb_data/amm-eth
      volumes:
      - name: rocksdb-data
        persistentVolumeClaim:
          claimName: amm-eth-pvc
```

### File: `k8s-manifests/amm-eth-service.yaml`

```yaml
apiVersion: v1
kind: Service
metadata:
  name: amm-eth-service
  namespace: aggregators
  labels:
    app: amm-eth
spec:
  selector:
    app: amm-eth
  type: ClusterIP  # Internal-only (not exposed outside cluster)
  ports:
  - name: ws
    port: 8080
    targetPort: 8080
    protocol: TCP
```

### File: `k8s-manifests/amm-eth-pvc.yaml`

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: amm-eth-pvc
  namespace: aggregators
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 10Gi
  storageClassName: local-path  # k3s provides this by default
```

### File: `k8s-manifests/arbitrade-eth-configmap.yaml`

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
  ENABLED_CEXES: "MEXC,BYBIT,KUCOIN,BITGET,GATE"
  DISABLED_CEXES: ""
  KYBER_CLIENT_ID: "my-trade-eth"
```

**Variables in ConfigMap (Non-Sensitive):**
- Logging and port configuration
- DEX connection settings (topology, retry logic)
- Trade parameters (min %, amount, cooldown)
- CEX enable/disable lists
- KyberSwap client ID (public)

---

### File: `k8s-manifests/arbitrade-eth-secrets.template.yaml`

⚠️ **TEMPLATE FILE - DO NOT USE DIRECTLY**

This file shows what secrets are needed. Replace placeholder values with real secrets from GitHub Settings → Secrets → Actions.

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: arbitrade-eth-secrets
  namespace: aggregators
type: Opaque
stringData:
  # Dashboard credentials
  DASHBOARD_USERNAME: "admin"
  DASHBOARD_PASSWORD: "YOUR_SECURE_PASSWORD_HERE"

  # Ethereum RPC endpoints (contain API keys)
  ETH_RPC_URL: "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"

  # Blockchain wallet
  ETH_PRIVATE_KEY: "0xYOUR_PRIVATE_KEY_HERE"

  # MEXC Exchange
  MEXC_API_KEY: "YOUR_KEY_HERE"
  MEXC_API_SECRET: "YOUR_SECRET_HERE"
  MEXC_ERC20_DEPOSIT_ADDRESS: "0xYOUR_ADDRESS_HERE"

  # Bybit Exchange
  BYBIT_API_KEY: "YOUR_KEY_HERE"
  BYBIT_API_SECRET: "YOUR_SECRET_HERE"
  BYBIT_ERC20_DEPOSIT_ADDRESS: "0xYOUR_ADDRESS_HERE"

  # KuCoin Exchange
  KUCOIN_API_KEY: "YOUR_KEY_HERE"
  KUCOIN_API_SECRET: "YOUR_SECRET_HERE"
  KUCOIN_API_PASSPHRASE: "YOUR_PASSPHRASE_HERE"
  KUCOIN_ERC20_DEPOSIT_ADDRESS: "0xYOUR_ADDRESS_HERE"

  # Bitget Exchange
  BITGET_API_KEY: "YOUR_KEY_HERE"
  BITGET_API_SECRET: "YOUR_SECRET_HERE"
  BITGET_API_PASSPHRASE: "YOUR_PASSPHRASE_HERE"
  BITGET_ERC20_DEPOSIT_ADDRESS: "0xYOUR_ADDRESS_HERE"

  # Gate.io Exchange
  GATE_API_KEY: "YOUR_KEY_HERE"
  GATE_API_SECRET: "YOUR_SECRET_HERE"
```

**Secrets are Created by GitHub Actions Workflow:**
- Secrets are NOT stored in Git
- Secrets are stored encrypted in GitHub (Settings → Secrets → Actions)
- GitHub Actions workflow extracts them and creates the k8s Secret
- See **Phase 4: CI/CD Setup** for details

---

### File: `k8s-manifests/arbitrade-eth-deployment.yaml`

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: arbitrade-eth
  namespace: aggregators
  labels:
    app: arbitrade-eth
spec:
  replicas: 1
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app: arbitrade-eth
  template:
    metadata:
      labels:
        app: arbitrade-eth
    spec:
      containers:
      - name: arbitrade-eth
        image: localhost:5000/arbitrade-eth:latest  # Pull from local registry!
        imagePullPolicy: Always
        ports:
        - containerPort: 3001
          name: api
          protocol: TCP
        envFrom:
        # Load non-sensitive config from ConfigMap
        - configMapRef:
            name: arbitrade-eth-config
        # Load sensitive secrets from Kubernetes Secret
        - secretRef:
            name: arbitrade-eth-secrets

        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "1Gi"
            cpu: "1000m"
        livenessProbe:
          httpGet:
            path: /health
            port: 3001
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 3001
          initialDelaySeconds: 10
          periodSeconds: 5
        volumeMounts:
        - name: rocksdb-data
          mountPath: /app/rocksdb_data
      volumes:
      - name: rocksdb-data
        persistentVolumeClaim:
          claimName: arbitrade-eth-pvc
```

### File: `k8s-manifests/arbitrade-eth-service.yaml`

```yaml
apiVersion: v1
kind: Service
metadata:
  name: arbitrade-eth-service
  namespace: aggregators
  labels:
    app: arbitrade-eth
spec:
  selector:
    app: arbitrade-eth
  type: NodePort  # Expose on host port
  ports:
  - name: api
    port: 3001
    targetPort: 3001
    protocol: TCP
    nodePort: 30001  # Accessible at your-server-ip:30001
```

### File: `k8s-manifests/arbitrade-eth-pvc.yaml`

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: arbitrade-eth-pvc
  namespace: aggregators
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 10Gi
  storageClassName: local-path
```

**Explanation of Key Fields:**

| Field | Meaning |
|-------|---------|
| `image: localhost:5000/amm-eth:latest` | Pull from local registry on your server |
| `imagePullPolicy: Always` | Always pull latest (so new deployments get new code) |
| `DEX_PRICE_STREAM: ws://amm-eth-service:8080` | Service DNS - arbitrade-eth finds amm-eth automatically |
| `persistentVolumeClaim` | Data persists even if pod restarts |
| `requests/limits` | Guarantees/caps CPU and memory |
| `livenessProbe` | Restart pod if health check fails |
| `readinessProbe` | Wait until pod is ready before sending traffic |

---

## Step 12: Deploy ConfigMaps Locally (Secrets Come from GitHub Actions)

**Location:** Your LOCAL machine (via kubectl)
**Time:** ~2 minutes

This is the key difference in our approach:
- ✅ **ConfigMaps**: Deploy from Git locally
- ✅ **Secrets**: Created automatically by GitHub Actions (NOT manually)

### Option A: Deploy ConfigMaps Only (Recommended for Testing)

```bash
# Navigate to k8s-manifests directory
cd k8s-manifests

# Apply namespace + ConfigMaps only (no secrets)
kubectl apply -f namespace.yaml
kubectl apply -f amm-eth-configmap.yaml
kubectl apply -f arbitrade-eth-configmap.yaml

# Verify ConfigMaps were created
kubectl get configmaps -n aggregators
# OUTPUT:
# NAME                    DATA   AGE
# amm-eth-config          4      10s
# arbitrade-eth-config    15     8s
```

**Why just ConfigMaps?**
- ConfigMaps contain non-sensitive config that's safe in Git
- Secrets will be created by GitHub Actions workflow from GitHub Secrets vault
- This lets you test the ConfigMap setup before secrets are available

### Option B: Full Manual Deployment (if you want to test locally first)

⚠️ **Only do this for testing** — in production, secrets come from GitHub Actions!

If you want to manually create secrets locally for testing:

```bash
# Create amm-eth secret manually
kubectl create secret generic amm-eth-secrets \
  -n aggregators \
  --from-literal=ETH_RPC_URL="https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY" \
  --from-literal=ETH_WEBSOCKET_URL="wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"

# Create arbitrade-eth secret manually
kubectl create secret generic arbitrade-eth-secrets \
  -n aggregators \
  --from-literal=ETH_RPC_URL="https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY" \
  --from-literal=ETH_PRIVATE_KEY="0xYOUR_KEY" \
  --from-literal=DASHBOARD_USERNAME="admin" \
  --from-literal=DASHBOARD_PASSWORD="your-password" \
  --from-literal=MEXC_API_KEY="your-key" \
  --from-literal=MEXC_API_SECRET="your-secret" \
  # ... (add all other secrets)

# Apply all deployment manifests
kubectl apply -f amm-eth-deployment.yaml
kubectl apply -f amm-eth-service.yaml
kubectl apply -f amm-eth-pvc.yaml
kubectl apply -f arbitrade-eth-deployment.yaml
kubectl apply -f arbitrade-eth-service.yaml
kubectl apply -f arbitrade-eth-pvc.yaml

# Watch pods come up
kubectl get pods -n aggregators -w
```

> ⚠️ **IMPORTANT**: This manual approach is ONLY for local testing. In production, secrets are created by GitHub Actions from the GitHub Secrets vault. Never commit secret files to Git!

---

## Step 13: Understanding the GitHub Actions Deployment Flow

**How secrets get to your cluster (the proper way):**

```
1. You add secrets to GitHub Settings → Secrets → Actions
   ├─ K8S_ETH_RPC_URL
   ├─ K8S_DASHBOARD_PASSWORD
   ├─ K8S_MEXC_API_KEY
   └─ ... (21 more)

2. You commit code + push to main

3. GitHub Actions workflow triggers:
   ├─ Builds Docker images
   ├─ Pushes to registry
   ├─ Extracts secrets from GitHub vault
   ├─ Creates ConfigMaps from Git:
   │  └─ kubectl apply -f amm-eth-configmap.yaml
   ├─ Creates Secrets from GitHub Secrets:
   │  └─ kubectl create secret generic amm-eth-secrets \
   │      --from-literal=ETH_RPC_URL=${{ secrets.K8S_ETH_RPC_URL }}
   ├─ Applies Deployments:
   │  └─ kubectl apply -f amm-eth-deployment.yaml
   └─ Verifies rollout

4. Result: Pods run with both ConfigMap + Secret env vars ✅
```

**Why this approach?**
- ✅ Secrets never stored in Git
- ✅ Secrets encrypted in GitHub vault
- ✅ Automatic deployment on every push
- ✅ Secrets rotated easily (update GitHub, next push uses new values)
- ✅ Professional, industry-standard workflow

---

## Step 14: Test the Deployment

**Location:** Your LOCAL machine (via kubectl)
**Time:** ~5 minutes

### After Step 12: Check ConfigMaps

```bash
# If you only deployed ConfigMaps (Recommended approach):
kubectl get all -n aggregators
# You should see:
# - Namespace: aggregators ✅
# - ConfigMaps: amm-eth-config, arbitrade-eth-config ✅
# - No pods yet (waiting for secrets) ⏳

kubectl get configmaps -n aggregators -o yaml
# Verify all config values are present
```

### After GitHub Actions Deploys (or manual secrets):

```bash
# Watch pods come up
kubectl get pods -n aggregators -w
# Expected after ~30 seconds:
# NAME                             READY   STATUS    RESTARTS   AGE
# pod/amm-eth-xxxxx                1/1     Running   0          30s
# pod/arbitrade-eth-xxxxx          1/1     Running   0          25s

# Check services
kubectl get services -n aggregators
# NAME                    TYPE        CLUSTER-IP      PORT(S)
# amm-eth-service         ClusterIP   10.43.xxx.xxx   8080/TCP
# arbitrade-eth-service   NodePort    10.43.yyy.yyy   3001:30001/TCP

# Check ConfigMaps
kubectl get configmaps -n aggregators
# NAME                    DATA   AGE
# amm-eth-config          4      5m
# arbitrade-eth-config    15     5m

# Check Secrets (values are hidden)
kubectl get secrets -n aggregators
# NAME                     TYPE     DATA   AGE
# amm-eth-secrets          Opaque   2      3m
# arbitrade-eth-secrets    Opaque   17     3m
```

### View Pod Logs

```bash
# Check amm-eth logs
kubectl logs -n aggregators deployment/amm-eth -f
# Should show: "Listening on ws://0.0.0.0:8080"

# In another terminal, check arbitrade-eth logs
kubectl logs -n aggregators deployment/arbitrade-eth -f
# Should show: "Connected to amm-eth at ws://amm-eth-service:8080"

# Exit log view: Ctrl+C
```

### Port Forward to Test API

```bash
# Forward arbitrade-eth API to local machine
kubectl port-forward -n aggregators svc/arbitrade-eth-service 3001:3001

# In another terminal, test the API
curl http://localhost:3001/health
# Should return: {"status":"ok"}

# Exit port-forward: Ctrl+C
```

---

## Timeline Summary

**Manual Testing Scenario:**
- Step 12a: Deploy ConfigMaps locally → 30 seconds
- Step 12b: Manually create secrets locally → 1 minute
- Step 12c: Deploy manifests locally → 1 minute
- Step 13: Pods come up → 30 seconds
- **Total: ~3 minutes**

**GitHub Actions Scenario (Production):**
- You commit code → GitHub Actions triggered → 0-2 minutes
- Build + test → 2 minutes
- Docker build → 2 minutes
- Push to registry → 30 seconds
- Create ConfigMaps → 10 seconds
- Create Secrets (from GitHub vault) → 10 seconds
- Apply Deployments → 30 seconds
- Rollout verification → 1 minute
- **Total: ~7 minutes from push to live**


**Location:** Your LOCAL machine (via kubectl)
**Time:** ~5 minutes

```bash
# Check logs of amm-eth
kubectl logs -n aggregators deployment/amm-eth -f

# In another terminal, check arbitrade-eth logs
kubectl logs -n aggregators deployment/arbitrade-eth -f

# Port forward to access from your local machine
kubectl port-forward -n aggregators svc/arbitrade-eth-service 3001:3001

# Now access the API
curl http://localhost:3001/health
```

**Explanation:**
- `kubectl logs` shows container output (like `docker logs`)
- `kubectl port-forward` creates a tunnel from local:3001 → cluster:3001
- This is how you access services inside the cluster from outside

---

# PHASE 4: GITHUB SECRETS & CI/CD SETUP

## Step 14: Add All Secrets to GitHub

**Location:** GitHub Settings (cloud)
**Time:** ~15 minutes

This is the **most important step** for your CI/CD to work!

### Navigate to GitHub Secrets

1. Go to your GitHub repository
2. Click **Settings** (top right)
3. Click **Secrets and variables** → **Actions** (left sidebar)
4. Click **"New repository secret"** for each secret below

### Secrets for Kubernetes Access

```
Name: K8S_KUBECONFIG_B64
Value: [base64-encoded kubeconfig]
```

**How to get it:**
```bash
# On your local machine (after copying kubeconfig from server)
cat ~/.kube/k3s-config.yaml | base64
# Copy the entire output and paste as the value
```

### Secrets for Ethereum & Blockchain

```
Name: K8S_ETH_RPC_URL
Value: https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
```

```
Name: K8S_ETH_WEBSOCKET_URL
Value: wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
```

```
Name: K8S_ETH_PRIVATE_KEY
Value: 0xYOUR_PRIVATE_KEY (32 bytes, with 0x prefix)
```

### Secrets for Dashboard

```
Name: K8S_DASHBOARD_USERNAME
Value: admin
```

```
Name: K8S_DASHBOARD_PASSWORD
Value: YOUR_SECURE_PASSWORD
```

### Secrets for CEX Exchange APIs

**MEXC:**
```
Name: K8S_MEXC_API_KEY
Value: [from MEXC account settings]
```

```
Name: K8S_MEXC_API_SECRET
Value: [from MEXC account settings]
```

```
Name: K8S_MEXC_ERC20_DEPOSIT_ADDRESS
Value: 0x[your MEXC ERC20 address]
```

**Bybit:**
```
Name: K8S_BYBIT_API_KEY
Value: [from Bybit account settings]
```

```
Name: K8S_BYBIT_API_SECRET
Value: [from Bybit account settings]
```

```
Name: K8S_BYBIT_ERC20_DEPOSIT_ADDRESS
Value: 0x[your Bybit ERC20 address]
```

**KuCoin:**
```
Name: K8S_KUCOIN_API_KEY
Value: [from KuCoin account settings]
```

```
Name: K8S_KUCOIN_API_SECRET
Value: [from KuCoin account settings]
```

```
Name: K8S_KUCOIN_API_PASSPHRASE
Value: [from KuCoin account settings]
```

```
Name: K8S_KUCOIN_ERC20_DEPOSIT_ADDRESS
Value: 0x[your KuCoin ERC20 address]
```

**Bitget:**
```
Name: K8S_BITGET_API_KEY
Value: [from Bitget account settings]
```

```
Name: K8S_BITGET_API_SECRET
Value: [from Bitget account settings]
```

```
Name: K8S_BITGET_API_PASSPHRASE
Value: [from Bitget account settings]
```

```
Name: K8S_BITGET_ERC20_DEPOSIT_ADDRESS
Value: 0x[your Bitget ERC20 address]
```

**Gate.io:**
```
Name: K8S_GATE_API_KEY
Value: [from Gate.io account settings]
```

```
Name: K8S_GATE_API_SECRET
Value: [from Gate.io account settings]
```

### Verify All Secrets Added

You should now have **22 secrets** total in GitHub:

```
✅ K8S_KUBECONFIG_B64
✅ K8S_ETH_RPC_URL
✅ K8S_ETH_WEBSOCKET_URL
✅ K8S_ETH_PRIVATE_KEY
✅ K8S_DASHBOARD_USERNAME
✅ K8S_DASHBOARD_PASSWORD
✅ K8S_MEXC_API_KEY
✅ K8S_MEXC_API_SECRET
✅ K8S_MEXC_ERC20_DEPOSIT_ADDRESS
✅ K8S_BYBIT_API_KEY
✅ K8S_BYBIT_API_SECRET
✅ K8S_BYBIT_ERC20_DEPOSIT_ADDRESS
✅ K8S_KUCOIN_API_KEY
✅ K8S_KUCOIN_API_SECRET
✅ K8S_KUCOIN_API_PASSPHRASE
✅ K8S_KUCOIN_ERC20_DEPOSIT_ADDRESS
✅ K8S_BITGET_API_KEY
✅ K8S_BITGET_API_SECRET
✅ K8S_BITGET_API_PASSPHRASE
✅ K8S_BITGET_ERC20_DEPOSIT_ADDRESS
✅ K8S_GATE_API_KEY
✅ K8S_GATE_API_SECRET
```

---

---

## Step 15: Create CI/CD Workflow

**Location:** Your git repo (local machine)
**Time:** ~10 minutes

Create file: `.github/workflows/deploy-to-k3s.yml`

```yaml
name: Build and Deploy to k3s

on:
  push:
    branches: [ main ]
    paths:
      - 'bins/amm-eth/**'
      - 'bins/arbitrade-eth/**'
      - 'crates/**'
      - 'Cargo.toml'
      - 'Cargo.lock'
      - '.github/workflows/deploy-to-k3s.yml'
      - 'docker/Dockerfile.eth'
      - 'k8s-manifests/**'

jobs:
  build-and-deploy:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Install build dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            protobuf-compiler \
            libprotobuf-dev \
            pkg-config \
            libssl-dev \
            libclang-dev \
            clang \
            > /dev/null 2>&1

      # ===== BUILD STAGE =====

      - name: Run cargo fmt check
        run: cargo fmt --all -- --check || true

      - name: Run cargo clippy
        run: cargo clippy --all-targets --all-features -- -D warnings || true

      - name: Build amm-eth
        run: cargo build --release --bin amm-eth

      - name: Build arbitrade-eth
        run: cargo build --release --bin arbitrade-eth

      # ===== DOCKER BUILD STAGE =====

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v2

      - name: Build and load Docker images
        run: |
          docker build \
            --file docker/Dockerfile.eth \
            --tag localhost:5000/amm-eth:${{ github.sha }} \
            --tag localhost:5000/amm-eth:latest \
            .

          docker build \
            --file docker/Dockerfile.eth \
            --tag localhost:5000/arbitrade-eth:${{ github.sha }} \
            --tag localhost:5000/arbitrade-eth:latest \
            .

      # ===== PUSH STAGE =====

      - name: Push to Docker Registry
        env:
          SERVER_IP: ${{ secrets.SERVER_IP }}
        run: |
          # Save images
          docker save localhost:5000/amm-eth:latest | \
            ssh root@$SERVER_IP docker load

          docker save localhost:5000/arbitrade-eth:latest | \
            ssh root@$SERVER_IP docker load

      # ===== DEPLOY STAGE =====

      - name: Setup kubectl
        uses: azure/setup-kubectl@v3
        with:
          version: 'latest'

      - name: Configure kubeconfig
        env:
          KUBECONFIG_B64: ${{ secrets.K8S_KUBECONFIG_B64 }}
        run: |
          mkdir -p ~/.kube
          echo $KUBECONFIG_B64 | base64 -d > ~/.kube/config
          chmod 600 ~/.kube/config

      - name: Apply ConfigMaps
        run: |
          # Apply non-sensitive config from Git
          kubectl apply -f k8s-manifests/namespace.yaml
          kubectl apply -f k8s-manifests/amm-eth-configmap.yaml
          kubectl apply -f k8s-manifests/arbitrade-eth-configmap.yaml

      - name: Create amm-eth Secret
        env:
          ETH_RPC_URL: ${{ secrets.K8S_ETH_RPC_URL }}
          ETH_WEBSOCKET_URL: ${{ secrets.K8S_ETH_WEBSOCKET_URL }}
        run: |
          kubectl create secret generic amm-eth-secrets \
            -n aggregators \
            --from-literal=ETH_RPC_URL=$ETH_RPC_URL \
            --from-literal=ETH_WEBSOCKET_URL=$ETH_WEBSOCKET_URL \
            --dry-run=client -o yaml | kubectl apply -f -

      - name: Create arbitrade-eth Secret
        env:
          # Blockchain
          ETH_RPC_URL: ${{ secrets.K8S_ETH_RPC_URL }}
          ETH_PRIVATE_KEY: ${{ secrets.K8S_ETH_PRIVATE_KEY }}

          # Dashboard
          DASHBOARD_USERNAME: ${{ secrets.K8S_DASHBOARD_USERNAME }}
          DASHBOARD_PASSWORD: ${{ secrets.K8S_DASHBOARD_PASSWORD }}

          # MEXC
          MEXC_API_KEY: ${{ secrets.K8S_MEXC_API_KEY }}
          MEXC_API_SECRET: ${{ secrets.K8S_MEXC_API_SECRET }}
          MEXC_ERC20_DEPOSIT_ADDRESS: ${{ secrets.K8S_MEXC_ERC20_DEPOSIT_ADDRESS }}

          # Bybit
          BYBIT_API_KEY: ${{ secrets.K8S_BYBIT_API_KEY }}
          BYBIT_API_SECRET: ${{ secrets.K8S_BYBIT_API_SECRET }}
          BYBIT_ERC20_DEPOSIT_ADDRESS: ${{ secrets.K8S_BYBIT_ERC20_DEPOSIT_ADDRESS }}

          # KuCoin
          KUCOIN_API_KEY: ${{ secrets.K8S_KUCOIN_API_KEY }}
          KUCOIN_API_SECRET: ${{ secrets.K8S_KUCOIN_API_SECRET }}
          KUCOIN_API_PASSPHRASE: ${{ secrets.K8S_KUCOIN_API_PASSPHRASE }}
          KUCOIN_ERC20_DEPOSIT_ADDRESS: ${{ secrets.K8S_KUCOIN_ERC20_DEPOSIT_ADDRESS }}

          # Bitget
          BITGET_API_KEY: ${{ secrets.K8S_BITGET_API_KEY }}
          BITGET_API_SECRET: ${{ secrets.K8S_BITGET_API_SECRET }}
          BITGET_API_PASSPHRASE: ${{ secrets.K8S_BITGET_API_PASSPHRASE }}
          BITGET_ERC20_DEPOSIT_ADDRESS: ${{ secrets.K8S_BITGET_ERC20_DEPOSIT_ADDRESS }}

          # Gate.io
          GATE_API_KEY: ${{ secrets.K8S_GATE_API_KEY }}
          GATE_API_SECRET: ${{ secrets.K8S_GATE_API_SECRET }}
        run: |
          kubectl create secret generic arbitrade-eth-secrets \
            -n aggregators \
            --from-literal=ETH_RPC_URL=$ETH_RPC_URL \
            --from-literal=ETH_PRIVATE_KEY=$ETH_PRIVATE_KEY \
            --from-literal=DASHBOARD_USERNAME=$DASHBOARD_USERNAME \
            --from-literal=DASHBOARD_PASSWORD=$DASHBOARD_PASSWORD \
            --from-literal=MEXC_API_KEY=$MEXC_API_KEY \
            --from-literal=MEXC_API_SECRET=$MEXC_API_SECRET \
            --from-literal=MEXC_ERC20_DEPOSIT_ADDRESS=$MEXC_ERC20_DEPOSIT_ADDRESS \
            --from-literal=BYBIT_API_KEY=$BYBIT_API_KEY \
            --from-literal=BYBIT_API_SECRET=$BYBIT_API_SECRET \
            --from-literal=BYBIT_ERC20_DEPOSIT_ADDRESS=$BYBIT_ERC20_DEPOSIT_ADDRESS \
            --from-literal=KUCOIN_API_KEY=$KUCOIN_API_KEY \
            --from-literal=KUCOIN_API_SECRET=$KUCOIN_API_SECRET \
            --from-literal=KUCOIN_API_PASSPHRASE=$KUCOIN_API_PASSPHRASE \
            --from-literal=KUCOIN_ERC20_DEPOSIT_ADDRESS=$KUCOIN_ERC20_DEPOSIT_ADDRESS \
            --from-literal=BITGET_API_KEY=$BITGET_API_KEY \
            --from-literal=BITGET_API_SECRET=$BITGET_API_SECRET \
            --from-literal=BITGET_API_PASSPHRASE=$BITGET_API_PASSPHRASE \
            --from-literal=BITGET_ERC20_DEPOSIT_ADDRESS=$BITGET_ERC20_DEPOSIT_ADDRESS \
            --from-literal=GATE_API_KEY=$GATE_API_KEY \
            --from-literal=GATE_API_SECRET=$GATE_API_SECRET \
            --dry-run=client -o yaml | kubectl apply -f -

      - name: Apply Deployments and Services
        run: |
          kubectl apply -f k8s-manifests/amm-eth-deployment.yaml
          kubectl apply -f k8s-manifests/amm-eth-service.yaml
          kubectl apply -f k8s-manifests/amm-eth-pvc.yaml
          kubectl apply -f k8s-manifests/arbitrade-eth-deployment.yaml
          kubectl apply -f k8s-manifests/arbitrade-eth-service.yaml
          kubectl apply -f k8s-manifests/arbitrade-eth-pvc.yaml

      - name: Wait for rollout
        run: |
          kubectl rollout status deployment/amm-eth -n aggregators --timeout=5m
          kubectl rollout status deployment/arbitrade-eth -n aggregators --timeout=5m

      - name: Verify deployment
        run: |
          echo "=== Pods ==="
          kubectl get pods -n aggregators

          echo "=== Services ==="
          kubectl get svc -n aggregators

          echo "=== Secrets (values hidden) ==="
          kubectl get secrets -n aggregators

          echo "=== ConfigMaps ==="
          kubectl get configmaps -n aggregators

          echo "=== Recent Events ==="
          kubectl get events -n aggregators --sort-by='.lastTimestamp' | tail -10
```

**What This Workflow Does:**

```
┌────────────────────────────────────────────┐
│ 1. Checkout code from Git                  │
│ 2. Build & test (fmt, clippy)              │
│ 3. Cargo build (create binaries)           │
│ 4. Docker build (create images)            │
│ 5. Push images to registry                 │
│ 6. Extract secrets from GitHub Secrets     │
│ 7. Apply ConfigMaps (from Git)             │
│ 8. Create amm-eth Secret (from GitHub)     │
│ 9. Create arbitrade-eth Secret (from GitHub)
│ 10. Apply Deployments, Services, PVCs      │
│ 11. Verify rollout and cluster state       │
└────────────────────────────────────────────┘
```

**Key Points:**

- ✅ ConfigMaps are committed to Git (safe, non-sensitive)
- ✅ Secrets are extracted from GitHub Secrets (encrypted vault)
- ✅ Secrets are never stored as files
- ✅ Secrets are created at deploy time
- ✅ All environment variables injected correctly into containers

---

## Step 16: Test the CI/CD Pipeline

**Location:** Local machine (git push triggers GitHub Actions)
**Time:** ~5 minutes

```bash
# Make a small change to test the pipeline
echo "# Deployment Test" >> README.md

# Commit and push
git add .
git commit -m "Test: Trigger GitHub Actions deployment"
git push origin main

# Watch the workflow run
# Go to: https://github.com/akirapham/sol-aggregator-rust/actions
# Click on the latest workflow run
# Watch all steps execute
```

**Expected Timeline:**
- Build & test: ~2 minutes
- Docker build: ~2 minutes
- Push images: ~30 seconds
- Apply manifests: ~1 minute
- Create secrets: ~30 seconds
- Deploy & verify: ~1 minute
- **Total:** ~7 minutes from push to live

**Verify Deployment:**

```bash
# From your local machine
kubectl get pods -n aggregators
kubectl logs -n aggregators deployment/amm-eth -f
kubectl logs -n aggregators deployment/arbitrade-eth -f
```

---

# PHASE 5: OPERATIONS (Day-to-Day Usage)

## How to Deploy New Code

```bash
# On your local machine
cd ~/sol-agg-rust
git commit -m "Fix bug in arbitrade logic"
git push origin main

# GitHub Actions automatically:
# 1. Builds and tests code
# 2. Builds Docker images
# 3. Pushes to registry
# 4. Creates ConfigMaps from Git
# 5. Creates Secrets from GitHub vault
# 6. Applies all manifests
# 7. Verifies deployment

# That's it! No manual steps needed.
```

## How to Check Cluster Status

```bash
# View all pods
kubectl get pods -n aggregators

# View logs in real-time
kubectl logs -n aggregators deployment/amm-eth -f

# Port forward to test locally
kubectl port-forward -n aggregators svc/arbitrade-eth-service 3001:3001

# Open browser: http://localhost:3001

# View cluster resources
kubectl top pods -n aggregators  # CPU/memory usage
kubectl describe pod -n aggregators amm-eth-xxxxx  # Full pod info
```

## How to Update a Secret (e.g., rotate API key)

```bash
# DON'T edit files - edit GitHub Settings instead!

# 1. Go to GitHub Settings → Secrets and variables → Actions
# 2. Find K8S_MEXC_API_KEY
# 3. Click "Update secret"
# 4. Paste new value
# 5. Next git push will use new secret ✅
```

## How to Manually Restart a Service

```bash
# Restart amm-eth (kills and restarts pods)
kubectl rollout restart deployment/amm-eth -n aggregators

# Watch it come back up
kubectl get pods -n aggregators -w
```

## How to Check What's Actually Running

```bash
# SSH into your server
ssh root@your-server-ip

# Check Docker images
docker images

# Check Docker registry contents
curl http://localhost:5000/v2/_catalog

# Check k3s nodes
sudo k3s kubectl get nodes

# Check all resources
sudo k3s kubectl get all -n aggregators
```

## How to Scale Your Services

```bash
# Scale amm-eth to 3 replicas
kubectl scale deployment amm-eth --replicas=3 -n aggregators

# Auto-scale based on CPU (requires metrics-server, included in k3s)
kubectl autoscale deployment arbitrade-eth \
  --min=1 --max=5 --cpu-percent=80 -n aggregators
```

---

# PHASE 6: TROUBLESHOOTING

## Pod stuck in "ImagePullBackOff"

```bash
# Means it can't pull the image from localhost:5000
# Check if registry is running on server
ssh root@your-server-ip
docker ps | grep registry

# If not running, restart it
docker start registry

# Then delete the pod so k3s tries again
kubectl delete pod -n aggregators -l app=amm-eth
```

## Can't connect to arbitrade-eth API

```bash
# Check if service is up
kubectl get svc -n aggregators

# Check if pod is ready
kubectl get pods -n aggregators

# Test from within the cluster
kubectl exec -n aggregators -it amm-eth-xxxxx -- bash
curl http://arbitrade-eth-service:3001/health

# Test port forward
kubectl port-forward -n aggregators svc/arbitrade-eth-service 3001:3001
# Then: curl http://localhost:3001/health
```

## Check why pod won't start

```bash
# View pod details
kubectl describe pod -n aggregators amm-eth-xxxxx

# View pod logs
kubectl logs -n aggregators amm-eth-xxxxx

# If previous pod crashed, view old logs
kubectl logs -n aggregators amm-eth-xxxxx --previous
```

---

# SUMMARY

## What Lives Where

| Component | Location | Setup Time |
|-----------|----------|-----------|
| k3s cluster | Dedicated server | 5 min |
| Docker registry | Dedicated server | 2 min |
| kubeconfig | Local machine | 2 min |
| k8s manifests | Git repo | 15 min |
| CI/CD workflow | GitHub Actions | 10 min |

## Typical Workflow

```
You: git push origin main
     ↓
GitHub Actions: build Docker images, run tests
     ↓
GitHub Actions: push images to localhost:5000 on server
     ↓
GitHub Actions: kubectl apply new deployment
     ↓
Server: k3s pulls new images and updates pods
     ↓
Your aggregators are live with new code
```

## Cost Comparison

| Service | Your Setup (k3s) | AWS EKS | Google GKE |
|---------|---|---|---|
| Cluster | Included with server | $73/month | $73/month |
| Nodes | Your server | $40+/month each | $40+/month each |
| Image registry | Free (Docker) | ~$0.10/GB stored | Free (GCP) |
| Control plane | Free (k3s) | $73 | Free |
| **Total/month** | $0-100 (your server) | $200+ | $100+ |

**k3s is 50-80% cheaper than managed Kubernetes!**

---

## Next Steps

1. ✅ Set up k3s on your server (Step 1-7)
2. ✅ Copy kubeconfig to local machine (Step 8-10)
3. ✅ Create k8s manifests (Step 11)
4. ✅ Deploy manually (Step 12-13)
5. ✅ Set up GitHub Actions CI/CD (Step 14-15)
6. ✅ Test the pipeline (Step 16)

You now have a Kubernetes setup that's:
- ✅ Similar to AWS EKS / Google GKE
- ✅ Fully automated with CI/CD
- ✅ Cheap to run
- ✅ Easy to learn from
- ✅ Scalable to multi-node clusters later

---

# BONUS: LENS IDE QUICK START (For Visual Management)

> **TL;DR:** Lens is like VS Code for Kubernetes. One-click install, see your whole cluster visually, no CLI commands needed.

## Why Lens Over kubectl Commands?

**Without Lens (Using kubectl):**
```bash
# Check if pods are running
kubectl get pods -n aggregators

# See logs (and forget the exact command)
kubectl logs -n aggregators deployment/amm-eth -f

# SSH into a container
kubectl exec -it amm-eth-xxxxx -n aggregators -- /bin/bash

# Forward port to test
kubectl port-forward svc/arbitrade-eth-service 3001:3001

# Check resource usage
kubectl top pods -n aggregators

# View all at once? Nope. Run each command separately.
```

**With Lens IDE:**
```
Click your cluster → See everything at once:
├─ Live list of all pods (with status colors)
├─ Real-time CPU/memory for each pod
├─ All logs in one place
├─ Terminal to any pod (one click)
├─ Port forwarding (one click)
└─ All in a beautiful desktop app
```

---

## Step 1: Install Lens IDE

**macOS:**
```bash
brew install lens
```

**Linux:**
```bash
# Download from https://k8slens.dev
# Or use snap
sudo snap install lens --classic
```

**Windows:**
```
Download from https://k8slens.dev
```

---

## Step 2: Add Your k3s Cluster to Lens

**Open Lens**, click **"Clusters"** in left sidebar:

```
┌─────────────────────────────────────────┐
│ Lens                                     │
├─────────────────────────────────────────┤
│ Clusters        [+ Add Cluster]          │
│  ┌─────────────────────────────────────┐ │
│  │ No clusters added yet                │ │
│  │ Click "Add Cluster" to get started   │ │
│  └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
```

Click **"+ Add Cluster"** → Select **"Local kubeconfig"**:

```
┌─────────────────────────────────────────┐
│ Add Cluster                              │
├─────────────────────────────────────────┤
│ Select kubeconfig file:                  │
│                                          │
│ [Browse...]                              │
│ (Navigate to ~/.kube/k3s-config.yaml)   │
│                                          │
│ [Add Cluster]                            │
└─────────────────────────────────────────┘
```

Navigate to `~/.kube/k3s-config.yaml` and select it.

---

## Step 3: You're Connected! Here's What You See

Once connected, Lens shows:

```
┌──────────────────────────────────────────────────────┐
│ 🔵 k3s-config (your-server-ip)                        │
├──────────────────────────────────────────────────────┤
│                                                       │
│ CLUSTER STATUS                                        │
│ ├─ Nodes: 1                                           │
│ ├─ Pods: 5                                            │
│ ├─ CPU Usage: 35% (140m / 400m available)           │
│ ├─ Memory Usage: 40% (1.2Gi / 3Gi available)        │
│ └─ Storage: Healthy                                  │
│                                                       │
│ QUICK NAVIGATION (Left Sidebar)                       │
│ ├─ Cluster                                            │
│ │  ├─ Nodes (see your server health)                │
│ │  ├─ Storage                                        │
│ │  └─ Events                                         │
│ │                                                    │
│ ├─ Namespaces                                        │
│ │  ├─ aggregators ← (Your stuff is here)            │
│ │  │  ├─ Deployments                                │
│ │  │  ├─ Pods                                        │
│ │  │  ├─ Services                                    │
│ │  │  ├─ ConfigMaps                                 │
│ │  │  ├─ Secrets                                    │
│ │  │  └─ Storage                                    │
│ │  │                                                │
│ │  └─ kube-system (Kubernetes internals)            │
│ │     ├─ Deployments                                │
│ │     └─ Pods                                        │
│ │                                                    │
│ └─ Extensions (Add-ons like metrics)                │
│                                                       │
└──────────────────────────────────────────────────────┘
```

---

## Step 4: Common Tasks in Lens

### **Task 1: Check If Your Services Are Running**

```
Left Sidebar:
1. Click "aggregators" namespace
2. Click "Deployments"
3. You see:

┌────────────────────────────────────────┐
│ Deployments in aggregators             │
├────────────────────────────────────────┤
│ amm-eth          ✅ 1/1 Ready          │
│ └─ Updated 2 min ago                   │
│ └─ 1 replica running                   │
│ └─ Click to see details                │
│                                         │
│ arbitrade-eth    ✅ 1/1 Ready          │
│ └─ Updated 5 min ago                   │
│ └─ 1 replica running                   │
│ └─ Click to see details                │
└────────────────────────────────────────┘
```

### **Task 2: View Live Logs**

```
Left Sidebar:
1. Click "aggregators" namespace
2. Click "Pods"
3. You see:

┌────────────────────────────────────────┐
│ Pods in aggregators                    │
├────────────────────────────────────────┤
│ amm-eth-5d4rf                          │
│ ├─ Status: Running ✅                 │
│ ├─ CPU: 12%  |  Memory: 256Mi        │
│ ├─ Uptime: 2h 15m                     │
│ └─ Restarts: 0                        │
│                                         │
│ arbitrade-eth-7x9km                    │
│ ├─ Status: Running ✅                 │
│ ├─ CPU: 25%  |  Memory: 512Mi        │
│ ├─ Uptime: 1h 50m                     │
│ └─ Restarts: 0                        │
└────────────────────────────────────────┘

Double-click a pod → Opens details:

┌────────────────────────────────────────┐
│ amm-eth-5d4rf (Pod Details)            │
├────────────────────────────────────────┤
│ [Overview] [Logs] [Terminal] [YAML]   │
│                                         │
│ Logs Tab (Click it):                   │
├────────────────────────────────────────┤
│ $ tail -f /app/logs.txt                │
│ [INFO] Starting WebSocket server       │
│ [INFO] Listening on 0.0.0.0:8080      │
│ [INFO] Connected to ETH node           │
│ [DEBUG] Price update: 2350.50 USD      │
│ [DEBUG] Price update: 2350.75 USD      │
│ (Logs update in real-time)             │
└────────────────────────────────────────┘
```

### **Task 3: SSH Into a Container**

```
Same Pod Details window:

├────────────────────────────────────────┤
│ [Overview] [Logs] [Terminal] [YAML]   │
│                                         │
│ Terminal Tab (Click it):               │
├────────────────────────────────────────┤
│ $ bash-5.0#                            │
│ $ ls -la                               │
│ $ pwd                                  │
│ /app                                   │
│ $ cat rocksdb_data/IDENTITY            │
│ $ ps aux                               │
│ (It's like SSH! Type commands)        │
│ $ exit                                 │
└────────────────────────────────────────┘
```

### **Task 4: Test Your API with Port Forward**

```
Right-click on service: arbitrade-eth-service

┌────────────────────────────────────────┐
│ arbitrade-eth-service (Context Menu)   │
├────────────────────────────────────────┤
│ ✓ Port Forward                         │
│   localhost:3001 → pod:3001            │
└────────────────────────────────────────┘

Lens starts port-forward automatically.

Open your browser:
http://localhost:3001/health
```

### **Task 5: Check Resource Usage**

```
Top of Lens window shows:

CPU: 37m / 400m (9%)      ← Small indicator
Memory: 1.2Gi / 3Gi (40%) ← Shows you're not overloaded

Click "Cluster" in sidebar → "Nodes":

┌────────────────────────────────────────┐
│ Nodes                                  │
├────────────────────────────────────────┤
│ your-server (Control Plane + Worker)   │
│ ├─ Status: Ready ✅                   │
│ ├─ CPU: 37m / 400m (9%)               │
│ ├─ Memory: 1.2Gi / 3Gi (40%)          │
│ ├─ Disk: 50Gi / 500Gi (10%)           │
│ ├─ Pods Running: 5 pods                │
│ └─ Click for more details              │
└────────────────────────────────────────┘
```

### **Task 6: View ConfigMaps & Secrets**

```
Left Sidebar:
1. Click "aggregators"
2. Scroll down to "ConfigMaps"
3. You see:

┌────────────────────────────────────────┐
│ ConfigMaps in aggregators              │
├────────────────────────────────────────┤
│ amm-eth-config                         │
│ ├─ RUST_LOG: info                      │
│ ├─ ETH_PRICE_WS_PORT: 8080            │
│ └─ (Click to edit/update)              │
└────────────────────────────────────────┘

Same for Secrets:

┌────────────────────────────────────────┐
│ Secrets in aggregators                 │
├────────────────────────────────────────┤
│ api-keys-secret                        │
│ ├─ (Values hidden for security)        │
│ └─ (Click to edit if needed)           │
└────────────────────────────────────────┘
```

---

## Step 5: Keyboard Shortcuts (Pro Tips)

| Action | Shortcut |
|--------|----------|
| Search resources by name | `Cmd + K` (macOS) / `Ctrl + K` (Linux) |
| Quick view pod logs | Select pod, then `Cmd + L` |
| Open terminal in pod | Select pod, then `Cmd + T` |
| Refresh current view | `Cmd + R` / `F5` |
| Go back to cluster overview | Click "Cluster" in sidebar |

---

## Step 6: Real-World Example - Debugging a Deployment

**Scenario:** Your arbitrade-eth pod keeps crashing. What do you do?

**With kubectl:**
```bash
# Check if pod is running
kubectl get pods -n aggregators

# See why it crashed
kubectl logs -n aggregators arbitrade-eth-xxxxx --previous

# Check pod details
kubectl describe pod -n aggregators arbitrade-eth-xxxxx

# Check deployment
kubectl describe deployment -n aggregators arbitrade-eth

# Check events
kubectl get events -n aggregators
```

**With Lens (6 clicks):**
1. Click "aggregators" namespace
2. Click "Pods"
3. Double-click "arbitrade-eth-xxxxx"
4. Click "Logs" tab → See what crashed
5. Click "Overview" tab → See event history
6. Right-click pod → "Delete" → Kubernetes auto-restarts it
7. Watch new logs appear in real-time

**Lens is 10x faster for debugging!**

---

## Step 7: Useful Lens Features for Your Setup

### **Lens Chart: Monitor CPU/Memory Over Time**

```
Click "Cluster" → See graphs showing:
├─ CPU usage (last 1 hour)
├─ Memory usage (last 1 hour)
├─ Network I/O
└─ Disk usage
```

### **Hot Reload ConfigMaps**

```
Click ConfigMap → Edit → Save
Kubernetes auto-reloads it into your pods!
```

### **View Persistent Volumes**

```
Click "Storage" in sidebar:
├─ amm-eth-pvc (10Gi, 2Gi used)
├─ arbitrade-eth-pvc (10Gi, 1.5Gi used)
└─ (See if you're running out of space)
```

---

## Lens vs kubectl Comparison

| Task | kubectl | Lens |
|------|---------|------|
| View all pods | `kubectl get pods` | Click "Pods" |
| See pod logs | `kubectl logs deployment/xxx` | Click pod → Logs tab |
| SSH into pod | `kubectl exec -it pod -- bash` | Click pod → Terminal tab |
| Port forward | `kubectl port-forward svc/xxx 3001:3001` | Right-click service → Port Forward |
| Check resource usage | `kubectl top pods` | See in UI automatically |
| Edit ConfigMap | `kubectl edit cm xxx` | Click ConfigMap → Edit |
| View events | `kubectl get events` | Click "Events" in sidebar |
| Restart deployment | `kubectl rollout restart deployment/xxx` | Click deployment → Restart |
| **Learning curve** | Steep (80 commands to remember) | Gentle (UI discovery) |
| **Speed** | Fast (typing) | Faster (clicking + visual) |

---

## When to Use Lens vs kubectl

**Use Lens when:**
- ✅ You're debugging issues
- ✅ You want to see your cluster visually
- ✅ You're checking logs/resource usage
- ✅ You're SSH-ing into containers
- ✅ You're sharing status with team (take screenshots)

**Use kubectl when:**
- ✅ You're writing scripts/automation
- ✅ You're in a terminal-only environment (server SSH)
- ✅ You need exact control over YAML edits
- ✅ You're running CI/CD commands

**Pro Tip:** Use BOTH! Lens for daily work, kubectl for scripts.

---

## Lens IDE Summary

**Install:** `brew install lens`
**Setup:** Add cluster → Select `~/.kube/k3s-config.yaml`
**Result:** Beautiful visual dashboard for your k3s cluster

You now have **the same visibility as AWS/GCP consoles**, but for your own cluster!

---

Questions? Ask in the next message!
