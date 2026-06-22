# CODEX_TESTING.md — RustyBMP Automated Test Runbook
# Replaces: docs/UBUNTU_TESTING.md (deprecated as of RV9)
# All commands run from repo root on Ubuntu 24.04.
# Machine-readable JSON results written to runtime/test_results/<layer>.json

## Prerequisites (one-time)
```
sudo apt-get install -y cmake pkg-config libssl-dev libcurl4-openssl-dev python3 python3-pip duckdb
cargo build --workspace
python3 -m venv .venv
.venv/bin/pip install pytest requests httpx pytest-asyncio pytest-json-report websockets pydantic duckdb pandas pyarrow respx networkx
```

Notes:
- On Ubuntu 24.04, system Python may be PEP 668 managed. Prefer a repo-local virtualenv over `pip3 install` into the system interpreter.
- `cargo test --workspace -- --test-output immediate` is not valid for the current test binaries here. Use `cargo test --workspace` or `cargo test --workspace -- --nocapture`.

---

## Layer 0 — Rust Unit Tests (<30s)
```
# Run: always, no dependencies
cargo test --workspace
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
# Requires: cargo build -p rbmp-server --bins
# Use the local test config committed/created for protocol runs.
./target/debug/rustybmp runtime/protocol-test.toml &
SERVER_PID=$!
sleep 0.5
.venv/bin/python -m pytest tests/protocol/ -v \
  --json-report --json-report-file=runtime/test_results/layer2.json
EXIT=$?
kill $SERVER_PID 2>/dev/null
exit $EXIT
# Pass: exit 0 + layer2.json has "passed" == total
```

---

## Layer 3 — API Contract + ML Unit Tests (<90s)
```
# API tests require: running rustybmp in test mode on the default port expected by tests
RUSTYBMP_TEST_MODE=1 ./target/debug/rustybmp runtime/api-test.toml &
SERVER_PID=$!
sleep 0.5
.venv/bin/python -m pytest tests/ml/ tests/api/ -v \
  --ignore=tests/scenarios \
  --json-report --json-report-file=runtime/test_results/layer3.json
EXIT=$?
kill $SERVER_PID 2>/dev/null
exit $EXIT
# Pass: exit 0 + layer3.json has no failures
# Current observed scope in Ubuntu session: tests/ml/ (67 passed), tests/api/ (24 passed)
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
# Requires: server running + UI deps installed + Playwright Chromium installed
# Step 1: start server in test mode
RUSTYBMP_TEST_MODE=1 ./target/debug/rustybmp runtime/api-test.toml &
SERVER_PID=$!

# Step 2: seed deterministic test data
curl -sS -H 'Content-Type: application/json' \
  -d '{"fixture":"standard","truncate":true}' \
  http://127.0.0.1:7878/api/_test/seed

# Step 3: install deps and browsers (first run only)
cd ui
npm ci
npx playwright install chromium

# Step 4: run tests (Playwright will launch vite dev server via webServer config)
BASE_URL=http://127.0.0.1:5173 npx playwright test
EXIT=$?
cd ..

# Cleanup
kill $SERVER_PID 2>/dev/null
exit $EXIT
# Pass: exit 0 + all tests green (current Ubuntu baseline: 22 passed)
# Reports: ui/test-results/results.json and ui/playwright-report/index.html
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
RUSTYBMP_TEST_MODE=1 ./target/debug/rustybmp runtime/api-test.toml &
SERVER_PID=$!
sleep 1
.venv/bin/python -m pytest tests/protocol/ -v --json-report --json-report-file=runtime/test_results/layer2.json
.venv/bin/python -m pytest tests/ml/ tests/api/ -v --ignore=tests/scenarios --json-report --json-report-file=runtime/test_results/layer3.json
kill $SERVER_PID 2>/dev/null
echo "All non-lab layers passed"
```
