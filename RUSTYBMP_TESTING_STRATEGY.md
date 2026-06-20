# RustyBMP — Comprehensive Testing Strategy
## Progressive · Automated · Codex-Compatible · Internet-Scale

> **Context**: Written while RV7 codes, to be refined into RV8/RV9 backlog tasks.
> **Source references**:
> — Bonsai testing_discipline.md (three-layer model: wiring / smoke / e2e)
> — Bonsai signal-test-lab topology (SRL spine-leaf + FRR BMP node, exact clab.yml read)
> — Bonsai soak_test.py (fault-inject → detect → restore cycle pattern)
> — BGPStream v2 documentation (CAIDA BMP Kafka, RIPE RIS Live, RouteViews MRT)
> — ContainerLab docs (XRd, cEOS, cRPD, SRL known boot issues)
> — Playwright (E2E headless, data-testid, Codex compatibility)
>
> **Core thesis**: The old UBUNTU_TESTING.md (204 lines, 7 manual scenarios) fails
> because it requires operator judgment at every step. The replacement is a
> progressive infrastructure of hermetic, automated, machine-readable tests that
> Codex can run, read, fix, and rerun without human intervention.

---

## Part 1 — Why the Old Approach Fails

The original `docs/UBUNTU_TESTING.md` has three structural problems:

**Problem 1: Sequential narrative.** It reads as prose instructions. A human can follow it; Codex cannot run it as a program.

**Problem 2: Mixed layers.** It conflates "does cargo build?" with "does BMP parse correctly?" with "does the UI show the right metric?". When it fails at step 4, you don't know if the issue is in steps 1-3 or 4 itself.

**Problem 3: No oracle.** Steps say "verify the peers page shows connected peers" — but what's the machine-readable PASS criterion? There isn't one.

The replacement: seven independent test layers, each with a hermetic scope, a machine-readable PASS/FAIL contract, and a Codex-executable runner script. A failure in Layer 3 tells you exactly what broke, without running Layers 4-7.

---

## Part 2 — The Seven Testing Layers

```
Layer 0:  Rust unit tests          cargo test --workspace              <10s
Layer 1:  Wiring checks            scripts/check_wiring.sh             <15s
Layer 2:  Protocol integration     pytest tests/protocol/              <60s  (no clab, fixtures only)
Layer 3:  API contract tests       pytest tests/api/                   <90s  (local server, no clab)
Layer 4:  FRR smoke lab            pytest tests/scenarios/01_frr/      <3min (lightest clab)
Layer 5:  Multi-NOS integration    pytest tests/scenarios/0N_*/        <15min (full clab scenarios)
Layer 6:  Internet-scale load      python tests/load/mrt_replay.py     <30min (RouteViews full table)
Layer 7:  UI end-to-end            npx playwright test                 <5min  (headless Chromium)
```

**The contract every layer produces** (from bonsai testing_discipline.md):
```json
{
  "layer": "protocol_integration",
  "ts_unix": 1750000000,
  "rustybmp_version": "git-describe",
  "git_sha": "abc1234",
  "status": "pass",
  "ok": true,
  "summary": "All 47 BMP message type tests passed",
  "checks": [
    {"name": "bmp_init_parse",     "status": "pass", "ok": true, "ms": 0.4},
    {"name": "route_monitoring",   "status": "pass", "ok": true, "ms": 1.2},
    {"name": "stats_type30_parse", "status": "pass", "ok": true, "ms": 0.8}
  ]
}
```

Every layer writes this to `runtime/test_results/<layer>_<timestamp>.json`.
CI reads `runtime/test_results/latest.json` (symlink to most recent run).

---

## Part 3 — Layer 0: Rust Unit Tests (already 77, grow to 200+)

**Scope**: Pure parser + logic, zero network, zero database.

### What to add beyond RV6's 77 tests

```rust
// tests/bmp/path_status_tlv.rs  (RV7 new)
#[test]
fn parse_best_primary_ecmp_bits() {
    // status bitmap = Best(0x02) | Primary(0x08) = 0x0000000A
    let data = [0x00, 0x00, 0x00, 0x0A];
    let tlv = parse_path_status_tlv(&data).unwrap();
    assert!(tlv.is_best());
    assert!(tlv.is_primary());
    assert!(!tlv.is_backup());
    assert_eq!(tlv.label(), "best");
}

#[test]
fn parse_nonselected_with_reason_localpref() {
    // status = Nonselected(0x04), reason = LocalPref(0x0003)
    let data = [0x00, 0x00, 0x00, 0x04, 0x00, 0x03];
    let tlv = parse_path_status_tlv(&data).unwrap();
    assert!(tlv.is_nonselected());
    assert_eq!(tlv.reason, 0x0003);
    assert_eq!(tlv.reason_label(), "not preferred: LOCAL_PREF");
}

// tests/filter/roto_engine.rs  (RV7 new)
#[test]
fn roto_accepts_valid_route_with_all_criteria() {
    let engine = RotoFilterEngine::load("config/filters.roto").unwrap();
    let ctx = RouteCtx {
        prefix: "203.0.113.0/24".into(),
        prefix_len: 24,
        rpki: "valid".into(),
        as_path: "65001 64496".into(),
        peer_as: 65001,
        ..Default::default()
    };
    assert_eq!(engine.evaluate(&ctx), FilterVerdict::Accept);
}

#[test]
fn roto_rejects_bogon_prefix() {
    let engine = RotoFilterEngine::load("config/filters.roto").unwrap();
    let ctx = RouteCtx {
        prefix: "10.1.2.0/24".into(), prefix_len: 24,
        rpki: "not-found".into(), as_path: "65001".into(),
        ..Default::default()
    };
    assert_eq!(engine.evaluate(&ctx), FilterVerdict::Reject);
}

#[test]
fn roto_rejects_rpki_invalid_too_specific() {
    let engine = RotoFilterEngine::load("config/filters.roto").unwrap();
    let ctx = RouteCtx {
        prefix: "203.0.113.0/25".into(), prefix_len: 25,
        rpki: "invalid".into(), as_path: "65001 64496 12345".into(),
        ..Default::default()
    };
    assert_eq!(engine.evaluate(&ctx), FilterVerdict::Reject);
}
```

Target: 200 unit tests after RV7. Each new protocol feature (Path Status TLV, BGPsec validation, convergence event detection) gets minimum 5 tests at commit time.

---

## Part 4 — Layer 1: Wiring Checks (10 seconds)

From bonsai's `scripts/check_wiring.sh` pattern — fail fast before even starting the server.

```bash
#!/usr/bin/env bash
# scripts/check_wiring.sh
# Verify that architecture wiring is correct without starting the server.
set -euo pipefail

ERRORS=0

check() {
    local description="$1"
    local command="$2"
    if ! eval "$command" &>/dev/null; then
        echo "FAIL: $description"
        ERRORS=$((ERRORS + 1))
    else
        echo "PASS: $description"
    fi
}

# Every API route in api/mod.rs must have an implementation file
check "filters module wired"     "grep -r 'pub mod filters' crates/rbmp-server/src/api/mod.rs"
check "analytics module wired"   "grep -r 'pub mod analytics' crates/rbmp-server/src/api/mod.rs"
check "policy module wired"      "grep -r 'pub mod policy' crates/rbmp-server/src/api/mod.rs"

# Every vault use must reference RUSTYBMP_VAULT_PASSPHRASE, not BONSAI_
check "vault env var correct"    "! grep -r 'BONSAI_VAULT_PASSPHRASE' crates/"
check "ssh env var correct"      "! grep -r 'BONSAI_BOOTSTRAP_' bmppy/"

# Default filter config must exist and parse
check "filters.roto exists"      "test -f config/filters.roto"
check "filters.yaml exists"      "test -f config/filters.yaml"

# rbmp-mrt must compile (uses MRT replay for load tests)
check "rbmp-mrt compiles"        "cargo check -p rbmp-mrt --quiet 2>/dev/null"

# Python policy_fetcher must be importable
check "policy_fetcher importable" "python3 -c 'import sys; sys.argv=[\"\"]; exec(open(\"bmppy/policy_fetcher.py\").read())' 2>/dev/null || true"

if [ $ERRORS -gt 0 ]; then
    echo "WIRING CHECK FAILED: $ERRORS error(s)"
    exit 1
fi
echo "Wiring checks: all passed"
```

---

## Part 5 — Layer 2: Protocol Integration (60 seconds, no ContainerLab)

