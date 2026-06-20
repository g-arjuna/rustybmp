# Ubuntu Testing Runbook — RustyBMP RV4

Tested on: **Ubuntu 24.04 LTS** (x86-64), XRd 24.x in ContainerLab.

---

## 1. Prerequisites

```bash
# System deps
sudo apt update
sudo apt install -y build-essential cmake pkg-config libssl-dev \
                    git curl ca-certificates containerlab

# Rust (1.85+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup toolchain install 1.85

# Python 3.12 + bmppy SDK
sudo apt install -y python3.12 python3.12-venv python3-pip
python3 -m venv ~/rustybmp-venv
source ~/rustybmp-venv/bin/activate
pip install -e ./bmppy[dev]
```

---

## 2. Build

```bash
# Debug (fast, for tests)
cargo build --workspace

# Release (for production / Docker)
cargo build --release --bin rustybmp --bin rbmp-collector
```

---

## 3. Unit + integration tests

```bash
# All workspace unit tests
cargo test --workspace

# Integration tests only (requires compiled workspace)
cargo test --test integration -- --nocapture

# Benchmarks (criterion HTML report in target/criterion/)
cargo bench --bench bmp_parse
```

Expected baseline (Intel Xeon @ 2.5 GHz):
- Initiation PDU:    > 5 M msgs/sec
- Route Monitor:     > 1 M msgs/sec

---

## 4. ContainerLab + XRd smoke test

```bash
# 1. Start lab
sudo containerlab deploy --topo lab/xrd-bmp.clab.yml

# 2. Start rustybmp
cargo run --release -- config/rustybmp.toml.example

# 3. Verify BMP session (XRd sends BMP to :5000)
curl http://localhost:7878/health
curl http://localhost:7878/api/peers | python3 -m json.tool

# 4. Inject a route flap
bash lab/scenarios/flap_peer.sh

# 5. Check route events recorded
curl "http://localhost:7878/api/routes?limit=20" | python3 -m json.tool

# 6. BGP-LS graph (if XRd is configured with BGP-LS)
curl http://localhost:7878/api/bgpls/graph | python3 -m json.tool
```

---

## 5. Security smoke test (JWT auth)

Edit `rustybmp.toml` to enable auth:

```toml
[auth]
enabled         = true
jwt_secret      = "my-32-byte-prod-secret-change-me!!"
api_keys        = ["test-api-key-abc123"]
token_ttl_secs  = 3600
```

```bash
# Get a token
TOKEN=$(curl -s -X POST http://localhost:7878/auth \
  -H 'Content-Type: application/json' \
  -d '{"api_key":"test-api-key-abc123"}' | jq -r .token)

# Use the token
curl -H "Authorization: Bearer $TOKEN" http://localhost:7878/api/peers

# Should 401 without token
curl -o /dev/null -w "%{http_code}" http://localhost:7878/api/peers
```

---

## 6. Parquet export test

```bash
# Export last 7 days to Parquet
curl "http://localhost:7878/api/export/parquet?table=route_events" \
     -o /tmp/routes_export.parquet

# Inspect with DuckDB CLI
duckdb -c "SELECT COUNT(*), MIN(occurred_at), MAX(occurred_at) FROM '/tmp/routes_export.parquet'"

# Or via Python
python3 -c "
import duckdb
conn = duckdb.connect()
df = conn.execute(\"SELECT * FROM '/tmp/routes_export.parquet' LIMIT 5\").df()
print(df)
"
```

---

## 7. NATS smoke test

```bash
# Start NATS server
docker run -d --name nats -p 4222:4222 nats:2.10-alpine -js

# Enable NATS in config
cat >> rustybmp.toml <<'EOF'
[nats]
enabled        = true
server         = "nats://localhost:4222"
subject_prefix = "rustybmp"
EOF

# Subscribe to route events
nats sub 'rustybmp.unicast_prefix' &

# Restart rustybmp and inject traffic
cargo run --release -- rustybmp.toml
```

---

## 8. UI development server

```bash
cd ui
npm install
npm run dev
# Open http://localhost:5173
```

---

## 9. Docker end-to-end

```bash
docker compose build
docker compose up -d rustybmp
sleep 5
curl http://localhost:7878/health
docker compose logs rustybmp --tail=50
docker compose down
```

---

## 10. Python ML pipeline test

```bash
source ~/rustybmp-venv/bin/activate

# Export training data (requires populated DuckDB)
python -m rbmppy.parquet \
  --db runtime/routes.duckdb \
  --out ml/data \
  --days 7

# Train anomaly model
python bmppy/ml/train_route_anomaly.py \
  --input ml/data/routes_7d.parquet \
  --output ml/models/route_anomaly_v1.joblib

# Build topology snapshot (requires networkx)
python3 - <<'EOF'
from rbmppy.analytics import RouteAnalytics
from rbmppy.topology import BgpLsTopology, AsTopology
a = RouteAnalytics("runtime/routes.duckdb")
topo = BgpLsTopology(a)
print(topo.summary() if hasattr(topo, 'summary') else topo.to_dict())
EOF
```
