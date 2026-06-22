# CODEX_TESTING.md — RustyBMP Automated Test Runbook
# Replaces: docs/UBUNTU_TESTING.md (deprecated as of RV9)
# All commands run from repo root on Ubuntu 24.04.
# Machine-readable JSON results written to runtime/test_results/<layer>.json

## Prerequisites (one-time)
```
sudo apt-get install -y cmake pkg-config libssl-dev python3 python3-pip duckdb
cargo build --workspace
pip3 install pytest requests httpx pytest-asyncio pytest-json-report websockets
```

---

## Layer 0 — Rust Unit Tests (<30s)
```
# Run: always, no dependencies
cargo test --workspace -- --test-output immediate
# Pass: exit 0, all green
# Fail: any "FAILED" line or exit !=0
```

---

## Layer 1 — Wiring Checks (<15s)
```
bash scripts/check_wiring.sh
# Pass: exit 0, prints "All wiring checks passed"
# Fail: "WIRING CHECK FAILED: N error(s)" + exit 1
```

---

## Layer 2 — Protocol Integration (<60s)
```
# Requires: cargo build --workspace (collector binary present)
./target/debug/rbmp-collector \
  --bmp-port 17878 --api-port 17879 \
  --db :memory: --no-auth &
COLLECTOR_PID=$!
sleep 0.5
python3 -m pytest tests/protocol/ -v \
  --json-report --json-report-file=runtime/test_results/layer2.json
EXIT=$?
kill $COLLECTOR_PID 2>/dev/null
exit $EXIT
# Pass: exit 0 + layer2.json has "passed" == total
```

---

## Layer 3 — API Contract + ML Unit Tests (<90s)
```
# API tests require: running server + seed.sql loaded
duckdb /tmp/rbmp_test.duckdb < tests/seed.sql
./target/debug/rbmp-server \
  --db /tmp/rbmp_test.duckdb \
  --api-port 17880 --no-auth &
SERVER_PID=$!
sleep 0.5
python3 -m pytest tests/ml/ tests/api/ -v \
  --ignore=tests/scenarios \
  --json-report --json-report-file=runtime/test_results/layer3.json
EXIT=$?
kill $SERVER_PID 2>/dev/null
exit $EXIT
# Pass: exit 0 + layer3.json has no failures
# Current scope: tests/ml/ (34 tests), tests/api/ (when populated)
```

---

## Layer 4 — FRR Smoke Lab (<3min)
```
# Requires: containerlab, docker, quay.io/frrouting/frr:10.6.1
docker pull quay.io/frrouting/frr:10.6.1
docker build -t rustybmp:latest .
python3 -m pytest tests/scenarios/01_frr_minimal/ -v --timeout=180 \
  --json-report --json-report-file=runtime/test_results/layer4.json
# Pass: exit 0 + all TestFrrSmoke tests green
# Skip: if containerlab binary not found, tests auto-skip with pytest.mark.skipif
```

---

## Layer 5 — XRd RFC 9972 Stats Lab (<5min)
```
# Requires: containerlab, docker, ios-xr/xrd-control-plane:24.2.1 (Cisco license required)
python3 -m pytest tests/scenarios/02_xrd_rfc9972/ -v --timeout=300 \
  --json-report --json-report-file=runtime/test_results/layer5.json
# Pass: exit 0 + all TestXrdRfc9972 tests green
# Note: requires XRd license — skip in open CI
```

---

## Layer 7 — UI End-to-End Playwright (<5min)
```
# Requires: server running + UI built + Playwright browsers installed
# Step 1: start server
./target/debug/rbmp-server \
  --db /tmp/rbmp_test.duckdb \
  --api-port 7878 --no-auth &
SERVER_PID=$!

# Step 2: build and preview UI
cd ui
npm ci
npm run build
npx vite preview --port 5173 &
cd ..
sleep 3

# Step 3: install browsers (first run only)
cd ui && npx playwright install chromium && cd ..

# Step 4: run tests
cd ui
BASE_URL=http://localhost:5173 npx playwright test \
  --reporter=json,github \
  --output=../runtime/test_results/layer7.json
EXIT=$?
cd ..

# Cleanup
kill $SERVER_PID 2>/dev/null
exit $EXIT
# Pass: exit 0 + 26+ tests green
# Report: ui/playwright-report/index.html
```

---

## Mapping: Codex failure → fix
```
layer0.json  → cargo test failure → fix Rust unit in crates/
layer2.json  → protocol test fail → fix rbmp-core BMP parsing
layer3.json  → ML/API test fail   → fix bmppy/rbmppy/ module
layer4.json  → FRR smoke fail     → fix tests/scenarios/01_frr_minimal/
layer7.json  → Playwright fail    → find data-testid in ui/src/routes/ and fix
```

---

## Full sequential run (all non-containerlab layers)
```
set -e
mkdir -p runtime/test_results
cargo test --workspace
bash scripts/check_wiring.sh
python3 -m pytest tests/ml/ -v --json-report --json-report-file=runtime/test_results/layer3.json
echo "All non-lab layers passed"
```