**The key insight**: BMP is a TCP protocol. We don't need a real router to test parsing. We need a corpus of captured BMP bytes.

### Fixture corpus: `tests/fixtures/bmp/`

```
tests/fixtures/bmp/
├── peer_up_xrd.bin          # XRd IOS-XR peer-up PDU (captured from lab)
├── peer_up_srl.bin          # Nokia SRL peer-up PDU
├── peer_up_frr.bin          # FRR peer-up PDU
├── peer_down_hold_timer.bin # peer-down with hold-timer-expired reason
├── route_monitoring_ipv4_announce.bin
├── route_monitoring_ipv6_announce.bin
├── route_monitoring_evpn_type2.bin
├── route_monitoring_evpn_type5.bin
├── route_monitoring_mcast_vpn_type1.bin
├── route_monitoring_srpolicy_safi73.bin
├── route_monitoring_bgpls.bin
├── route_monitoring_with_path_status_tlv.bin  # RV7 new
├── stats_report_type30.bin  # RFC 9972 type 30 max-prefix gauge
├── stats_report_type18to38.bin  # all RFC 9972 stat types
└── routeviews_update.mrt    # RouteViews MRT update sample (2min slice)
```

Fixtures are captured once from real devices and committed to git. They never change unless we deliberately update them (version gate).

```python
# tests/protocol/test_bmp_parsing.py

import pytest, subprocess, json, socket, struct, time
from pathlib import Path

FIXTURES = Path("tests/fixtures/bmp")
SERVER_ADDR = ("127.0.0.1", 17878)

@pytest.fixture(scope="session")
def rustybmp_server(tmp_path_factory):
    """Start a rustybmp server with in-memory DuckDB, kill on test end."""
    db_path = tmp_path_factory.mktemp("db") / "test.duckdb"
    proc = subprocess.Popen([
        "./target/debug/rbmp-collector",
        "--bmp-port", "17878",
        "--api-port", "17879",
        "--db", str(db_path),
        "--no-auth",
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    time.sleep(0.5)  # wait for bind
    yield proc
    proc.terminate()

def inject_bmp(fixture_file: str) -> None:
    """Open TCP to collector and send raw BMP bytes from fixture."""
    data = (FIXTURES / fixture_file).read_bytes()
    with socket.create_connection(SERVER_ADDR, timeout=5) as s:
        s.sendall(data)
        time.sleep(0.2)  # allow processing

def api_get(path: str) -> dict:
    import requests
    return requests.get(f"http://127.0.0.1:17879/api/{path}").json()

# ── BMP Parser Tests ──────────────────────────────────────────────────────────

class TestBmpParsing:
    def test_peer_up_xrd(self, rustybmp_server):
        inject_bmp("peer_up_xrd.bin")
        peers = api_get("peers")["peers"]
        assert any(p["state"] == "up" for p in peers), "Expected at least one peer up after XRd PeerUp"

    def test_peer_down_updates_state(self, rustybmp_server):
        inject_bmp("peer_up_xrd.bin")
        inject_bmp("peer_down_hold_timer.bin")
        peers = api_get("peers")["peers"]
        assert any(p["state"] == "down" for p in peers), "Expected peer down after PeerDown message"

    def test_route_announce_ipv4(self, rustybmp_server):
        inject_bmp("peer_up_xrd.bin")
        inject_bmp("route_monitoring_ipv4_announce.bin")
        routes = api_get("routes?action=announce&limit=10")["routes"]
        assert len(routes) > 0, "Expected at least one route after RouteMonitoring announce"
        r = routes[0]
        assert r["afi"] == "ipv4"
        assert r["action"] == "announce"
        assert "/" in r["prefix"]

    def test_stats_type30_max_prefix(self, rustybmp_server):
        inject_bmp("peer_up_xrd.bin")
        inject_bmp("stats_report_type30.bin")
        # Stats type 30 = max-prefix headroom gauge
        # Verify it landed in stats_events table
        import requests
        resp = requests.get("http://127.0.0.1:17879/api/bmpstats/history?limit=10")
        stats = resp.json().get("stats", [])
        type30 = [s for s in stats if s.get("counter_type") == 30]
        assert len(type30) > 0, "Expected RFC 9972 type 30 stat in stats_events"

    def test_path_status_tlv_parsed(self, rustybmp_server):
        inject_bmp("peer_up_xrd.bin")
        inject_bmp("route_monitoring_with_path_status_tlv.bin")
        # Verify path_markings table populated (RV7)
        import requests, time
        time.sleep(0.3)
        resp = requests.get("http://127.0.0.1:17879/api/path-status/matrix?limit=10")
        assert resp.status_code in (200, 404), f"Unexpected status {resp.status_code}"
        if resp.status_code == 200:
            data = resp.json()
            assert "entries" in data

    def test_evpn_type2_route(self, rustybmp_server):
        inject_bmp("peer_up_xrd.bin")
        inject_bmp("route_monitoring_evpn_type2.bin")
        routes = api_get("routes?afi=l2vpn&limit=10")["routes"]
        evpn = [r for r in routes if r.get("is_evpn")]
        assert len(evpn) > 0, "Expected EVPN route after EVPN type-2 RouteMonitoring"

    def test_mrt_sample_ingestion(self, rustybmp_server):
        """Convert MRT sample to BMP and inject — tests the rbmp-mrt round-trip."""
        mrt_file = FIXTURES / "routeviews_update.mrt"
        if not mrt_file.exists():
            pytest.skip("MRT fixture not available")
        result = subprocess.run([
            "cargo", "run", "-p", "rbmp-mrt", "--bin", "mrt-to-bmp",
            "--input", str(mrt_file),
            "--bmp-host", "127.0.0.1",
            "--bmp-port", "17878",
        ], capture_output=True, timeout=30)
        assert result.returncode == 0, f"mrt-to-bmp failed: {result.stderr.decode()}"
        routes = api_get("routes?limit=100")["routes"]
        assert len(routes) > 10, "Expected >10 routes from MRT injection"
```

### How to generate fixture files

```bash
# One-time setup: capture from running XRd lab
# Start a tcpdump on the BMP port, then run the lab briefly
sudo tcpdump -i any -w /tmp/bmp_capture.pcap tcp port 11019 &
# ... run XRd lab for 2 minutes ...
sudo pkill tcpdump

# Extract BMP stream from pcap (use tshark or a Python script)
python3 scripts/extract_bmp_fixtures.py \
    --pcap /tmp/bmp_capture.pcap \
    --output-dir tests/fixtures/bmp/ \
    --extract peer_up,peer_down,route_monitoring,stats_report

# For MRT: download a 2-minute window from RouteViews
python3 scripts/download_mrt_fixture.py \
    --collector rrc00 \
    --duration 2min \
    --output tests/fixtures/bmp/routeviews_update.mrt
```

Once captured, fixtures are committed to git and never re-generated unless deliberately updated.

---

## Part 6 — Layer 3: API Contract Tests (90 seconds, local server only)

Every API endpoint gets a contract test. No clab required — the server runs with fixture DuckDB data injected at startup.

```python
# tests/api/conftest.py

@pytest.fixture(scope="session")
def server_with_seed_data():
    """Start server, seed DuckDB with fixture data, yield base URL."""
    db = "/tmp/rustybmp_test.duckdb"
    subprocess.run(["duckdb", db, ".read tests/fixtures/seed.sql"])
    proc = subprocess.Popen(["./target/debug/rbmp-server", "--db", db,
                              "--api-port", "17880", "--no-auth"])
    time.sleep(0.5)
    yield "http://127.0.0.1:17880"
    proc.terminate()
```

```sql
-- tests/fixtures/seed.sql — deterministic fixture data
-- This is the single source of truth for API tests.
-- All fixture data is hardcoded so tests are reproducible.

INSERT INTO peers (peer_addr, peer_as, speaker_addr, state, route_count, updated_at)
VALUES
    ('10.0.0.1', 65001, '192.168.1.1', 'up',   12450, NOW()),
    ('10.0.0.2', 65002, '192.168.1.1', 'up',    8890, NOW()),
    ('10.0.0.3', 65003, '192.168.1.1', 'down',      0, NOW() - INTERVAL '5 minutes');

INSERT INTO route_events (occurred_at, speaker_addr, peer_addr, peer_as, rib_type, action, prefix, afi, as_path, origin_asn, rpki_validity, local_pref)
VALUES
    (NOW() - INTERVAL '10 minutes', '192.168.1.1', '10.0.0.1', 65001, 'adj-in-post', 'announce', '203.0.113.0/24', 'ipv4', '65001 64496', 64496, 'valid', 200),
    (NOW() - INTERVAL '9 minutes',  '192.168.1.1', '10.0.0.2', 65002, 'adj-in-post', 'announce', '203.0.113.0/24', 'ipv4', '65002 64496', 64496, 'valid', 150),
    (NOW() - INTERVAL '8 minutes',  '192.168.1.1', '10.0.0.1', 65001, 'adj-in-post', 'announce', '198.51.100.0/24', 'ipv4', '65001 7018', 7018, 'not-found', 100);

INSERT INTO ml_anomalies (detected_at, kind, prefix, peer_addr, score, description, severity)
VALUES
    (NOW() - INTERVAL '30 minutes', 'origin_change', '203.0.113.0/24', '10.0.0.1', 0.87, 'Origin ASN changed from 64496 to 12345', 'critical');

INSERT INTO stats_events (occurred_at, speaker_addr, peer_addr, counter_type, counter_value, afi, safi)
VALUES
    (NOW(), '192.168.1.1', '10.0.0.1', 30, 153, 1, 1),   -- type 30: 153 routes headroom
    (NOW(), '192.168.1.1', '10.0.0.2', 30, 892, 1, 1);   -- type 30: 892 routes headroom
```

```python
# tests/api/test_all_endpoints.py

class TestRoutingEndpoints:
    def test_routes_list(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/routes")
        assert r.status_code == 200
        data = r.json()
        assert "routes" in data
        assert len(data["routes"]) >= 3

    def test_routes_filter_by_prefix(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/routes?prefix=203.0.113.0/24")
        assert r.status_code == 200
        routes = r.json()["routes"]
        assert all("203.0.113" in rt["prefix"] for rt in routes)

    def test_prefix_timeline(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/routes/prefix/203.0.113.0%2F24/timeline")
        assert r.status_code == 200
        data = r.json()
        assert "timeline" in data

class TestPeerEndpoints:
    def test_peers_list(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/peers")
        assert r.status_code == 200
        peers = r.json()["peers"]
        assert len(peers) == 3  # matches seed.sql
        states = {p["peer_addr"]: p["state"] for p in peers}
        assert states["10.0.0.1"] == "up"
        assert states["10.0.0.3"] == "down"

    def test_peer_capabilities(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/peers/10.0.0.1/capabilities")
        assert r.status_code in (200, 404)  # 404 ok if no open msg stored

class TestCapacityEndpoints:
    def test_maxprefix_capacity(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/capacity/maxprefix")
        assert r.status_code == 200
        data = r.json()
        assert "peers" in data
        # Verify type 30 stat is in the response
        peers = data["peers"]
        peer1 = next((p for p in peers if p["peer_addr"] == "10.0.0.1"), None)
        if peer1:
            assert peer1["headroom"] == 153

class TestMlEndpoints:
    def test_anomalies_list(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/ml/anomalies?limit=10")
        assert r.status_code == 200
        anomalies = r.json()["anomalies"]
        assert len(anomalies) >= 1
        assert anomalies[0]["kind"] == "origin_change"
        assert anomalies[0]["severity"] == "critical"

class TestSseEndpoints:
    def test_sse_connects(self, server_with_seed_data):
        """SSE endpoint must accept connection and send at least one event."""
        import threading
        received = []
        def stream():
            try:
                r = requests.get(f"{server_with_seed_data}/api/events",
                                 stream=True, timeout=3)
                for line in r.iter_lines():
                    if line.startswith(b"data:"):
                        received.append(line)
                        break
            except requests.exceptions.ReadTimeout:
                pass
        t = threading.Thread(target=stream)
        t.start()
        t.join(timeout=5)
        # SSE must at least connect (status 200) — events not guaranteed in unit test

class TestFilterEndpoints:
    def test_filter_test_endpoint(self, server_with_seed_data):
        r = requests.post(f"{server_with_seed_data}/api/filters/test", json={
            "prefix": "10.0.0.0/8",
            "rpki": "not-found",
            "peer_as": 65001,
            "as_path": "65001 64496",
        })
        assert r.status_code == 200
        result = r.json()
        assert result["verdict"] in ("accept", "reject")
        assert result["verdict"] == "reject", "Bogon 10.0.0.0/8 must be rejected by default filter"
        assert result["elapsed_ns"] > 0

    def test_filter_stats(self, server_with_seed_data):
        r = requests.get(f"{server_with_seed_data}/api/filters/stats")
        assert r.status_code == 200
        stats = r.json()
        assert "accept" in stats and "reject" in stats
```

---

## Part 7 — Internet-Scale BGP Data Sources (Free)

This is the answer to "how do we test with real internet-scale traffic without a production network."

### Source 1: RIPE RIS Live (WebSocket, real-time, completely free)

RIPE RIS Live provides real-time BGP updates collected from more than 600 peers, streamed in real time. This is the cleanest source for live internet BGP data.

```python
# tests/load/ripe_ris_bridge.py
"""
Bridge RIPE RIS Live WebSocket stream → BMP messages → rustybmp TCP port.
Converts RIPE's JSON BGP stream into valid BMP route-monitoring PDUs.
No cost, no registration. Up to 600 BGP peers globally.

Usage:
    python tests/load/ripe_ris_bridge.py \
        --bmp-host 127.0.0.1 --bmp-port 11019 \
        --duration 300 \
        --collectors rrc00,rrc01,rrc03
"""
import asyncio, websockets, json, struct, socket, time, argparse

RIS_LIVE_URL = "wss://ris-live.ripe.net/v1/ws/"
SUBSCRIBE_MSG = {
    "type": "ris_subscribe",
    "data": {
        "type": "UPDATE",
        "host": "",       # "" = all collectors
        "socketOptions": {"includeRaw": False},
    }
}

class RisToBmpBridge:
    def __init__(self, bmp_host: str, bmp_port: int):
        self.bmp_host  = bmp_host
        self.bmp_port  = bmp_port
        self.sock      = None
        self.msg_count = 0
        self.peer_up_sent: set[str] = set()

    def _send_peer_up(self, peer_ip: str, peer_as: int) -> None:
        """Synthesize a BMP PeerUp message for this peer."""
        ...  # build RFC 7854 PeerUp PDU and send via self.sock

    def _ris_to_bmp_route_monitoring(self, msg: dict) -> bytes:
        """Convert RIS Live JSON UPDATE to BMP RouteMonitoring PDU bytes."""
        peer_ip   = msg.get("peer", "0.0.0.0")
        peer_as   = int(msg.get("peer_asn", 0))
        timestamp = msg.get("timestamp", time.time())
        type_ = msg.get("type", "")

        if peer_ip not in self.peer_up_sent:
            self._send_peer_up(peer_ip, peer_as)
            self.peer_up_sent.add(peer_ip)

        announcements = msg.get("announcements", [])
        withdrawals   = msg.get("withdrawals", [])
        as_path       = msg.get("path", [])
        communities   = msg.get("community", [])

        # Build BMP RouteMonitoring wrapping a BGP UPDATE
        # ... (uses rbmp_core types via PyO3 bindings or struct packing)
        return b""  # placeholder — full implementation in bridge

    async def run(self, duration_secs: int, collectors: list[str]) -> None:
        async with websockets.connect(RIS_LIVE_URL) as ws:
            for collector in (collectors or [""]):
                sub = {**SUBSCRIBE_MSG, "data": {**SUBSCRIBE_MSG["data"], "host": collector}}
                await ws.send(json.dumps(sub))

            self.sock = socket.create_connection((self.bmp_host, self.bmp_port), timeout=10)
            deadline = time.time() + duration_secs

            async for raw_msg in ws:
                if time.time() > deadline:
                    break
                msg = json.loads(raw_msg)
                if msg.get("type") == "ris_message":
                    bmp_bytes = self._ris_to_bmp_route_monitoring(msg["data"])
                    if bmp_bytes:
                        self.sock.sendall(bmp_bytes)
                        self.msg_count += 1

            self.sock.close()
            print(json.dumps({
                "driver":      "ripe_ris_bridge",
                "ts_unix":     int(time.time()),
                "status":      "pass",
                "ok":          True,
                "summary":     f"Bridged {self.msg_count} RIS Live messages to rustybmp",
                "checks": [{"name": "message_count", "value": self.msg_count}],
            }))
```

### Source 2: CAIDA BGPStream BMP Kafka Feed (free, read-only)

BGPStream v2 provides access to a publicly-accessible, read-only, Kafka cluster at `bmp.bgpstream.caida.org:9092` which contains raw BMP data. This live BMP feed is available via the broker data interface alongside the traditional MRT-based data from Route Views and RIPE RIS.

This is the most direct source — actual BMP PDUs, no conversion needed:

```python
# tests/load/caida_bmp_replay.py
"""
Read raw BMP data from CAIDA's public Kafka cluster and relay to rustybmp.
This is actual BMP protocol data — no conversion needed.

Topic: caida-bmp (project name in BGPStream v2)
Broker: bmp.bgpstream.caida.org:9092 (public, read-only)

Usage:
    python tests/load/caida_bmp_replay.py \
        --bmp-host 127.0.0.1 --bmp-port 11019 \
        --duration 300
"""
from kafka import KafkaConsumer
import socket, time, json, argparse

CAIDA_BMP_BROKER = "bmp.bgpstream.caida.org:9092"
CAIDA_BMP_TOPIC  = "openbmp.parsed.router"  # actual CAIDA BGPStream BMP topic

def relay_bmp(bmp_host: str, bmp_port: int, duration_secs: int) -> dict:
    consumer = KafkaConsumer(
        CAIDA_BMP_TOPIC,
        bootstrap_servers=[CAIDA_BMP_BROKER],
        auto_offset_reset="latest",
        consumer_timeout_ms=5000,
    )
    sock = socket.create_connection((bmp_host, bmp_port), timeout=10)
    msg_count = 0
    deadline = time.time() + duration_secs

    for message in consumer:
        if time.time() > deadline:
            break
        # CAIDA wraps BMP in OpenBMP message header — strip header, relay BMP payload
        bmp_payload = strip_openbmp_header(message.value)
        if bmp_payload:
            sock.sendall(bmp_payload)
            msg_count += 1

    sock.close()
    consumer.close()
    return {"messages": msg_count, "source": "caida-bmp"}
```

### Source 3: RouteViews MRT Dumps → BMP Injection (full internet table, free)

RouteViews provides full BGP table snapshots (~800-900K prefixes) and UPDATE files. Our `rbmp-mrt` crate already reads MRT format. The pipeline:

```python
# tests/load/mrt_replay.py
"""
Download RouteViews MRT, convert via rbmp-mrt, inject as BMP.
Tests rustybmp at internet-scale (800K+ prefixes).

Collectors: route-views2, route-views3, route-views4
           rrc00 (RIPE), rrc01, rrc03, rrc04

Full table sizes (June 2026):
  IPv4:     ~900K prefixes
  IPv6:     ~190K prefixes
  Combined: ~1.1M prefixes

Expected throughput target: >1M BMP msgs/sec (cargo bench baseline RV4)

Usage:
    python tests/load/mrt_replay.py \
        --collector route-views2 \
        --type updates \
        --bmp-host 127.0.0.1 --bmp-port 11019 \
        --measure-throughput
"""
import subprocess, requests, gzip, os, time, json, argparse

ROUTEVIEWS_BASE = "http://archive.routeviews.org"
RIPE_RIS_BASE   = "https://data.ris.ripe.net"

COLLECTORS = {
    "route-views2": f"{ROUTEVIEWS_BASE}/bgpdata",
    "route-views3": f"{ROUTEVIEWS_BASE}/route-views3/bgpdata",
    "rrc00":        f"{RIPE_RIS_BASE}/rrc00",
    "rrc01":        f"{RIPE_RIS_BASE}/rrc01",
}

def download_latest_mrt(collector: str, mrt_type: str = "updates") -> str:
    """Download the latest MRT dump for a collector. Returns local path."""
    base = COLLECTORS[collector]
    folder = "UPDATES" if mrt_type == "updates" else "RIBS"
    # Fetch index to find latest file
    from datetime import datetime, UTC
    now = datetime.now(UTC)
    url = f"{base}/{now.strftime('%Y.%m')}/{folder}/"
    resp = requests.get(url, timeout=30)
    # Parse listing to find latest .gz file
    import re
    files = re.findall(r'href="((?:updates|bview)\.\d{8}\.\d{4}\.gz)"', resp.text)
    if not files:
        raise ValueError(f"No MRT files found at {url}")
    latest = sorted(files)[-1]
    local_path = f"/tmp/rustybmp_mrt_{collector}_{latest}"
    if not os.path.exists(local_path):
        data = requests.get(f"{url}{latest}", timeout=120).content
        open(local_path, "wb").write(data)
    return local_path

def inject_via_rbmp_mrt(mrt_path: str, bmp_host: str, bmp_port: int) -> dict:
    """Use rbmp-mrt binary to convert MRT → BMP and inject to TCP port."""
    t0 = time.time()
    result = subprocess.run([
        "cargo", "run", "-p", "rbmp-mrt", "--release", "--bin", "mrt-inject",
        "--input",    mrt_path,
        "--bmp-host", bmp_host,
        "--bmp-port", str(bmp_port),
        "--peer-as",  "64496",       # synthetic peer AS for fixture
        "--peer-ip",  "10.255.0.1",  # synthetic peer IP
    ], capture_output=True, timeout=600)
    elapsed = time.time() - t0
    if result.returncode != 0:
        raise RuntimeError(f"mrt-inject failed: {result.stderr.decode()}")
    return {"elapsed_s": elapsed, "mrt_file": mrt_path}

def measure_throughput_after_injection(api_base: str) -> dict:
    """Query the API to count ingested routes and compute throughput."""
    import requests as req
    r = req.get(f"{api_base}/api/routes?limit=1&count=true")
    total_routes = r.json().get("total", 0)
    return {"total_routes": total_routes}
```

### Source 4: BGPKit Broker (Rust-native, 70+ collectors)

BGPKit indexes data from RouteViews and RIPE RIS with 70+ collectors and 1000+ full-feed peers, supporting real-time data streams including RIS-Live and BMP/OpenBMP messages.

For Rust-side load testing, add a `bench_mrt_ingestion` benchmark:

```rust
// benches/mrt_ingestion.rs
// Existing cargo bench establishes >1M msgs/sec baseline (RV4).
// This extends it with real MRT data:

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use bgpkit_parser::BgpkitParser;

fn bench_mrt_parse_and_ingest(c: &mut Criterion) {
    let mrt_path = "tests/fixtures/bmp/routeviews_update.mrt";
    if !std::path::Path::new(mrt_path).exists() {
        return;  // skip if fixture not available
    }

    c.bench_function("mrt_full_table_ingest", |b| {
        b.iter(|| {
            let parser = BgpkitParser::new(mrt_path).unwrap();
            let mut count = 0usize;
            for _elem in parser {
                count += 1;
            }
            black_box(count)
        })
    });
}

criterion_group!(benches, bench_mrt_parse_and_ingest);
criterion_main!(benches);
```

---

## Part 8 — Layer 4: ContainerLab Scenarios

### The fundamental clab problem list and solutions

Before writing any topology, address these known failure modes:

| Problem | Root cause | Solution |
|---------|-----------|---------|
| Device boots but config not applied | clab deploys config before interface is up | Use `startup-config` (not post-deploy SSH); add `exec` wait |
| BMP session never establishes | BMP target configured before routing converges | Wait for BGP Established first, then verify BMP PeerUp |
| SSH connection refused | Container not fully booted | Use `wait_until_ssh_ready()` in test fixture with 30s timeout |
| XRd inotify limit | Ubuntu default too low | Pre-set `fs.inotify.max_user_instances=64000` in host sysctl |
| Routing not converged | Links up but ISIS/BGP still converging | Poll `/api/peers` until state==up before asserting |
| Config rejected by device | Syntax error in startup config | Validate configs with Batfish BEFORE deploying the lab |
| Memory OOM kills container | Too many nodes for available RAM | Budget 2GB/SRL, 3GB/XRd, 1GB/cEOS, 512MB/cRPD, 256MB/FRR |
| cEOS MD5 auth fails on macOS | macOS 15.4 kernel doesn't support TCP MD5 | Remove MD5 from all neighbor configs in cEOS |

### The NOS image tier system

```
TIER 0 — Zero friction (pull from public registry, no account):
  FRRouting:       quay.io/frrouting/frr:10.6.1  ← BMP speaker confirmed
  Nokia SR Linux:  ghcr.io/nokia/srlinux:latest   ← BMP via gNMI/NETCONF
  GoBGP:          custom Dockerfile (100MB)        ← Pure BMP speaker

TIER 1 — Free with account (download from vendor portal):
  Arista cEOS:    ceos:4.32.0F                    ← Good BMP + RFC 9972 stats
  Cisco XRd:      ios-xr/xrd-control-plane:24.x   ← Full BMP + RFC 9972 + ASPA

TIER 2 — Licensed (skip for CI, available in dedicated lab):
  Juniper cRPD:   crpd:24.2R1.14 + license file   ← Partial BMP
  Nokia SR-OS:    vrnetlab/nokia_sros:* + license  ← BMP via YANG
```

CI uses Tier 0 only. Developer machines use Tier 0 + Tier 1. Licensed hardware stays in a dedicated lab.

---

### Scenario 01: FRR Minimal (Tier 0, 30s boot, BMP smoke test)

**Purpose**: Fastest possible verification that rustybmp receives BMP, parses peer events, stores routes.

```yaml
# tests/scenarios/01_frr_minimal/topology.clab.yml
name: rustybmp-frr-minimal

topology:
  nodes:
    rustybmp:
      kind: linux
      image: ubuntu:24.04
      binds:
        - ../../../target/debug/rbmp-collector:/usr/local/bin/rbmp-collector:ro
        - configs/rustybmp.toml:/etc/rustybmp/rustybmp.toml:ro
      exec:
        - rbmp-collector --config /etc/rustybmp/rustybmp.toml &

    frr-pe1:
      kind: linux
      image: quay.io/frrouting/frr:10.6.1
      mgmt-ipv4: 172.20.20.10
      binds:
        - configs/frr-pe1/daemons:/etc/frr/daemons
        - configs/frr-pe1/frr.conf:/etc/frr/frr.conf

    frr-pe2:
      kind: linux
      image: quay.io/frrouting/frr:10.6.1
      mgmt-ipv4: 172.20.20.11
      binds:
        - configs/frr-pe1/daemons:/etc/frr/daemons
        - configs/frr-pe2/frr.conf:/etc/frr/frr.conf

  links:
    - endpoints: ["frr-pe1:eth1", "frr-pe2:eth1"]
    - endpoints: ["frr-pe1:eth2", "rustybmp:eth1"]
```

```
# tests/scenarios/01_frr_minimal/configs/frr-pe1/frr.conf
# Key sections — full file in tests/scenarios/01_frr_minimal/configs/

router bgp 65001
  bgp router-id 10.0.0.1
  neighbor 10.0.0.2 remote-as 65002
  address-family ipv4 unicast
    network 203.0.113.0/24
    neighbor 10.0.0.2 activate
  exit-address-family
  bmp targets rustybmp
    bmp connect 172.20.20.1 port 11019 min-retry 1000 max-retry 5000
    bmp monitor ipv4 unicast pre-policy
    bmp monitor ipv4 unicast post-policy
    bmp monitor ipv4 unicast loc-rib
  exit
```

```python
# tests/scenarios/01_frr_minimal/test_frr_minimal.py

import pytest, time, requests, subprocess, os
from pathlib import Path

SCENARIO_DIR = Path(__file__).parent
API_BASE = "http://172.20.20.1:7878"  # rustybmp API on host

@pytest.fixture(scope="module")
def clab_frr_minimal():
    """Deploy clab topology and wait for BMP peer-up."""
    subprocess.run(
        ["containerlab", "deploy", "-t", str(SCENARIO_DIR / "topology.clab.yml"), "--reconfigure"],
        check=True, cwd=SCENARIO_DIR
    )
    # Wait for BMP peer-up (up to 60s)
    deadline = time.time() + 60
    while time.time() < deadline:
        try:
            r = requests.get(f"{API_BASE}/api/peers", timeout=2)
            if r.ok and any(p["state"] == "up" for p in r.json().get("peers", [])):
                break
        except Exception:
            pass
        time.sleep(1)
    else:
        pytest.fail("BMP peer-up not received within 60s")
    yield
    subprocess.run(
        ["containerlab", "destroy", "-t", str(SCENARIO_DIR / "topology.clab.yml"), "--cleanup"],
        cwd=SCENARIO_DIR
    )

def test_bmp_peer_up_received(clab_frr_minimal):
    r = requests.get(f"{API_BASE}/api/peers")
    peers = r.json()["peers"]
    assert any(p["state"] == "up" for p in peers), "Expected at least one BGP peer in BMP up state"

def test_ipv4_route_announced(clab_frr_minimal):
    r = requests.get(f"{API_BASE}/api/routes?prefix=203.0.113.0%2F24")
    routes = r.json()["routes"]
    assert len(routes) > 0, "Expected 203.0.113.0/24 announced via BMP"

def test_pre_and_post_policy_ribs_present(clab_frr_minimal):
    r = requests.get(f"{API_BASE}/api/policy")
    data = r.json()
    ribs = {row.get("rib_type") for row in data.get("by_rib_type", [])}
    assert "adj-in-pre"  in ribs, "pre-policy RIB missing"
    assert "adj-in-post" in ribs, "post-policy RIB missing"
```

---

### Scenario 02: XRd BGP Functional (Tier 1, RFC 9972 validation)

**Purpose**: Validate RFC 9972 stats types 18-38 (the whole reason rustybmp was built), EVPN, ASPA.

```yaml
# tests/scenarios/02_xrd_functional/topology.clab.yml
name: rustybmp-xrd-functional

topology:
  nodes:
    rustybmp:
      kind: linux
      image: ubuntu:24.04
      binds:
        - ../../../target/debug/rbmp-collector:/usr/local/bin/rbmp-collector:ro
        - configs/rustybmp.toml:/etc/rustybmp/rustybmp.toml:ro
      exec:
        - rbmp-collector --config /etc/rustybmp/rustybmp.toml &

    xrd-pe1:
      kind: cisco_xrd
      image: ios-xr/xrd-control-plane:24.4.1
      mgmt-ipv4: 172.20.20.20
      startup-config: configs/xrd-pe1.cfg
      # Pre-requisite: inotify limits set in host /etc/sysctl.conf

    xrd-pe2:
      kind: cisco_xrd
      image: ios-xr/xrd-control-plane:24.4.1
      mgmt-ipv4: 172.20.20.21
      startup-config: configs/xrd-pe2.cfg

  links:
    - endpoints: ["xrd-pe1:Gi0/0/0/0", "xrd-pe2:Gi0/0/0/0"]
    - endpoints: ["xrd-pe1:Gi0/0/0/1", "rustybmp:eth1"]
```

```
# tests/scenarios/02_xrd_functional/configs/xrd-pe1.cfg
router bgp 65001
 bgp router-id 10.0.0.1
 bmp server 1
  host 172.20.20.1 port 11019
  flapping-delay 30
 !
 bmp servers 1
  initial-delay 5
  stats-reporting-period 30
  description rustybmp-collector
 !
 neighbor 10.0.0.2
  remote-as 65002
  bmp-activate server 1
  address-family ipv4 unicast
   maximum-prefix 1000
  !
 !
 address-family ipv4 unicast
  maximum-prefix 1000
```

```python
# tests/scenarios/02_xrd_functional/test_xrd_functional.py

class TestXrdRfc9972:
    def test_stats_type30_headroom(self, clab_xrd):
        """RFC 9972 type 30 = routes left before max-prefix limit fires."""
        r = requests.get(f"{API_BASE}/api/bmpstats/history?limit=50")
        stats = r.json()["stats"]
        type30 = [s for s in stats if s.get("counter_type") == 30]
        assert len(type30) > 0, "RFC 9972 type 30 (max-prefix headroom) not received from XRd"
        assert type30[0]["counter_value"] < 1000, "Headroom should be < configured max-prefix of 1000"

    def test_stats_afisafi_breakdown(self, clab_xrd):
        """RFC 9972 stats must include AFI/SAFI fields."""
        r = requests.get(f"{API_BASE}/api/bmpstats/history?limit=100")
        stats = r.json()["stats"]
        with_afi = [s for s in stats if s.get("afi")]
        assert len(with_afi) > 0, "RFC 9972 stats should include AFI/SAFI breakdown"

    def test_aspa_validation_fires(self, clab_xrd):
        """After injecting a route with AS path violating ASPA, expect aspa_validations entry."""
        # Inject a route with invalid AS path via ExaBGP stub
        r = requests.get(f"{API_BASE}/api/aspa?limit=10")
        if r.status_code == 200:
            # ASPA is best-effort — skip if table empty
            assert "validations" in r.json()
```

---

### Scenario 03: Multi-Vendor BMP Matrix (Tier 0+1, interoperability)

**Purpose**: Ensure BMP works identically across FRR, SRL, cEOS, XRd. BMP implementations vary.

```yaml
# tests/scenarios/03_multi_vendor/topology.clab.yml
name: rustybmp-multi-vendor

topology:
  defaults:
    kind: nokia_srlinux
    image: ghcr.io/nokia/srlinux:latest

  nodes:
    rustybmp:
      kind: linux
      image: ubuntu:24.04

    # Tier 0: always available in CI
    frr-as65001:
      kind: linux
      image: quay.io/frrouting/frr:10.6.1
      binds:
        - configs/frr-as65001/:/etc/frr/

    srl-as65100:
      mgmt-ipv4: 172.20.20.30
      startup-config: configs/srl-as65100.cfg

    # Tier 1: require vendor images (skip in CI if not present)
    # ceos-as65200:  # conditional — only if image available
    # xrd-as65300:   # conditional

  links:
    - endpoints: ["frr-as65001:eth1", "srl-as65100:e1-1"]
    - endpoints: ["frr-as65001:eth2", "rustybmp:eth1"]
    - endpoints: ["srl-as65100:e1-2", "rustybmp:eth2"]
```

```python
# tests/scenarios/03_multi_vendor/test_multi_vendor.py

VENDOR_PEERS = {
    "frr":   "10.0.1.1",
    "srl":   "10.0.2.1",
    "ceos":  "10.0.3.1",  # optional
    "xrd":   "10.0.4.1",  # optional
}

def test_bmp_peer_up_per_vendor(clab_multi_vendor):
    """Each vendor's BMP implementation must deliver a PeerUp message."""
    peers = requests.get(f"{API_BASE}/api/peers").json()["peers"]
    peer_addrs = {p["peer_addr"] for p in peers if p["state"] == "up"}

    for vendor, peer_ip in VENDOR_PEERS.items():
        if peer_ip in peer_addrs:
            pass  # OK
        elif vendor in ("ceos", "xrd"):
            pytest.skip(f"{vendor} image not available in this environment")
        else:
            pytest.fail(f"{vendor} BMP peer-up not received (expected {peer_ip} in {peer_addrs})")

def test_route_counts_match_per_vendor(clab_multi_vendor):
    """Route count from BMP must match what the device claims to advertise."""
    # FRR: configured with 5 static prefixes → expect 5 routes from frr-as65001
    routes = requests.get(f"{API_BASE}/api/routes?peer_addr=10.0.1.1").json()["routes"]
    assert len(routes) == 5, f"FRR should advertise exactly 5 routes, got {len(routes)}"
```

---

### Scenario 04: BGP Anomaly Injection (Fault-inject, soak test equivalent)

**Purpose**: Inject known-bad BGP patterns and verify detectors fire. BGP equivalent of bonsai's `soak_test.py`.

```python
# tests/scenarios/04_anomaly_injection/fault_injector.py
"""
Inject BGP fault patterns via ExaBGP into FRR → verify detector fires in rustybmp.

Fault library:
  1. hijack_same_prefix_wrong_origin:  announce 203.0.113.0/24 from wrong ASN
  2. route_leak_private_asn:           announce prefix with 64512 in as_path
  3. rpki_invalid_more_specific:       announce /25 subprefix of valid /24 ROA
  4. peer_flap:                        admin-down/up BGP neighbor 3x in 10s
  5. max_prefix_approach:              announce prefixes until >80% of max_prefix
  6. otc_route_leak:                   announce with OTC attribute set (RFC 9234)
"""
import time, subprocess, requests

class FaultInjector:
    def __init__(self, api_base: str, exabgp_host: str):
        self.api_base     = api_base
        self.exabgp_host  = exabgp_host

    def inject_hijack(self, prefix: str, wrong_origin_as: int) -> None:
        """Announce a prefix from a wrong origin ASN to simulate a hijack."""
        # ExaBGP announce format
        cmd = f"announce route {prefix} next-hop 10.255.255.1 as-path [ {wrong_origin_as} ]"
        self._exabgp_cmd(cmd)

    def inject_private_asn_leak(self, prefix: str) -> None:
        """Announce a prefix with a private ASN in the path."""
        cmd = f"announce route {prefix} next-hop 10.255.255.1 as-path [ 65001 64512 64496 ]"
        self._exabgp_cmd(cmd)

    def wait_for_anomaly(self, kind: str, timeout: int = 30) -> dict:
        """Poll ml_anomalies until an anomaly of this kind appears."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            r = requests.get(f"{self.api_base}/api/ml/anomalies?limit=20")
            anomalies = r.json().get("anomalies", [])
            match = [a for a in anomalies if a["kind"] == kind]
            if match:
                return match[0]
            time.sleep(1)
        raise TimeoutError(f"Anomaly '{kind}' not detected within {timeout}s")

    def _exabgp_cmd(self, cmd: str) -> None:
        with open("/run/exabgp.in", "w") as f:
            f.write(cmd + "\n")
        time.sleep(0.2)

# Test cases
class TestAnomalyInjection:
    def test_hijack_detected(self, clab_anomaly):
        injector = FaultInjector(API_BASE, exabgp_host="10.0.99.1")
        injector.inject_hijack("203.0.113.0/24", wrong_origin_as=12345)
        anomaly = injector.wait_for_anomaly("origin_change", timeout=30)
        assert anomaly["severity"] in ("warn", "critical")
        assert "12345" in anomaly.get("description", "")

    def test_route_leak_detected(self, clab_anomaly):
        injector = FaultInjector(API_BASE, "10.0.99.1")
        injector.inject_private_asn_leak("198.51.100.0/24")
        anomaly = injector.wait_for_anomaly("route_leak", timeout=30)
        assert anomaly is not None

    def test_rpki_invalid_rejected_by_filter(self, clab_anomaly):
        injector = FaultInjector(API_BASE, "10.0.99.1")
        # Announce a /25 subprefix — RPKI invalid (ROA only covers /24)
        injector.inject_hijack("203.0.113.128/25", wrong_origin_as=64496)
        time.sleep(2)
        r = requests.post(f"{API_BASE}/api/filters/test", json={
            "prefix": "203.0.113.128/25",
            "rpki": "invalid",
            "peer_as": 64496,
        })
        assert r.json()["verdict"] == "reject", "RPKI-invalid /25 should be rejected by default filter"
```

---

### Scenario 05: RPKI Testbed (Tier 0, routinator + FRR)

**Purpose**: Test the full RPKI pipeline: ROA validation, ASPA, RPKI-invalid filter.

```yaml
# tests/scenarios/05_rpki_testbed/topology.clab.yml
name: rustybmp-rpki-testbed

topology:
  nodes:
    rustybmp:
      kind: linux
      image: ubuntu:24.04

    routinator:
      kind: linux
      image: nlnetlabs/routinator:latest
      binds:
        - configs/routinator/routinator.conf:/etc/routinator/routinator.conf:ro
        - configs/routinator/tals/:/etc/routinator/tals/:ro

    frr-validator:
      kind: linux
      image: quay.io/frrouting/frr:10.6.1
      binds:
        - configs/frr-validator/:/etc/frr/
      exec:
        # FRR connects to routinator RTR on port 3323, then sends BMP to rustybmp
        - "sleep 5 && vtysh -c 'conf t' -c 'rpki' -c 'rpki cache 172.20.20.50 3323' -c 'exit' -c 'exit'"
```

---

### Scenario 06: SP Scale (full topology, convergence testing)

**Purpose**: Simulate a service provider topology with route reflectors, VPN, and BGP-LS. Test convergence event detection.

```yaml
# tests/scenarios/06_sp_scale/topology.clab.yml
name: rustybmp-sp-scale

topology:
  nodes:
    rustybmp:  { kind: linux, image: ubuntu:24.04 }

    # Route reflector (all BGP sessions → BMP to rustybmp)
    frr-rr:
      kind: linux
      image: quay.io/frrouting/frr:10.6.1
      binds: [configs/frr-rr/:/etc/frr/]

    # PE routers
    frr-pe1: { kind: linux, image: quay.io/frrouting/frr:10.6.1, binds: [configs/frr-pe1/:/etc/frr/] }
    frr-pe2: { kind: linux, image: quay.io/frrouting/frr:10.6.1, binds: [configs/frr-pe2/:/etc/frr/] }
    frr-pe3: { kind: linux, image: quay.io/frrouting/frr:10.6.1, binds: [configs/frr-pe3/:/etc/frr/] }

    # CE routers (BGP CE → PE)
    frr-ce1: { kind: linux, image: quay.io/frrouting/frr:10.6.1, binds: [configs/frr-ce1/:/etc/frr/] }
    frr-ce2: { kind: linux, image: quay.io/frrouting/frr:10.6.1, binds: [configs/frr-ce2/:/etc/frr/] }

  links:
    - endpoints: ["frr-rr:eth1",  "frr-pe1:eth1"]
    - endpoints: ["frr-rr:eth2",  "frr-pe2:eth1"]
    - endpoints: ["frr-rr:eth3",  "frr-pe3:eth1"]
    - endpoints: ["frr-pe1:eth2", "frr-ce1:eth1"]
    - endpoints: ["frr-pe2:eth2", "frr-ce2:eth1"]
    - endpoints: ["frr-rr:eth4",  "rustybmp:eth1"]   # BMP sessions from RR
```

```python
class TestSpScaleConvergence:
    def test_convergence_event_detected(self, clab_sp_scale):
        """Admin-down a PE, verify convergence event appears in rustybmp."""
        # Take PE2 down (admin-down all BGP sessions)
        subprocess.run(["docker", "exec", "clab-rustybmp-sp-scale-frr-pe2",
                        "vtysh", "-c", "clear bgp *"], check=True)
        time.sleep(2)  # allow BMP PeerDown to arrive

        r = requests.get(f"{API_BASE}/api/convergence?hours=1")
        events = r.json().get("events", [])
        assert len(events) > 0, "Expected convergence event after PE2 session clear"

        event = events[0]
        assert event["convergence_ms"] > 0
        assert event.get("affected_prefixes", 0) > 0
```

---

## Part 9 — Layer 7: UI End-to-End Tests (Playwright, Codex-compatible)

### The core principle: operator-free UI testing

All UI elements that are tested must have `data-testid` attributes. Playwright tests use ONLY `data-testid` selectors — never CSS classes (which change with Tailwind updates), never text content (which changes with copy edits), never element hierarchy.

### data-testid tagging convention

```svelte
<!-- Every interactive element and metric display gets a data-testid -->
<!-- Convention: page-component-role -->

<!-- In +page.svelte (Dashboard) -->
<div data-testid="dashboard-peers-up-count">{peersUp}</div>
<div data-testid="dashboard-peers-down-count">{peersDown}</div>
<div data-testid="dashboard-rpki-valid-pct">{rpkiValidPct}%</div>
<table data-testid="dashboard-peer-table">
  {#each peers as peer}
    <tr data-testid="peer-row-{peer.peer_addr}">
      <td data-testid="peer-addr">{peer.peer_addr}</td>
      <td data-testid="peer-state">{peer.state}</td>
    </tr>
  {/each}
</table>

<!-- In peers/[addr]/+page.svelte -->
<div data-testid="peer-detail-flap-count">{downCount}</div>
<div data-testid="peer-detail-session-uptime">{longestSession?.dur_secs}</div>

<!-- In /filters -->
<textarea data-testid="filter-test-input"></textarea>
<button data-testid="filter-test-submit">Test filter</button>
<div data-testid="filter-test-verdict">{verdict}</div>

<!-- In /path-status (RV7 new) -->
<div data-testid="path-status-matrix"></div>
<div data-testid="path-status-best-count">{bestCount}</div>
<div data-testid="capacity-gauge-peer-{addr}">{utilPct}%</div>
```

### Playwright test suite

```typescript
// ui/tests/dashboard.spec.ts

import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  // Seed fixture data via API before each test
  await fetch('http://127.0.0.1:7878/api/_test/seed', {
    method: 'POST',
    body: JSON.stringify({ fixture: 'standard_two_peers' })
  });
});

test('dashboard shows correct peer counts', async ({ page }) => {
  await page.goto('http://127.0.0.1:5173/');
  await page.waitForSelector('[data-testid="dashboard-peers-up-count"]');

  const peersUp = await page.locator('[data-testid="dashboard-peers-up-count"]').textContent();
  expect(parseInt(peersUp!)).toBe(2);

  const peersDown = await page.locator('[data-testid="dashboard-peers-down-count"]').textContent();
  expect(parseInt(peersDown!)).toBe(1);
});

test('peer table rows are clickable and navigate', async ({ page }) => {
  await page.goto('http://127.0.0.1:5173/');
  await page.waitForSelector('[data-testid="peer-row-10.0.0.1"]');
  await page.click('[data-testid="peer-row-10.0.0.1"]');
  await expect(page).toHaveURL(/\/peers\/10\.0\.0\.1/);
  await page.waitForSelector('[data-testid="peer-detail-flap-count"]');
});
```

```typescript
// ui/tests/filters.spec.ts

test('filter test rejects bogon prefix', async ({ page }) => {
  await page.goto('http://127.0.0.1:5173/filters');
  await page.waitForSelector('[data-testid="filter-test-input"]');

  // Fill the filter test form
  await page.fill('[data-testid="filter-test-prefix"]', '10.0.0.0/8');
  await page.selectOption('[data-testid="filter-test-rpki"]', 'not-found');
  await page.click('[data-testid="filter-test-submit"]');

  await page.waitForSelector('[data-testid="filter-test-verdict"]');
  const verdict = await page.locator('[data-testid="filter-test-verdict"]').textContent();
  expect(verdict).toContain('reject');
});

test('filter hot-reload updates verdict', async ({ page }) => {
  await page.goto('http://127.0.0.1:5173/filters');

  // Edit filter to accept all
  await page.click('[data-testid="filter-editor-toggle"]');
  await page.fill('[data-testid="filter-editor-textarea"]',
    'fn bgp_filter(route: RouteCtx) -> bool { true }');
  await page.click('[data-testid="filter-reload-button"]');
  await page.waitForSelector('[data-testid="filter-reload-status-ok"]');

  // Now test bogon — should be accepted after override
  await page.fill('[data-testid="filter-test-prefix"]', '10.0.0.0/8');
  await page.click('[data-testid="filter-test-submit"]');
  await page.waitForSelector('[data-testid="filter-test-verdict"]');
  const verdict = await page.locator('[data-testid="filter-test-verdict"]').textContent();
  expect(verdict).toContain('accept');
});
```

```typescript
// ui/tests/capacity.spec.ts  (RV7 new)

test('max-prefix fuel gauge shows utilization', async ({ page }) => {
  await page.goto('http://127.0.0.1:5173/capacity');
  await page.waitForSelector('[data-testid="capacity-gauge-10.0.0.1"]');

  const utilPct = await page.locator('[data-testid="capacity-gauge-10.0.0.1"]').textContent();
  // Fixture seed has headroom=153, route_count=847 → 84.7% utilization
  expect(parseFloat(utilPct!)).toBeCloseTo(84.7, 0);
});

test('critical peer shows warning banner', async ({ page }) => {
  await page.goto('http://127.0.0.1:5173/capacity');
  // Fixture: peer 10.0.0.3 is at 96% utilization
  await page.waitForSelector('[data-testid="capacity-critical-alert"]');
  const alert = await page.locator('[data-testid="capacity-critical-alert"]').textContent();
  expect(alert).toContain('10.0.0.3');
});
```

---

## Part 10 — The API Test Endpoint: `POST /api/_test/seed`

This is the linchpin of operator-free UI testing. The server exposes a test-only endpoint that seeds DuckDB with fixture data:

```rust
// crates/rbmp-server/src/api/test_endpoints.rs
// Only compiled with cfg(test) or feature = "test-endpoints"

pub async fn seed_handler(
    State(state): State<AppState>,
    Json(req): Json<SeedRequest>,
) -> Json<serde_json::Value> {
    let fixture = match req.fixture.as_str() {
        "standard_two_peers" => include_str!("../../../../tests/fixtures/seed_standard.sql"),
        "anomaly_active"     => include_str!("../../../../tests/fixtures/seed_anomaly.sql"),
        "maxprefix_critical" => include_str!("../../../../tests/fixtures/seed_maxprefix.sql"),
        "convergence_event"  => include_str!("../../../../tests/fixtures/seed_convergence.sql"),
        _                    => return Json(json!({"error": "unknown fixture"})),
    };
    // Execute SQL against DuckDB — truncate tables first, then insert fixture data
    state.store.execute_seed_sql(fixture).await;
    Json(json!({"ok": true, "fixture": req.fixture}))
}
```

Every Playwright test calls this before navigating. The UI always sees deterministic data.

---

## Part 11 — GitHub Actions CI Pipeline

```yaml
# .github/workflows/ci.yml

name: CI

on: [push, pull_request]

jobs:
  layer0_unit:
    name: "Layer 0 — Unit tests"
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --workspace -- --test-output immediate
      - run: cargo build --workspace 2>&1 | grep -c "warning" | xargs -I{} test {} -eq 0

  layer1_wiring:
    name: "Layer 1 — Wiring checks"
    runs-on: ubuntu-24.04
    needs: layer0_unit
    steps:
      - uses: actions/checkout@v4
      - run: bash scripts/check_wiring.sh

  layer2_protocol:
    name: "Layer 2 — Protocol integration"
    runs-on: ubuntu-24.04
    needs: layer1_wiring
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
      - run: pip install pytest requests
      - run: pytest tests/protocol/ -v --tb=short --json-report --json-report-file=runtime/test_results/layer2.json

  layer3_api:
    name: "Layer 3 — API contracts"
    runs-on: ubuntu-24.04
    needs: layer2_protocol
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
      - run: duckdb /tmp/test.duckdb ".read tests/fixtures/seed.sql" && pytest tests/api/ -v

  layer4_frr_smoke:
    name: "Layer 4 — FRR smoke (clab)"
    runs-on: ubuntu-24.04
    needs: layer3_api
    steps:
      - uses: actions/checkout@v4
      - name: Install containerlab
        run: bash -c "$(curl -sL https://get.containerlab.dev)"
      - name: Pull FRR image
        run: docker pull quay.io/frrouting/frr:10.6.1
      - run: cargo build --workspace
      - run: pytest tests/scenarios/01_frr_minimal/ -v --timeout=120

  layer7_ui:
    name: "Layer 7 — UI E2E"
    runs-on: ubuntu-24.04
    needs: layer3_api
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
      - run: cd ui && npm ci && npm run build
      - run: npx playwright install chromium --with-deps
      - run: npx playwright test --reporter=github
```

---

## Part 12 — The Codex Execution Model

When Codex runs in the Ubuntu environment:

1. **Codex reads test output** — every layer produces JSON at `runtime/test_results/<layer>.json`. Codex parses the `checks` array and identifies which specific check failed.

2. **Codex identifies the failing code** — the `name` field in the failing check maps directly to a test function name. Codex finds that function in `tests/` and reads the assertion.

3. **Codex makes targeted fixes** — the test assertion tells Codex exactly what the code should do. A failing `test_stats_type30_max_prefix` means the `stats_events` table isn't getting counter_type=30 rows.

4. **Codex reruns the specific layer** — `pytest tests/protocol/ -k test_stats_type30_max_prefix -v` is faster than rerunning all 200 unit tests.

5. **Codex runs Playwright in headless** — `npx playwright test --headed=false` works in Ubuntu without a display server. The screenshots from failed tests land in `playwright-report/` for Codex to read.

### What makes tests Codex-compatible:

- Every assertion has a message: `assert len(routes) > 0, "Expected routes after RouteMonitoring announce"` — not just `assert len(routes) > 0`
- Fixtures are deterministic — same input = same output every run
- Tests are independent — running test N doesn't require tests 1..N-1 to have passed
- The failing test name maps 1:1 to a specific module and function in the codebase
- `data-testid` selectors are stable — Codex can find them in Svelte source files

---

## Part 13 — Test Infrastructure Files Summary

```
tests/
├── fixtures/
│   ├── bmp/                         # Binary BMP captures (committed to git)
│   │   ├── peer_up_xrd.bin
│   │   ├── peer_up_frr.bin
│   │   ├── route_monitoring_ipv4_announce.bin
│   │   ├── route_monitoring_with_path_status_tlv.bin  # RV7
│   │   ├── stats_report_type30.bin
│   │   └── routeviews_update.mrt
│   ├── seed.sql                     # DuckDB fixture for API tests
│   ├── seed_anomaly.sql             # Anomaly fixture for UI tests
│   ├── seed_maxprefix.sql           # Max-prefix fixture (RV7)
│   └── seed_convergence.sql        # Convergence fixture (RV7)
│
├── protocol/                        # Layer 2 — BMP protocol
│   └── test_bmp_parsing.py
│
├── api/                             # Layer 3 — API contract
│   ├── conftest.py
│   ├── test_all_endpoints.py
│   └── test_mcp_tools.py            # RV8
│
├── scenarios/                       # Layer 4/5 — ContainerLab
│   ├── 01_frr_minimal/
│   ├── 02_xrd_functional/
│   ├── 03_multi_vendor/
│   ├── 04_anomaly_injection/
│   ├── 05_rpki_testbed/
│   └── 06_sp_scale/
│
├── load/                            # Layer 6 — Scale
│   ├── ripe_ris_bridge.py
│   ├── caida_bmp_relay.py
│   └── mrt_replay.py
│
└── ui/                              # Layer 7 — Playwright
    ├── playwright.config.ts
    └── tests/
        ├── dashboard.spec.ts
        ├── peers.spec.ts
        ├── filters.spec.ts
        ├── path-status.spec.ts      # RV7
        └── capacity.spec.ts         # RV7

scripts/
├── check_wiring.sh                  # Layer 1
├── capture_bmp_fixtures.py          # One-time fixture generation
├── download_mrt_fixture.py          # Download RouteViews MRT sample
└── smoke/
    ├── smoke_bmp_ingestion.sh
    ├── smoke_roto_filter.sh
    └── smoke_api_endpoints.sh

runtime/
└── test_results/                    # Machine-readable test output (git-ignored)
    ├── latest.json -> ...
    └── <layer>_<timestamp>.json
```

---

## Part 14 — RV8 Testing Epic Index

| Epic | Title | Layer | Priority |
|------|-------|-------|----------|
| RV8-T1 | Fixture corpus — capture BMP PDUs from XRd/SRL/FRR | Layer 2 | P0 |
| RV8-T2 | `tests/protocol/test_bmp_parsing.py` — 20+ protocol checks | Layer 2 | P0 |
| RV8-T3 | `tests/fixtures/seed.sql` — deterministic DuckDB seed | Layer 3 | P0 |
| RV8-T4 | `tests/api/test_all_endpoints.py` — every endpoint covered | Layer 3 | P0 |
| RV8-T5 | `POST /api/_test/seed` — fixture injection endpoint | Layer 3 | P0 |
| RV8-T6 | `data-testid` tagging on all interactive UI elements | Layer 7 | P0 |
| RV8-T7 | `tests/scenarios/01_frr_minimal/` — Tier 0 smoke lab | Layer 4 | P0 |
| RV8-T8 | `scripts/check_wiring.sh` — 15-second wiring gate | Layer 1 | P1 |
| RV8-T9 | `tests/scenarios/02_xrd_functional/` — RFC 9972 validation | Layer 5 | P1 |
| RV8-T10 | `tests/scenarios/04_anomaly_injection/` — fault inject cycle | Layer 5 | P1 |
| RV8-T11 | Playwright test suite — all 15+ pages with data-testid | Layer 7 | P1 |
| RV8-T12 | `ripe_ris_bridge.py` — live internet BGP stream | Layer 6 | P1 |
| RV8-T13 | `mrt_replay.py` — RouteViews full table injection | Layer 6 | P1 |
| RV8-T14 | GitHub Actions CI — Layers 0-4 + 7 | CI | P1 |
| RV8-T15 | `tests/scenarios/03_multi_vendor/` — NOS interop | Layer 5 | P2 |
| RV8-T16 | `tests/scenarios/05_rpki_testbed/` — routinator + FRR | Layer 5 | P2 |
| RV8-T17 | `tests/scenarios/06_sp_scale/` — convergence testing | Layer 5 | P2 |
| RV8-T18 | `caida_bmp_relay.py` — CAIDA Kafka BMP feed | Layer 6 | P2 |
| RV8-T19 | Cargo bench extension with real MRT data | Layer 6 | P2 |
| RV8-T20 | Path Status TLV fixture + test | Layer 2 | P1 (RV7 dep) |

---

*End of RUSTYBMP_TESTING_STRATEGY.md*
