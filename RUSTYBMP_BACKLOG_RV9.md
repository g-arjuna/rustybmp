# RustyBMP — Sprint RV9 Backlog
## Completeness Sprint · Exhaustive Bundle Plan · Testing-First Architecture

> **Version**: RV9
> **Date**: 2026-06-21
> **Basis**: Full read of rv8_all_changes.patch · RUSTYBMP_BACKLOG_RV8.md ·
>            RUSTYBMP_TESTING_STRATEGY.md · RUSTYBMP_PROJECT_CONTEXT.md
>
> **Principle**: RV9 is the COMPLETENESS sprint. Nothing is deferred without explicit
> reason. Every feature from RV6/RV7/RV8 that was planned but not shipped must land.
> Testing is NOT the last bundle — it is woven into EVERY bundle as the final gate.
> No bundle is considered done until its tests pass.
>
> **Methodology**: Every epic from RV7+RV8 backlogs cross-referenced against the actual
> patch file contents. Items confirmed as "delivered" have specific file/line evidence.
> Items marked "deferred" have no presence in the patch or are stub-only.

---

## RV9 Bundle Overview

| Bundle | Theme | Epics | Testing Gate |
|--------|-------|-------|--------------|
| **A** | Test Infrastructure Foundation | RV9-T0 through T3 | `cargo test` + wiring script |
| **B** | Protocol & API Test Coverage | RV9-T4 through T6 | pytest Layer 2+3 pass |
| **C** | Convergence Detection + BGPsec | RV9-ML4, RV9-NEW2 | Unit tests + API contract |
| **D** | ML Pipeline Completion | RV9-ML1 through ML3, ML5 | pytest ml/ suite |
| **E** | Output Adapters Completion | RV9-OUT4, OUT5, OUT6 | Adapter smoke tests |
| **F** | Enrichers + External APIs | RV9-ENR1/2, EXT2/3/4 | Integration tests |
| **G** | UI Completeness | RV9-UX1 through UX6, T6, T8 | Playwright Layer 7 |
| **H** | Lab Scenarios (ContainerLab) | RV9-T7, T9, T-load | clab pytest suite |
| **I** | New Capabilities | RV9-NEW1/3/4/5/6/7 | Unit + smoke tests |
| **J** | CI + Release Infrastructure | RV9-T9, CODEX_TESTING.md | GitHub Actions green |

**Sequencing rule**: A → B → (C, D, E, F in parallel) → G → H → I → J.
A and B are strict prerequisites. C-F can be worked in parallel teams.
G requires B (data-testid) + T3 (seed endpoint). H requires A+B+FRR image.
J requires all bundles green.

---

## Bundle A — Test Infrastructure Foundation

**Why first**: Testing layers 1-7 have zero runnable infrastructure post-RV8. Every subsequent bundle adds tests INTO this scaffold. Until A lands, no automated regression exists.

### A1 — Unit Test Target: 200 Tests (currently 77)

New Rust test modules:

```
tests/bmp/path_status_tlv.rs        — 8 tests (parse_best_bit, ecmp, backup, filtered, reason codes, roundtrip)
tests/filter/roto_engine.rs         — 6 tests (accept_valid, reject_bogon_10/192, rpki_invalid, community_has, as_path_contains)
tests/mcp/mcp_dispatch.rs           — 5 tests (initialize_shape, tools_list_11_names, unknown_method_-32601, unknown_tool_isError, nl_token_budget_exhaustion)
tests/governor/governor_state.rs    — 4 tests (shed_signal_lifecycle, snapshot_correctness, rate_counter_increment, write_pressure_flag)
tests/bgpsec/signature_parse.rs     — 3 tests (block_present, block_graceful_missing, asn_extraction)
tests/output/adapter_cursor.rs      — 4 tests (cursor_file_created, cursor_advances, cursor_survives_restart, dedup_window_suppresses_repeat)
tests/convergence/event_detection.rs — 5 tests (peer_down_starts_tracking, withdrawals_increment, stats_type7_finalizes, idle_5s_finalizes, ms_computed_correctly)
```

**Target**: 200 tests. Gap from 77: 123 new tests.
**Gate**: `cargo test --workspace` — 0 failures, ≥200 tests.

---

### A2 — `scripts/check_wiring.sh` (Layer 1 — 15 checks, <15s)

```bash
#!/usr/bin/env bash
set -euo pipefail; ERRORS=0
check() { eval "$2" &>/dev/null && echo "PASS: $1" || { echo "FAIL: $1"; ERRORS=$((ERRORS+1)); }; }

check "api/filters wired"          "grep -q 'pub mod filters'    crates/rbmp-server/src/api/mod.rs"
check "api/governance wired"       "grep -q 'pub mod governance' crates/rbmp-server/src/api/mod.rs"
check "api/external wired"         "grep -q 'pub mod external'   crates/rbmp-server/src/api/mod.rs"
check "api/schema wired"           "grep -q 'pub mod schema'     crates/rbmp-server/src/api/mod.rs"
check "mcp handler registered"     "grep -q 'mcp_server'         crates/rbmp-server/src/main.rs"
check "mcp has >= 11 tools"        "grep -c '\"name\"' crates/rbmp-server/src/mcp_server.rs | xargs -I{} test {} -ge 11"
check "output/elasticsearch"       "test -f crates/rbmp-server/src/output/elasticsearch.rs"
check "output/splunk"              "test -f crates/rbmp-server/src/output/splunk.rs"
check "no BONSAI_ in crates"       "! grep -rq 'BONSAI_VAULT_PASSPHRASE' crates/"
check "no BONSAI_ in bmppy"        "! grep -rq 'BONSAI_BOOTSTRAP_' bmppy/"
check "config/filters.roto"        "test -f config/filters.roto"
check "config/filters.yaml"        "test -f config/filters.yaml"
check "tests/seed.sql"             "test -f tests/seed.sql"
check "internet.py RipeStatClient" "grep -q 'class RipeStatClient' bmppy/rbmppy/internet.py"
check "policy_fetcher parseable"   "python3 -c 'import ast; ast.parse(open(\"bmppy/policy_fetcher.py\").read())'"

[ $ERRORS -gt 0 ] && { echo "WIRING FAILED: $ERRORS errors"; exit 1; }
echo "All wiring checks passed"
```

---

### A3 — BMP Fixture Corpus (`tests/fixtures/bmp/`)

Binary PDU captures committed to git — Layer 2 injects these over TCP:

```
tests/fixtures/bmp/
├── peer_up_xrd.bin                           # XRd 24.4.1 PeerUp PDU
├── peer_up_frr.bin                           # FRR 10.6.1 PeerUp PDU
├── peer_down_hold_timer.bin                  # PeerDown reason=hold-timer-expired
├── route_monitoring_ipv4_announce.bin
├── route_monitoring_ipv6_announce.bin
├── route_monitoring_evpn_type2.bin
├── route_monitoring_with_path_status_tlv.bin # RV7 feature
├── stats_report_type30.bin                   # RFC 9972 type 30 max-prefix gauge
└── routeviews_update.mrt                     # 2-minute RouteViews MRT slice
```

Capture tool: `scripts/capture_bmp_fixtures.py` — tcpdump extractor per `RUSTYBMP_TESTING_STRATEGY.md §Part 5`.

---

### A4 — `POST /api/_test/seed` Endpoint

Feature-gated (`#[cfg(feature = "test-endpoints")]`) — linchpin of Playwright testing.

```rust
// crates/rbmp-server/src/api/test_seed.rs
pub async fn seed_handler(State(state): State<AppState>, Json(req): Json<SeedRequest>) -> Json<Value> {
    let sql = match req.fixture.as_str() {
        "standard"           => include_str!("../../../../tests/seed.sql"),
        "anomaly_active"     => include_str!("../../../../tests/fixtures/seed_anomaly.sql"),
        "maxprefix_critical" => include_str!("../../../../tests/fixtures/seed_maxprefix.sql"),
        "convergence"        => include_str!("../../../../tests/fixtures/seed_convergence.sql"),
        _                    => return Json(json!({"error": "unknown fixture"})),
    };
    state.store.execute_seed_sql(sql).await;
    Json(json!({"ok": true, "fixture": req.fixture}))
}
```

Additional seed files:
- `tests/fixtures/seed_anomaly.sql` — active origin_change + route_leak (severity=critical)
- `tests/fixtures/seed_maxprefix.sql` — peer at 96% max-prefix; one at 100% (fired)
- `tests/fixtures/seed_convergence.sql` — convergence event: 847ms, 1234 affected prefixes

### Bundle A Gate

```bash
cargo test --workspace -- --test-output immediate | tail -1      # "200 passed; 0 failed"
bash scripts/check_wiring.sh                                      # "All wiring checks passed"
ls tests/fixtures/bmp/*.bin | wc -l                               # >= 8
curl -sfX POST localhost:7878/api/_test/seed -d '{"fixture":"standard"}' | jq .ok  # true
```

---

## Bundle B — Protocol & API Test Coverage

**Why second**: All C-I bundles add new features. Without B in place those features ship with zero automated regression. B gives the harness; subsequent bundles fill it.

**Prerequisite**: Bundle A.

### B1 — Layer 2: Protocol Integration Tests (`tests/protocol/test_bmp_parsing.py`)

Uses `rustybmp_server` pytest fixture (starts collector on port 17878/17879, in-memory DB). Each test injects raw bytes from `tests/fixtures/bmp/` via TCP socket.

Minimum 15 required tests:

| Test | Fixture | Assert |
|------|---------|--------|
| `test_peer_up_xrd` | `peer_up_xrd.bin` | `/api/peers` ≥1 peer state=up |
| `test_peer_down_transition` | peer_up + `peer_down_hold_timer.bin` | state=down |
| `test_ipv4_announce` | peer_up + `route_monitoring_ipv4_announce.bin` | `/api/routes` ≥1 ipv4 route |
| `test_ipv6_announce` | peer_up + `route_monitoring_ipv6_announce.bin` | ≥1 ipv6 route |
| `test_evpn_type2` | peer_up + `route_monitoring_evpn_type2.bin` | route with `is_evpn=true` |
| `test_stats_type30` | peer_up + `stats_report_type30.bin` | `/api/bmpstats/history` has counter_type=30 |
| `test_path_status_tlv` | peer_up + `route_monitoring_with_path_status_tlv.bin` | `/api/path-status/matrix` has entries |
| `test_mrt_ingestion` | `routeviews_update.mrt` | ≥10 routes after rbmp-mrt inject |
| `test_frr_peer_up` | `peer_up_frr.bin` | peer up, different peer_addr |
| `test_governance_endpoint` | (none) | `/api/governance` returns 3 pressure booleans |
| `test_swagger_ui` | (none) | `GET /api/swagger` 200 HTML |
| `test_openapi_json` | (none) | `GET /api/openapi.json` valid JSON with `openapi: "3.0.3"` |
| `test_mcp_initialize` | (none) | `POST /mcp` initialize → `protocol_version` present |
| `test_mcp_tools_list` | (none) | `POST /mcp` tools/list → 11 tools |
| `test_mcp_unknown_tool_is_error` | (none) | tools/call unknown → `isError: true` |

### B2 — Layer 3: API Contract Tests (`tests/api/test_all_endpoints.py`)

Every endpoint in `api/mod.rs` gets ≥1 passing test. Server pre-loaded with `tests/seed.sql`.

Coverage required (24 endpoints):
`/api/routes`, `/api/routes?prefix=`, `/api/routes/prefix/{p}/timeline`, `/api/peers`, `/api/peers/{addr}/capabilities`, `/api/speakers/summary`, `/api/capacity/max-prefix`, `/api/ml/anomalies`, `/api/events` (SSE), `/api/filters/test`, `/api/filters/stats`, `/api/path-status/matrix`, `/api/governance`, `/api/external/prefix-visibility`, `/api/swagger`, `/api/openapi.json`, `POST /mcp` (initialize, tools/list, tools/call), `/health`, `/api/rpki/analysis`, `/api/bgpls/graph`, `/api/convergence`, `/api/export/parquet`.

### B3 — `data-testid` on All Pages

Homepage ✅ done in RV8. Remaining pages:

| Page | Required testids |
|------|-----------------|
| `/peers` | `peer-table`, `peer-row-{addr}`, `peer-state-{addr}`, `peer-route-count-{addr}` |
| `/peers/[addr]` | `peer-detail-flap-count`, `peer-detail-session-uptime`, `peer-detail-rpki-invalid-pct` |
| `/prefixes` | `prefix-table`, `prefix-row-{prefix}`, `prefix-rpki-badge-{prefix}` |
| `/filters` | `filter-test-prefix`, `filter-test-rpki`, `filter-test-submit`, `filter-test-verdict`, `filter-reload-button`, `filter-reload-status-ok`, `filter-editor-toggle`, `filter-editor-textarea` |
| `/capacity` | `capacity-gauge-{addr}`, `capacity-pct-{addr}`, `capacity-eta-{addr}`, `capacity-critical-alert` |
| `/path-status` | `path-status-matrix`, `path-status-best-count`, `path-status-backup-count`, `path-status-filtered-count` |
| `/rpki` | `rpki-valid-count`, `rpki-invalid-count`, `rpki-not-found-count`, `rpki-coverage-pct` |
| `/ml` | `anomaly-table`, `anomaly-row-{id}`, `anomaly-severity-{id}`, `anomaly-kind-{id}` |
| `/topology` | `topology-canvas`, `topology-node-count`, `topology-edge-count`, `topology-lod-indicator` |
| `/policy` | `policy-table`, `policy-rib-type-filter`, `policy-diff-row-{prefix}` |

### Bundle B Gate

```bash
pytest tests/protocol/ -v --tb=short | tail -3    # "15+ passed"
pytest tests/api/ -v --tb=short | tail -3          # "24+ passed"
grep -r 'data-testid' ui/src/routes/ | wc -l       # >= 60
```

---

## Bundle C — Convergence Detection + BGPsec Validation

**Theme**: Two features with DuckDB table schemas in place since RV7, zero implementation code after two sprints.

**Prerequisite**: Bundle A (unit test modules for convergence + bgpsec).

### C1 — Convergence Event Detector

`bmppy/rbmppy/convergence_detector.py`

The `convergence_events` table is populated per this algorithm:
1. `peer_down` event → start tracking: record `started_at`, `speaker_addr`, reset withdrawal counter
2. Increment `withdraw_count` on each withdraw route_event from same peer
3. EOR detection: StatsReport type 7 or 8 counter drops to 0, **or** withdrawal rate falls below 5/sec for 5 consecutive seconds
4. `convergence_ms = (eor_at - started_at).total_seconds() * 1000`
5. `affected_prefixes` = count distinct prefixes withdrawn + re-announced
6. INSERT into `convergence_events`

```python
class ConvergenceDetector:
    def __init__(self, db_path: str):
        self._active: dict = {}   # peer_addr → {started_at, speaker_addr, withdraw_count, last_burst_ts}

    async def process_peer_event(self, event: PeerEvent) -> None:
        if event.event_type == "peer_down":
            self._active[event.peer_addr] = {
                "started_at": event.occurred_at,
                "speaker_addr": event.speaker_addr,
                "withdraw_count": 0,
                "last_burst_ts": event.occurred_at,
            }

    async def process_route_event(self, event: RouteEvent) -> None:
        if event.action == "withdraw" and event.peer_addr in self._active:
            self._active[event.peer_addr]["withdraw_count"] += 1
            self._active[event.peer_addr]["last_burst_ts"] = event.occurred_at

    async def process_stats_event(self, event: StatsEvent) -> None:
        # EOR signal: type 7 (adj-RIBs-In routes) or type 8 (loc-RIB routes) == 0
        if event.counter_type in (7, 8) and event.counter_value == 0:
            await self._finalize(event.peer_addr, event.occurred_at)

    async def run_idle_check(self) -> None:
        # Called every second; finalizes peers quiet for >= 5s
        now = datetime.now(UTC)
        for peer_addr, state in list(self._active.items()):
            if (now - state["last_burst_ts"]).total_seconds() >= 5 and state["withdraw_count"] > 0:
                await self._finalize(peer_addr, now)

    async def _finalize(self, peer_addr: str, eor_at: datetime) -> None:
        if peer_addr not in self._active:
            return
        state = self._active.pop(peer_addr)
        ms = (eor_at - state["started_at"]).total_seconds() * 1000
        # INSERT into convergence_events (peer_addr, speaker_addr, started_at, eor_at,
        #   convergence_ms, trigger_type, affected_prefixes)
```

Wire via tokio channel in `crates/rbmp-server/src/receiver.rs`.

After each finalized event, run `ConvergenceAnomalyDetector` (D4): if `convergence_ms > 3× 7-day P50` → insert into `ml_anomalies` with kind=`slow_convergence`.

### C2 — BGPsec Full ECDSA Validation

`crates/rbmp-enrichment/src/bgpsec.rs`

New deps in `crates/rbmp-enrichment/Cargo.toml`:
```toml
ring = "0.17"
x509-parser = "0.16"
```

```rust
use ring::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};

pub enum BgpsecVerdict { Valid, Invalid, NotCovered }

pub async fn validate_bgpsec_path(
    signature_blocks: &[u8],
    router_pubkey_der: &[u8],
    message: &[u8],
) -> BgpsecVerdict {
    let key = UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, router_pubkey_der);
    match key.verify(message, signature_blocks) {
        Ok(())  => BgpsecVerdict::Valid,
        Err(_)  => BgpsecVerdict::Invalid,
    }
}
```

Wire: when a route-monitoring PDU with type-30 BGPsec_Path attribute arrives, spawn a task → fetch router EE cert via RPKI repository (RFC 8182 RRDP/rsync) → validate → write result to `bgpsec_validations`.

UI: add `BgpsecBadge` component to prefix detail (mirrors `RpkiBadge`).

### Bundle C Gate

```bash
cargo test -p rbmp-enrichment -- bgpsec | tail -1      # "5 passed"
cargo test --test convergence_event_detection | tail -1 # "5 passed"
pytest tests/api/ -k "convergence" -v | tail -3         # "test_convergence_list passed"
pytest tests/protocol/ -k "convergence" --timeout=30 -v # convergence_ms > 0
```

---

## Bundle D — ML Pipeline Completion

**Theme**: Complete `to_pyg()` (stub since RV5), ship STGNN training, replace hijack heuristic with a classifier, add community semantics learning.

**Prerequisite**: Bundle A. Python deps: `torch`, `torch-geometric`, `scikit-learn`, `mlxtend`.

### D1 — Complete `to_pyg()` with Path Status TLV Features

`bmppy/ml/topology_snapshot.py` — add RV7 path marking columns to node feature matrix:

```python
NODE_FEATURE_COLS = [
    'route_count', 'churn_rate_1h', 'rpki_invalid_ratio', 'session_uptime_secs', 'flap_count_24h',
    'best_count', 'ecmp_count', 'backup_count', 'filtered_count', 'nonselected_count', 'redundancy_ratio',
]  # 11 features total

def to_pyg(self) -> "HeteroData":
    from torch_geometric.data import HeteroData; import torch
    data = HeteroData()
    data['peer'].x = torch.tensor(self.nodes_df[NODE_FEATURE_COLS].fillna(0).values, dtype=torch.float32)
    data['peer'].node_id = self.nodes_df['peer_addr'].tolist()
    # ... edge construction from bgpls_links (existing logic) ...
    return data
```

Add `topology_snapshots` DuckDB table (one row per 5-minute interval) to support STGNN sequence training.

### D2 — STGNN Training Script

`bmppy/ml/train_bgp_stgnn.py` — GATv2 spatial + GRU temporal:

```python
class BgpStgnn(torch.nn.Module):
    def __init__(self, in_channels=11, hidden=64, heads=4):
        super().__init__()
        self.conv1 = GATv2Conv(in_channels, hidden, heads=heads, concat=True)
        self.conv2 = GATv2Conv(hidden * heads, hidden, heads=1, concat=False)
        self.gru   = torch.nn.GRU(hidden, hidden, batch_first=True)
        self.out   = torch.nn.Linear(hidden, 1)  # anomaly prob per node

    def forward(self, snapshots: list) -> torch.Tensor:
        embs = [F.elu(self.conv2(F.elu(self.conv1(s.x, s.edge_index)), s.edge_index)) for s in snapshots]
        _, h_n = self.gru(torch.stack(embs, dim=1))
        return torch.sigmoid(self.out(h_n.squeeze(0)))
```

### D3 — Hijack Probability Classifier

`bmppy/ml/hijack_classifier.py` — replace `HijackDetector` heuristic with `GradientBoostingClassifier`:

Features: `origin_asn_changed`, `prefix_specificity`, `rpki_validity_enc` (0/1/2), `as_path_len_delta`, `aspa_verdict_enc`, `is_subprefix_of_known`, `peer_as_is_expected`.

Target: recall > 0.95, AUC ≥ 0.90. Fallback to heuristic when no `.pkl` checkpoint present.

Training data: BGPStream historical hijack events (labeled dataset via `bgpstream` Python library).

### D4 — Convergence Anomaly Detector

`bmppy/rbmppy/convergence_anomaly_detector.py` — fires `slow_convergence` anomaly when a new convergence event exceeds 3× the rolling 7-day P50 for that peer. Called automatically by C1's `_finalize()`.

### D5 — Community Semantics Learner

`bmppy/ml/community_learner.py` — `fpgrowth` (mlxtend) on pre/post policy community attribute correlations:

```python
def learn_community_semantics(db_path: str, min_support: float = 0.05) -> list[dict]:
    """Returns: [{"community": "65001:100", "inferred_meaning": "preferred transit (LP=200)", "confidence": 0.94}]"""
```

New table: `community_semantics(community_value, inferred_meaning, confidence, support, last_learned_at)`.
New endpoint: `GET /api/communities/semantics`.

### Bundle D Gate

```bash
python3 -c "
from bmppy.ml.topology_snapshot import TopologySnapshot
snap = TopologySnapshot.from_db('tests/fixtures/test.duckdb')
data = snap.to_pyg()
assert data['peer'].x.shape[1] == 11, f'Expected 11 features, got {data[\"peer\"].x.shape[1]}'
print('D1 PASS')
"
python3 -m pytest tests/ml/ -v | tail -3    # all passed, AUC >= 0.85 reported
pytest tests/api/ -k "communities" -v | tail -3
```

---

## Bundle E — Output Adapters Completion

**Theme**: RV8 shipped Elasticsearch + Splunk. Three remaining adapters + management UI.

**Prerequisite**: Bundle A (cursor/dedup unit tests from A1-output module).

### E1 — ServiceNow EM Adapter

`crates/rbmp-server/src/output/servicenow_em.rs`

Pattern: bonsai `src/output/servicenow_em.rs`. Pushes BGP anomalies to `em_event` table.

Severity map: `critical→1, high→2, warn→3, info→5`. Dedup key: `"{speaker_addr}:{peer_addr}:{anomaly_kind}"` with 300s dedup window. Cursor: `runtime/cursors/servicenow_em.cursor`.

Config:
```toml
[[output.adapters]]
name = "snow-em"
type = "servicenow_em"
instance_url = "https://example.service-now.com"
credential_alias = "snow-basic"
min_severity = "warn"
dedup_window_secs = 300
```

### E2 — Webhook Adapter

`crates/rbmp-server/src/output/webhook.rs`

Profile-driven. Built-in profiles: `Slack` (Blocks API), `PagerDuty` (Events v2 with dedup_key), `OpsGenie` (message + alias + priority), `Teams` (Adaptive Cards), `Custom` (raw Handlebars body_template + configurable headers).

All profiles honor `min_severity` filter and `dedup_window_secs`.

### E3 — Adapter Management API

```
GET  /api/adapters                — list configured adapters: name, type, health, last_push, event_count
GET  /api/adapters/{name}         — detail
POST /api/adapters/{name}/test    — test_connection(), returns {ok: bool, latency_ms}
GET  /api/adapters/{name}/audit   — last 20 push log entries
PUT  /api/adapters/{name}/enable  — toggle enabled without restart
```

### E4 — `/adapters` Management UI Page

`ui/src/routes/adapters/+page.svelte` — card grid per adapter. Each card: name, type icon, health badge (green/red/yellow), last push time, event count, "Test connection" button.

### Bundle E Gate

```bash
cargo test -p rbmp-server -- output | tail -1           # "4+ passed" (cursor tests)
cargo test -p rbmp-server -- adapter_smoke | tail -1    # mocked HTTP tests
pytest tests/api/ -k "adapters" -v | tail -3            # GET /api/adapters 200
```

---

## Bundle F — Enrichers + External APIs

**Theme**: Complete the external data surface. IRR validation, NetBox/SNOW CMDB enrichment, looking glasses, RIPE Atlas.

**Prerequisite**: Bundle A.

### F1 — NetBox Enricher

`crates/rbmp-enrichment/src/netbox.rs` — dual transport (REST or MCP proxy). Enriches `speaker_registry` with `hostname`, `site`, `role`, `model` from NetBox DCIM.

REST: `GET /api/dcim/devices/?primary_ip={ip}`. Cache TTL: 15 minutes. Config key: `credential_alias = "netbox-token"`.

### F2 — ServiceNow CMDB Enricher

`crates/rbmp-enrichment/src/servicenow_cmdb.rs` — `GET /api/now/table/cmdb_ci_network_gear?ip_address={ip}`. Returns `ci_name`, `u_role`, `u_site`. Also fetches open CHG records ±2h around policy diffs (correlates BMP-observed config changes with planned maintenance windows).

### F3 — IRR/RADB Client

`bmppy/rbmppy/irr_client.py` — asyncio TCP whois client to `whois.radb.net:43`:
- `validate_route(prefix, origin_as)` → `IrrResult(status="match"|"mismatch"|"no_match")`
- `get_as_set(as_set_name)` → `list[int]` (recursive expansion)
- `get_route_objects(asn)` → `list[str]`

New endpoint: `GET /api/external/irr-validation?prefix=X&origin_as=Y`

### F4 — Looking Glass: Cloudflare Radar + HE BGP

`bmppy/rbmppy/looking_glass.py` — adds `cloudflare_visible` + `he_bgp_origin` fields to existing `/api/external/prefix-visibility` response. Cloudflare Radar API key optional (10K/day free tier). HE BGP API — no auth, rate-limited.

### F5 — RIPE Atlas Traceroute

`bmppy/rbmppy/ripe_atlas.py` — async traceroute creation + result polling:
- `POST /api/external/traceroute` → `{"measurement_id": "12345678"}`
- `GET /api/external/traceroute/{id}` → results or `{"status": "pending"}`

API key optional; anonymous = 100 measurements/day.

### Bundle F Gate

```bash
cargo check -p rbmp-enrichment 2>&1 | grep -c "^error" | xargs test 0 -eq
python3 -m pytest tests/external/ -v -k "not real_network" | tail -3
pytest tests/api/ -k "external or irr or traceroute" -v | tail -3
```

---

## Bundle G — UI Completeness

**Theme**: Six new or completed UI surfaces. All pages instrumented with `data-testid`. Playwright suite covering five core pages.

**Prerequisite**: Bundles A4 (seed endpoint) + B3 (data-testid spec).

### G1 — `/query` Natural Language Query Page

`ui/src/routes/query/+page.svelte`:
- Plain-English textarea + submit
- Example query chips: "Show RPKI invalid routes today", "Which peers flapped 3+ times?", "Longest AS path this week?"
- Calls `nl_query` MCP tool via `POST /mcp`
- SQL preview panel (collapsible — shows generated SQL before execution)
- Results table + copy-to-CSV
- Daily token budget meter: `X / 500,000 tokens used today`

### G2 — `/adapters` Output Adapter Management Page (with E4)

Card per adapter. Health badge. Test connection button. Audit log drawer.

### G3 — Communities Explorer (`/communities`)

`ui/src/routes/communities/+page.svelte`:
- Frequency table: community value, route count, first/last seen
- Pre vs post policy column: does this community survive filtering?
- Inferred semantic label from `GET /api/communities/semantics` (D5)
- Timeline chart: community appearance over time

### G4 — Topology LOD (Adaptive Rendering)

`ui/src/routes/topology/+page.svelte` — switch D3 layout based on node count:
- `< 100 nodes` → existing force layout (no change)
- `100–1000 nodes` → hierarchical: cluster by AS, force within each AS cluster
- `>= 1000 nodes` → clustered: AS-level graph with route count badges

`data-testid="topology-lod-indicator"` shows current mode.

### G5 — FlowSpec Rules Viewer

New `/flowspec` page or section on `/policy`. Parses `route_events` where `afi=ipv4 AND safi=flowspec`. Shows match components (destination prefix, source prefix, IP protocol, port, DSCP, fragment), decoded action (drop/redirect), source peer and community encoding. Alert badge for rules covering prefixes > /20.

New endpoint: `GET /api/flowspec` — queries existing `route_events` table, no schema change needed.

### G6 — Multi-VRF Context Switcher

VRF dropdown in top nav (distinct `rd` values from `route_events`). When VRF selected, all API calls add `?vrf=RD` param. `/prefixes` groups by RD. `/topology` adds VRF overlay toggle.

New API param on: `/api/routes`, `/api/peers`, `/api/bgpls/graph`.

### G7 — Playwright Test Suite (Layer 7)

`ui/tests/` — minimum 15 tests across 5 pages. Every test seeds via `POST /api/_test/seed`.

Priority order of implementation:

1. `dashboard.spec.ts` — empty state onboarding, active state speaker cards, peer counts
2. `filters.spec.ts` — bogon reject, filter hot-reload accept-all override
3. `peers.spec.ts` — table renders, peer-row navigation to detail page
4. `capacity.spec.ts` — fuel gauge %, critical alert visible at 96%
5. `path-status.spec.ts` — matrix renders with seed data

`playwright.config.ts` prerequisites: server with `--features test-endpoints` + UI dev server at `http://127.0.0.1:5173`.

### Bundle G Gate

```bash
npx playwright test --reporter=list | tail -5    # "15+ passed, 0 failed"
grep -r 'data-testid' ui/src/routes/ | wc -l     # >= 80
```

---

## Bundle H — Lab Scenarios (ContainerLab)

**Theme**: Real router BMP sessions. Layer 4 (Tier 0 FRR, always available in CI) + Layer 5 (Tier 1 XRd, anomaly injection, RPKI testbed) + load scripts.

**Prerequisite**: Bundles A + B. Docker images: `quay.io/frrouting/frr:10.6.1` (free), `nlnetlabs/routinator:latest` (free), `ios-xr/xrd-control-plane:24.4.1` (Tier 1 — requires license).

### H1 — Scenario 01: FRR Minimal Smoke (Layer 4 — Tier 0)

`tests/scenarios/01_frr_minimal/` — two FRR PEs + rustybmp, 3 static prefixes, BMP pre+post+loc-rib. Runs in <3 minutes with no license requirements.

Config files: `topology.clab.yml`, `configs/frr-pe1/frr.conf`, `configs/frr-pe2/frr.conf`, `configs/rustybmp.toml`, `test_frr_bmp_smoke.py`.

5 required tests:
1. `test_bmp_peer_up_received` — ≥1 peer up within 60s
2. `test_three_routes_visible` — all 3 static networks in `/api/routes`
3. `test_pre_and_post_rib_populated` — `adj-in-pre` + `adj-in-post` in `/api/policy`
4. `test_bogon_filter_verdict` — `10.0.0.0/8` → reject
5. `test_sse_delivers_event` — SSE stream delivers ≥1 event during session

### H2 — Scenario 02: XRd RFC 9972 Validation (Layer 5 — Tier 1)

`tests/scenarios/02_xrd_functional/` — RFC 9972 stats types 18-38, Path Status TLV, max-prefix headroom.

5 required tests:
1. `test_stats_type30_headroom` — type 30 counter_value < configured max-prefix
2. `test_stats_afisafi_breakdown` — RFC 9972 stats include `afi`/`safi`
3. `test_path_status_tlv_received` — `/api/path-status/matrix` has entries (skip if XRd < 24.4)
4. `test_capacity_fuel_gauge` — `/api/capacity/max-prefix` reflects type 30 data
5. `test_peer_up_with_capabilities` — peer up includes `add_path` or `graceful_restart`

### H3 — Scenario 04: BGP Anomaly Injection (Layer 5)

`tests/scenarios/04_anomaly_injection/` — ExaBGP + FRR, 4 fault patterns:
1. Hijack: wrong-origin advertise → `origin_change` anomaly within 30s
2. Route leak: private ASN 64512 in path → `route_leak` anomaly
3. RPKI invalid more-specific /25 → filter reject
4. Peer flap: clear-bgp 3× in 10s → `flap` anomaly

### H4 — Scenario 05: RPKI Testbed (Layer 5 — Tier 0)

`tests/scenarios/05_rpki_testbed/` — `routinator:latest` RTR server + FRR. 3 tests: routinator RTR connects, RPKI-invalid route rejected post-policy, `/api/rpki/coverage` shows prefix covered.

### H5 — Load Testing Scripts

```
tests/load/
├── mrt_replay.py          # RouteViews MRT → inject via rbmp-mrt at 50K routes/s
├── ripe_ris_bridge.py     # RIPE RIS Live WebSocket → relay as BMP to collector
└── caida_bmp_relay.py     # CAIDA BGPStream Kafka topic → BMP relay
```

`mrt_replay.py` — target: ingest a 2M-route full table MRT dump in <120s with governor active.
`ripe_ris_bridge.py` — connect to `ris-live.ripe.net/v1/ws/`, relay 1000 updates/s for 60s, assert governor does not shed more than 5%.

### Bundle H Gate

```bash
# H1 (always runnable in CI)
pytest tests/scenarios/01_frr_minimal/ -v --timeout=180 | tail -3   # "5 passed"

# H4 (always runnable in CI)
pytest tests/scenarios/05_rpki_testbed/ -v --timeout=120 | tail -3  # "3 passed"

# H2, H3 (require vendor images — skip gracefully if absent)
pytest tests/scenarios/02_xrd_functional/ -v --timeout=480 | tail -3
pytest tests/scenarios/04_anomaly_injection/ -v --timeout=120 | tail -3

# H5 smoke (30s MRT replay)
python3 tests/load/mrt_replay.py --duration=30 --assert-route-count=5000
```

---

## Bundle I — New Capabilities

**Theme**: Features with no prior backlog entry. All are high-signal additions that leverage RV9's completed infrastructure.

**Prerequisite**: Bundles A + C (convergence) + D (ML).

### I1 — Grafana Dashboard Bundle

`grafana/rustybmp-dashboard.json` — pre-built Grafana dashboard for import.

Panels (all sourced from `GET /api/metrics` Prometheus endpoint):
- BMP messages/sec (rate gauge)
- Route events/sec (time series)
- Peer state timeline (state chart: up/down per peer)
- RPKI validity distribution (pie: valid/invalid/not-found)
- Governor memory pressure (gauge: 0-100%)
- Top anomaly kinds last 24h (horizontal bar)
- Convergence P50/P90/P99 (histogram)
- DuckDB query latency heatmap

Datasource: Prometheus scraping `http://{rbmp-host}:7878/api/metrics` at 15s interval.

### I2 — ACL/Prefix-List Generator

`bmppy/rbmppy/acl_generator.py` + new endpoint `POST /api/ml/generate-acl`.

```python
class AclGenerator:
    def generate_prefix_filter(self, prefixes: list[str], action: str = "deny") -> dict[str, str]:
        """Returns vendor configs: IOS-XR, FRR, JunOS, Arista EOS."""

    def generate_as_path_filter(self, asns: list[int], action: str = "deny") -> dict[str, str]:
        """Generate AS_PATH access-list for each vendor."""
```

UI: "Generate ACL" button on anomaly detail page. Returns collapsible vendor-specific config blocks.

### I3 — GoBGP + OpenBGPD NOS Coverage

Extend `tests/scenarios/03_multi_vendor/topology.clab.yml`:
- **GoBGP** (`quay.io/osrg/gobgp:latest`) — free, deterministic BMP behavior, zero license cost
- **OpenBGPD** (custom Dockerfile on alpine) — reference implementation, catches edge-case parser differences from FRR

Both added as Tier 0 nodes — always available in CI.

### I4 — OpenTelemetry Distributed Tracing

Trace the BMP pipeline end-to-end: TCP receive → parse → RIB update → DuckDB write → SSE push.

```toml
# New workspace Cargo.toml deps
opentelemetry         = "0.23"
opentelemetry-otlp    = "0.16"
tracing-opentelemetry = "0.24"
```

Each BMP message: one trace span. Tags: `bmp.message_type`, `bmp.peer_addr`, `bmp.speaker_addr`, `bmp.prefix` (route-monitoring).

Config:
```toml
[telemetry]
otlp_endpoint = "http://localhost:4317"
sample_rate   = 0.01   # 1% at internet scale
```

### I5 — YANG Push / gRPC Telemetry Correlation

`crates/rbmp-server/src/telemetry_correlator.rs` — lightweight gNMI subscription client.

Subscribes to interface operational state. When a BMP prefix change arrives, looks up correlated interface metrics (bandwidth utilization, error counts) at same timestamp.

New table: `interface_events(occurred_at, speaker_addr, interface, oper_status, bandwidth_util_pct)`.
New endpoint: `GET /api/correlation/prefix/{prefix}` — BGP events + correlated interface events on shared timeline.

### I6 — Policy Recommendation Engine

`bmppy/rbmppy/policy_advisor.py`:

```python
class PolicyAdvisor:
    def analyze_filter_gaps(self, recent_routes: list[RouteCtx]) -> list[PolicySuggestion]:
        """
        Detect routes accepted but probably shouldn't be (RPKI not-found + private ASN),
        routes rejected but probably should be accepted (known-good community + valid RPKI),
        and routes matching no explicit rule (fall-through).
        Returns suggested Roto rule additions with before/after snippets.
        """
```

New endpoint: `GET /api/filters/recommendations` — returns `PolicySuggestion[]` with Roto code.

### Bundle I Gate

```bash
# I1: Grafana JSON is valid
python3 -c "import json; json.load(open('grafana/rustybmp-dashboard.json')); print('PASS')"

# I2: ACL generator produces vendor configs
python3 -m pytest tests/ml/test_acl_generator.py -v | tail -3

# I3: GoBGP/OpenBGPD in multi-vendor scenario
pytest tests/scenarios/03_multi_vendor/ -v --timeout=300 | tail -3

# I4: OTel compiles
cargo check -p rbmp-server 2>&1 | grep -c "^error" | xargs test 0 -eq
```

---

## Bundle J — CI + Release Infrastructure

**Theme**: The GitHub Actions pipeline, `docs/CODEX_TESTING.md` runbook, and final verification that all layers pass.

**Prerequisite**: All other bundles complete and passing.

### J1 — GitHub Actions CI

`.github/workflows/ci.yml` — per `RUSTYBMP_TESTING_STRATEGY.md §Part 11`:

```yaml
name: CI
on: [push, pull_request]
jobs:
  layer0_unit:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --workspace -- --test-output immediate

  layer1_wiring:
    needs: layer0_unit
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: bash scripts/check_wiring.sh

  layer2_protocol:
    needs: layer1_wiring
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
      - run: pip install pytest requests pytest-json-report
      - run: pytest tests/protocol/ -v --tb=short --json-report --json-report-file=runtime/test_results/layer2.json

  layer3_api:
    needs: layer2_protocol
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
      - run: pytest tests/api/ -v --json-report --json-report-file=runtime/test_results/layer3.json

  layer4_frr_smoke:
    needs: layer3_api
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: bash -c "$(curl -sL https://get.containerlab.dev)"
      - run: docker pull quay.io/frrouting/frr:10.6.1
      - run: cargo build --workspace
      - run: pytest tests/scenarios/01_frr_minimal/ -v --timeout=180

  layer7_ui:
    needs: layer3_api
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
      - run: cd ui && npm ci && npm run build
      - run: npx playwright install chromium --with-deps
      - run: npx playwright test --reporter=github
```

### J2 — `docs/CODEX_TESTING.md`

Replaces `docs/UBUNTU_TESTING.md`. Shell-commands-only runbook. Independently executable per layer. Machine-readable JSON output at `runtime/test_results/<layer>.json`.

```markdown
# CODEX_TESTING.md — RustyBMP Automated Test Runbook
# Replaces: docs/UBUNTU_TESTING.md (deprecated)
# All commands run from repo root on Ubuntu 24.04.

## Prerequisites (one-time)
apt-get install -y python3 python3-pip duckdb containerlab
cargo build --workspace
pip install pytest requests pytest-json-report websockets httpx playwright
npx playwright install chromium --with-deps

## Layer 0 — Rust Unit Tests (<10s)
cargo test --workspace -- --test-output immediate
# Pass: exit 0, all 200 tests green

## Layer 1 — Wiring Checks (<15s)
bash scripts/check_wiring.sh
# Pass: exit 0, "All wiring checks passed"

## Layer 2 — Protocol Integration (<60s)
./target/debug/rbmp-collector --bmp-port 17878 --api-port 17879 --db :memory: --no-auth &
CPID=$!; sleep 0.5
pytest tests/protocol/ -v --json-report --json-report-file=runtime/test_results/layer2.json
EXIT=$?; kill $CPID; exit $EXIT

## Layer 3 — API Contract Tests (<90s)
./target/debug/rbmp-server --db :memory: --api-port 17880 --no-auth --features test-endpoints &
SPID=$!; sleep 0.5
pytest tests/api/ -v --json-report --json-report-file=runtime/test_results/layer3.json
EXIT=$?; kill $SPID; exit $EXIT

## Layer 4 — FRR Smoke Lab (<3min)
docker pull quay.io/frrouting/frr:10.6.1
pytest tests/scenarios/01_frr_minimal/ -v --timeout=180 \
  --json-report --json-report-file=runtime/test_results/layer4.json

## Layer 7 — UI End-to-End (<5min)
./target/debug/rbmp-server --db :memory: --api-port 7878 --features test-endpoints &
cd ui && npm run preview &
sleep 2
npx playwright test --reporter=github \
  --output=runtime/test_results/layer7.json
```

Codex reads `runtime/test_results/<layer>.json` → finds failing `"name"` → maps to test function in `tests/` → reads assertion message → fixes → reruns only that layer.

### Bundle J Gate

```bash
# All layers green
cargo test --workspace | grep -E "^test result" | grep -v "FAILED"
bash scripts/check_wiring.sh
pytest tests/protocol/ tests/api/ | tail -3
pytest tests/scenarios/01_frr_minimal/ --timeout=180 | tail -3
npx playwright test | tail -3
# Expected: 0 failures across all layers
```

---

## Part 1 — RV8 Delivery Audit (What Landed vs What Didn't)

### ✅ Confirmed Delivered in RV8

| Epic | File Evidence | Completeness |
|------|--------------|--------------|
| RV8-GOV1 Resource Governor (3 loops) | `governor.rs` — full `ResourceGovernor` struct, `spawn_memory_loop()`, `spawn_write_loop()`, `spawn_rate_loop()`, `should_shed()`, `record_event()` | Full |
| RV8-GOV2 `GET /api/governance` | `api/governance.rs` new file; registered in `api/mod.rs` | Full |
| RV8-GOV3 DuckDB write queue sizing | `config/rustybmp.toml.example` — `batch_size=5000`, `batch_timeout_ms=200`, `write_queue_capacity=100000` | Full |
| RV8-GOV4 Governor-aware batch expand | Loop 2 in `governor.rs` expands batch on sustained write pressure | Full |
| RV8-MC1 MCP server (11 tools) | `mcp_server.rs` 547 lines — JSON-RPC 2.0 at POST /mcp | Full |
| RV8-MC3 Daily NL token budget | `NL_TOKEN_BUDGET: AtomicU64 = 500_000`, midnight reset, `consume_nl_tokens()` | Full |
| RV8-MC4 ANOMALY_CATALOGUE | 5 entries: origin_change, route_leak, slow_convergence, rpki_invalid, flap | Full |
| RV8-OA1 OpenAPI 3.0.3 spec | `api/schema.rs` 237 lines — `build_spec()` + all tag groups | Full |
| RV8-OA2 Swagger UI | `GET /api/swagger` returns inline HTML, pulls from `/api/openapi.json` | Full |
| RV8-OUT1 OutputAdapter trait | `output/mod.rs` — `OutputAdapter` async trait + cursor pump (`BATCH_SIZE=256`) | Full |
| RV8-OUT2 Elasticsearch adapter | `output/elasticsearch.rs` new file — ECS-formatted `_bulk` ndjson | Full |
| RV8-OUT3 Splunk HEC adapter | `output/splunk.rs` new file — `POST /services/collector/event` | Full |
| RV8-EXT1 RIPE STAT client | `internet.py` +152 lines — `RipeStatClient` with `prefix_overview()`, `visibility`, async | Full |
| RV8-EXT5 `/api/external/prefix-visibility` | `api/external.rs` 158 lines — internal RIB + RIPE STAT + BGP.Tools side-by-side | Full |
| RV8-UX1 Adaptive homepage states | `+page.svelte` — empty/waiting/active state machine with `$derived` | Full |
| RV8-UX2 Speaker cards on dashboard | Speaker section with hostname, vendor, peers_up, route_count, RPKI% | Full |
| RV8-UX3 `GET /api/speakers/summary` | `peers.rs` — `speakers_summary()` handler; route registered | Full |
| RV8-UX4 Router config snippets | IOS-XR, FRRouting, Arista EOS, JunOS snippets in onboarding state | Full |
| RV8-T2 `tests/seed.sql` | 99-line deterministic DuckDB seed with schema + data for 2 speakers, 4+ peers | Full |
| RV8-T4 MCP integration tests | `tests/integration/mcp_tools.rs` 161 lines — JSON-RPC envelope + tool dispatch tests | Partial (Rust unit tests, not full integration) |
| Homepage data-testid | `data-testid` on: `onboarding-empty-state`, `speaker-card-*`, `speaker-status`, `summary-metrics` | Partial (homepage only) |

**Note on MCP tool names**: The delivered tools differ slightly from the RV8 spec. The spec listed 11 tools including `get_prefix_status`, `compute_igp_path`, `test_filter`, `get_capacity_status`. The patch delivers: `get_prefix_history`, `get_peer_flaps`, `get_anomalies`, `get_as_path_analysis`, `get_rpki_invalids`, `get_speaker_summary`, `get_prefix_visibility`, `get_convergence_events`, `get_policy_diff`, `get_community_summary`, `nl_query`. These are functionally equivalent but different names — update the spec to match the implementation.

---

### ❌ Not Delivered in RV8 (Confirmed by Patch Absence)

**Output / Integration:**

| Epic | Status | Note |
|------|--------|------|
| RV8-OUT4 ServiceNow EM adapter | ❌ Not in patch | Referenced only in BACKLOG comments |
| RV8-OUT5 Webhook adapter (Slack/PagerDuty) | ❌ Not in patch | Referenced only in BACKLOG comments |
| RV8-ENR1 NetBox enricher | ❌ Not in patch | IRR client referenced in backlog prose, not in code |
| RV8-ENR2 ServiceNow CMDB enricher | ❌ Not in patch | |
| RV8-EXT2 IRR/RADB client | ❌ Not in patch | `bmppy/rbmppy/irr_client.py` described but not created |
| RV8-EXT3 Looking glass: Cloudflare Radar + HE BGP | ❌ Not in patch | |
| RV8-EXT4 RIPE Atlas measurement creation | ❌ Not in patch | |
| RV8-OA3 Resolve endpoint `/api/resolve?q=X` | ❌ Not in patch | |
| RV8-MC2 Standalone NL→DuckDB SQL page | ❌ Not in patch | (nl_query MCP tool exists, but `/api/nl-query` REST endpoint and `/query` page do not) |
| RV8-UX5 `/adapters` management page | ❌ Not in patch | |
| RV8-UX6 `/query` NL query UI page | ❌ Not in patch | |
| RV8-UI5 Topology LOD (adaptive force/hierarchical/clustered) | ❌ Not in patch | Deferred since RV6 |

**ML:**

| Epic | Status | Note |
|------|--------|------|
| RV8-ML1 `to_pyg()` completion with Path Status features | ❌ Not in patch | Described in backlog as "stub since RV5" |
| RV8-ML2 `train_bgp_stgnn.py` GATv2-GRU training script | ❌ Not in patch | |
| RV8-ML3 Hijack probability classifier | ❌ Not in patch | Still heuristic-based |
| RV8-ML4 Convergence anomaly detector | ❌ Not in patch | `convergence_events` table exists (RV7), zero detection logic |
| RV8-ML5 Community semantics learner (fpgrowth) | ❌ Not in patch | |

**Testing — the largest gap:**

| Epic | Status | Note |
|------|--------|------|
| RV8-T1 BMP fixture capture from XRd/SRL/FRR | ❌ Not in patch | `tests/fixtures/bmp/` directory doesn't exist |
| RV8-T3 `POST /api/_test/seed` endpoint | ❌ Not in patch | `tests/seed.sql` exists but the HTTP endpoint to load it does not |
| RV8-T5 `tests/api/test_all_endpoints.py` | ❌ Not in patch | No Python API contract tests exist at all |
| RV8-T6 `data-testid` on all interactive UI elements | ❌ Partial | Homepage done; peers, prefixes, filters, capacity, path-status pages: none |
| RV8-T7 `tests/scenarios/01_frr_minimal/` | ❌ Not in patch | No ContainerLab scenarios exist |
| RV8-T8 `scripts/check_wiring.sh` | ❌ Not in patch | Layer 1 does not exist |
| RV8-T9 `tests/scenarios/02_xrd_rfc9972/` | ❌ Not in patch | |
| RV8-T10 `tests/load/mrt_replay.py` | ❌ Not in patch | |
| RV8-T11 `tests/load/ripe_ris_bridge.py` | ❌ Not in patch | |
| RV8-T12 Playwright test suite | ❌ Not in patch | No `.spec.ts` files exist |
| RV8-T13 GitHub Actions CI | ❌ Not in patch | No `.github/workflows/ci.yml` |
| RV8-T14 CAIDA Kafka BMP relay | ❌ Not in patch | |
| RV8-T20 Path Status TLV fixture + Layer 2 test | ❌ Not in patch | |

**From RV7 still deferred:**

| Item | Status |
|------|--------|
| BGPsec ECDSA validation (ring crate + router certs) | ❌ Not in RV7 or RV8 — table schema exists only |
| Convergence event detection Python logic | ❌ Not in any sprint — table exists, zero detection code |
| Batfish Tier 2 policy parser | ❌ Optional, never started |
| BGP-LS topology LOD | ❌ Deferred since RV6 |

---

## Part 2 — The Ubuntu Testing Guide Decision

### Which document should Codex use?

**The old `docs/UBUNTU_TESTING.md` (204 lines, 7 manual scenarios) is deprecated and must not be used.**

Reasons, per the testing strategy:
- It is sequential narrative prose — a human can follow it, Codex cannot run it
- It conflates build, protocol, API, and UI layers into one undifferentiated script
- It has no machine-readable pass/fail oracle at any step
- When it fails at step 4, nothing tells you whether steps 1-3 are the cause

**The `RUSTYBMP_TESTING_STRATEGY.md` is the authoritative design document.** It explains the seven-layer architecture, the JSON contract each layer produces, and the data sources. It is the north star.

**But the strategy document is not itself Codex-executable.** It is a design spec. Codex needs a third document.

### What RV9 must produce: `docs/CODEX_TESTING.md`

This is the step-by-step Ubuntu guide that replaces `UBUNTU_TESTING.md`. It must satisfy:

1. **Shell-only commands** — no prose instructions requiring human judgment
2. **Layer-isolated** — Codex can run Layer N without needing Layers 1..N-1 to have passed
3. **Machine-readable output** — every layer writes `runtime/test_results/<layer>.json`
4. **Explicit pass criterion** — `exit 0` = pass, `exit 1` = fail, never ambiguous
5. **Self-contained prerequisites** — each layer section lists what it requires

### Current runnable state per layer (honest assessment)

| Layer | Today (post-RV8) | Blocking gap |
|-------|-----------------|-------------|
| Layer 0 — `cargo test --workspace` | ✅ Runnable now | None — 77+ tests pass |
| Layer 1 — `scripts/check_wiring.sh` | ❌ NOT runnable | Script doesn't exist |
| Layer 2 — `pytest tests/protocol/` | ❌ NOT runnable | No Python tests, no BMP fixtures |
| Layer 3 — `pytest tests/api/` | ❌ NOT runnable | No Python tests; `POST /api/_test/seed` missing |
| Layer 4 — `pytest tests/scenarios/01_frr_minimal/` | ❌ NOT runnable | No clab scenario exists |
| Layer 5 — `pytest tests/scenarios/02+` | ❌ NOT runnable | No clab scenarios exist |
| Layer 6 — `python tests/load/*.py` | ❌ NOT runnable | No load scripts exist |
| Layer 7 — `npx playwright test` | ❌ NOT runnable | No `.spec.ts` files; no `data-testid` on most pages |

### The RV9 testing deliverable: minimum viable test pyramid

The full seven-layer system is the RV9+ target. The minimum viable subset that Codex can actually run at RV9 completion:

```
Layer 0:  cargo test --workspace                    ← 200+ tests (from 77 now)
Layer 1:  bash scripts/check_wiring.sh              ← 10 checks, <15s
Layer 2:  pytest tests/protocol/ -v --json-report   ← 20+ BMP parse checks
Layer 3:  pytest tests/api/ -v --json-report        ← every endpoint covered
Layer 4:  pytest tests/scenarios/01_frr_minimal/    ← FRR only (Tier 0, always available)
Layer 7:  npx playwright test                       ← homepage + peers + filters
```

Layers 5 (XRd/multi-vendor), 6 (internet-scale), and deeper scenarios stay as developer-only. CI runs Layers 0-4 + 7.

---

## Part 3 — RV9 Epic Index

### Testing Epics (P0 — All deferred from RV8)

#### RV9-T0: Target 200 Unit Tests
**Scope**: Rust unit tests only — no network, no DB.

New tests needed in `tests/`:
- `tests/bmp/path_status_tlv.rs` — 8 tests for the 12 status bits + 11 reason codes (RV7 feature, zero tests)
- `tests/filter/roto_engine.rs` — 5 tests: accept valid, reject bogon, reject RPKI-invalid, community_has, as_path_contains
- `tests/mcp/mcp_dispatch.rs` — 5 tests: initialize response shape, tools/list completeness (all 11 tool names), unknown method -32601, unknown tool isError, nl_query token budget exhaustion
- `tests/governor/governor_loops.rs` — 3 tests: shed signal lifecycle, snapshot correctness, rate counter increment
- `tests/convergence/event_detection.rs` — 5 tests for the convergence detection logic (once it exists)
- `tests/bgpsec/signature_parse.rs` — 3 tests for raw ECDSA block parsing (parse already done in RV6)

Target: 200 tests. Current: 77.

#### RV9-T1: `scripts/check_wiring.sh` (Layer 1)
**15 checks, <15s, no build required.**

```bash
#!/usr/bin/env bash
# scripts/check_wiring.sh  — Layer 1 wiring gate
set -euo pipefail
ERRORS=0
check() { local d="$1" c="$2"; eval "$c" &>/dev/null && echo "PASS: $d" || { echo "FAIL: $d"; ERRORS=$((ERRORS+1)); }; }

check "filters module wired"        "grep -r 'pub mod filters'     crates/rbmp-server/src/api/mod.rs"
check "governance module wired"     "grep -r 'pub mod governance'  crates/rbmp-server/src/api/mod.rs"
check "mcp handler registered"      "grep -r 'mcp_server::handler' crates/rbmp-server/src/main.rs"
check "output elasticsearch wired"  "grep -r 'mod elasticsearch'   crates/rbmp-server/src/output/mod.rs"
check "vault env var correct"       "! grep -r 'BONSAI_VAULT_PASSPHRASE' crates/"
check "ssh env var correct"         "! grep -r 'BONSAI_BOOTSTRAP_' bmppy/"
check "filters.roto exists"         "test -f config/filters.roto"
check "filters.yaml exists"         "test -f config/filters.yaml"
check "seed.sql exists"             "test -f tests/seed.sql"
check "rbmp-mrt compiles"           "cargo check -p rbmp-mrt --quiet 2>/dev/null"
check "rbmp-enrichment compiles"    "cargo check -p rbmp-enrichment --quiet 2>/dev/null"
check "policy_fetcher importable"   "python3 -c 'import ast; ast.parse(open(\"bmppy/policy_fetcher.py\").read())'"
check "internet.py has RipeStatClient" "grep -q 'class RipeStatClient' bmppy/rbmppy/internet.py"
check "mcp_server has 11 tools"     "grep -c '\"name\"' crates/rbmp-server/src/mcp_server.rs | xargs test 11 -le"
check "governor has 3 loops"        "grep -c 'spawn_.*_loop' crates/rbmp-server/src/governor.rs | xargs test 3 -le"

[ $ERRORS -gt 0 ] && { echo "WIRING CHECK FAILED: $ERRORS error(s)"; exit 1; }
echo "All wiring checks passed"
```

#### RV9-T2: BMP Fixture Corpus (`tests/fixtures/bmp/`)
**One-time capture. Binary files committed to git.**

Required fixtures:
```
tests/fixtures/bmp/
├── peer_up_xrd.bin                              # XRd 24.4.1 PeerUp PDU
├── peer_up_frr.bin                              # FRR 10.6 PeerUp PDU
├── peer_down_hold_timer.bin                     # PeerDown reason=hold-timer-expired
├── route_monitoring_ipv4_announce.bin
├── route_monitoring_ipv6_announce.bin
├── route_monitoring_evpn_type2.bin
├── route_monitoring_with_path_status_tlv.bin    # RV7 feature test
├── stats_report_type30.bin                      # RFC 9972 type 30
├── routeviews_update.mrt                        # 2-minute RouteViews slice
└── README.md                                    # capture methodology
```

Capture script: `scripts/capture_bmp_fixtures.py` — tcpdump-based extractor per the testing strategy §Part 5.

#### RV9-T3: `POST /api/_test/seed` Endpoint
**Required for Playwright — the linchpin of operator-free UI testing.**

```rust
// crates/rbmp-server/src/api/test_seed.rs
// Feature-gated: #[cfg(feature = "test-endpoints")]

pub async fn seed_handler(
    State(state): State<AppState>,
    Json(req): Json<SeedRequest>,
) -> Json<Value> {
    let sql = match req.fixture.as_str() {
        "standard"         => include_str!("../../../../tests/seed.sql"),
        "anomaly_active"   => include_str!("../../../../tests/fixtures/seed_anomaly.sql"),
        "maxprefix_critical" => include_str!("../../../../tests/fixtures/seed_maxprefix.sql"),
        "convergence"      => include_str!("../../../../tests/fixtures/seed_convergence.sql"),
        _                  => return Json(json!({"error": "unknown fixture"})),
    };
    // Truncate and reload
    state.store.execute_seed_sql(sql).await;
    Json(json!({"ok": true, "fixture": req.fixture}))
}
```

Additional seed files to create:
- `tests/fixtures/seed_anomaly.sql` — active origin_change + route_leak anomalies
- `tests/fixtures/seed_maxprefix.sql` — peer at 96% max-prefix utilization
- `tests/fixtures/seed_convergence.sql` — recent convergence event with 847ms duration

#### RV9-T4: Layer 2 — Protocol Integration Tests
`tests/protocol/test_bmp_parsing.py` — 20+ checks per the testing strategy §Part 5.

Key tests: peer_up_xrd, peer_down transitions, ipv4_announce, ipv6_announce, evpn_type2, stats_type30, path_status_tlv, mrt_sample_ingestion. All use the `rustybmp_server` pytest fixture and inject via TCP from fixture files.

#### RV9-T5: Layer 3 — API Contract Tests
`tests/api/test_all_endpoints.py` — every endpoint. Uses `server_with_seed_data` fixture that pre-loads `tests/seed.sql`.

Endpoint coverage required: `/api/routes`, `/api/routes?prefix=`, `/api/routes/prefix/{p}/timeline`, `/api/peers`, `/api/peers/{addr}/capabilities`, `/api/capacity/max-prefix`, `/api/ml/anomalies`, `/api/events` (SSE), `/api/filters/test`, `/api/filters/stats`, `/api/path-status/matrix`, `/api/governance`, `/api/speakers/summary`, `/api/external/prefix-visibility`, `/api/swagger`, `/api/openapi.json`, `POST /mcp` (initialize + tools/list + tools/call).

#### RV9-T6: `data-testid` Completion (all pages)
Homepage: ✅ done in RV8.
Pages needing `data-testid` instrumentation:

| Page | Elements needing testid |
|------|------------------------|
| `/peers` | `peer-table`, `peer-row-{addr}`, `peer-state-{addr}`, `peer-route-count-{addr}` |
| `/peers/[addr]` | `peer-detail-flap-count`, `peer-detail-uptime`, `peer-detail-rpki-invalid-pct` |
| `/prefixes` | `prefix-table`, `prefix-row-{prefix}`, `prefix-rpki-badge-{prefix}` |
| `/filters` | `filter-test-prefix`, `filter-test-rpki`, `filter-test-submit`, `filter-test-verdict`, `filter-editor-toggle`, `filter-editor-textarea`, `filter-reload-button`, `filter-reload-status-ok` |
| `/capacity` | `capacity-gauge-{addr}`, `capacity-critical-alert`, `capacity-eta-{addr}` |
| `/path-status` | `path-status-matrix`, `path-status-best-count`, `path-status-backup-count` |
| `/rpki` | `rpki-valid-count`, `rpki-invalid-count`, `rpki-not-found-count` |
| `/ml` | `anomaly-row-{id}`, `anomaly-severity-{id}`, `anomaly-kind-{id}` |

#### RV9-T7: Layer 4 — FRR Smoke Lab
`tests/scenarios/01_frr_minimal/` — full implementation per the testing strategy §Part 8 Scenario 01.

**Mandatory deliverables**:
- `topology.clab.yml` — FRR-PE1 + FRR-PE2 + rustybmp nodes
- `configs/frr-pe1/frr.conf` — BGP 65001 + BMP target + 3 static networks
- `configs/frr-pe2/frr.conf` — BGP 65002 + peer to PE1
- `configs/rustybmp.toml` — collector config for test env
- `test_frr_bmp_smoke.py` — 5 tests: PeerUp, pre+post RIB, routes, filter verdict, SSE

**Run**: `pytest tests/scenarios/01_frr_minimal/ -v --timeout=120`

#### RV9-T8: Layer 7 — Playwright Test Suite
`ui/tests/` — priority pages first.

```typescript
// Priority order for implementation:
// 1. ui/tests/dashboard.spec.ts    — empty/waiting/active state machine
// 2. ui/tests/filters.spec.ts      — filter test + hot-reload
// 3. ui/tests/peers.spec.ts        — table + peer-detail navigation
// 4. ui/tests/capacity.spec.ts     — fuel gauge + critical alert
// 5. ui/tests/path-status.spec.ts  — matrix rendering

// playwright.config.ts prerequisites:
// - Server running with --feature test-endpoints
// - seed endpoint available at POST /api/_test/seed
// - UI at http://127.0.0.1:5173
```

#### RV9-T9: GitHub Actions CI
`.github/workflows/ci.yml` — per the testing strategy §Part 11.

Jobs: `layer0_unit` → `layer1_wiring` → `layer2_protocol` → `layer3_api` → `layer4_frr_smoke` (parallel with) `layer7_ui`.

Layer 4 requires: `docker pull quay.io/frrouting/frr:10.6.1` + ContainerLab install via `curl -sL https://get.containerlab.dev`.

---

### Output / Integration Epics (Carried from RV8)

#### RV9-OUT4: ServiceNow EM Adapter
`crates/rbmp-server/src/output/servicenow_em.rs`

Pattern: copy from bonsai `src/output/servicenow_em.rs`. Same `POST /api/now/table/em_event` endpoint. Severity mapping: `critical→1, high→2, warn→3, info→4`. `message_key` = `{speaker_addr}:{peer_addr}:{anomaly_kind}` for ServiceNow native dedup. Cursor persistence: `runtime/cursors/servicenow_em.cursor`.

#### RV9-OUT5: Webhook Adapter (Slack / PagerDuty / OpsGenie / Teams)
`crates/rbmp-server/src/output/webhook.rs`

Not in bonsai — new for rustybmp. Profile-based: each profile defines `url`, `method`, `body_template` (Handlebars), `headers`, `severity_filter`, `deduplicate_by` fields.

Built-in profiles:
- **Slack**: `text` field with `{{severity_emoji}} *{{kind}}*: {{prefix}} ({{description}})`
- **PagerDuty Events v2**: `routing_key` from vault, `event_action: "trigger"`, `dedup_key: "{{speaker_addr}}-{{prefix}}-{{kind}}"`
- **OpsGenie**: `message`, `alias`, `priority` mapping from severity
- **Generic webhook**: raw JSON, configurable body template

#### RV9-OUT6: Adapter Management UI (`/adapters` page)
New SvelteKit page. Shows: registered adapters, health status (✅/❌), last push time, event count. Inline test button calls `POST /api/adapters/{name}/test`. Uses same card layout pattern as speakers on dashboard.

#### RV9-ENR1: NetBox Enricher
`crates/rbmp-enrichment/src/netbox.rs`

Source: bonsai `src/enrichment/netbox.rs`. Dual transport: REST (direct HTTP) or MCP (`netbox-mcp-server`). Enrichment: for each BMP speaker IP, fetch NetBox device CI → hostname, role, site, manufacturer. Attaches to `GovernanceSnapshot` for dashboard display. Cache TTL: 15 minutes.

#### RV9-ENR2: ServiceNow CMDB Enricher
`crates/rbmp-enrichment/src/servicenow_cmdb.rs`

Source: bonsai `src/enrichment/servicenow.rs`. REST to `/api/now/table/cmdb_ci_netgear?ip_address={addr}`. Returns: `ci_name`, `u_role`, `u_site`. Used to populate `speaker.hostname` and `speaker.vendor` when BMP init info doesn't include them.

#### RV9-EXT2: IRR/RADB Client
`bmppy/rbmppy/irr_client.py`

```python
class IrrClient:
    DATABASES = ["RIPE", "ARIN", "APNIC", "LACNIC", "AFRINIC", "RADB", "NTTCOM"]

    async def validate_route(self, prefix: str, origin_as: int) -> IrrResult:
        # Whois query to whois.radb.net:43 via asyncio TCP
        # Route object: ROUTE {prefix} ORIGIN AS{origin_as}
        # Returns: match | no_match | unknown

    async def get_as_set(self, as_set_name: str) -> list[int]:
        # Expand AS-SET recursively — used by hijack classifier

    async def get_route_objects(self, asn: int) -> list[str]:
        # All prefixes in IRR for this ASN
```

New API endpoint: `GET /api/external/irr-validation?prefix=X&origin_as=Y`

#### RV9-EXT3: Looking Glass — Cloudflare Radar + HE BGP
`bmppy/rbmppy/looking_glass.py`

```python
async def cloudflare_radar_prefix(prefix: str, api_key: str) -> dict:
    # GET https://radar.cloudflare.com/api/v4/bgp/routes?prefix={prefix}
    # Returns: visibility %, AS path diversity, route leak score

async def he_bgp_prefix(prefix: str) -> dict:
    # GET https://bgp.he.net/api/5/{prefix}
    # No auth required, rate-limited. Returns: origin ASNs, IRR status
```

These feed into `/api/external/prefix-visibility` response, adding `cloudflare_visible` and `he_bgp_origin` fields to the discrepancy analysis.

#### RV9-EXT4: RIPE Atlas Traceroute
`bmppy/rbmppy/ripe_atlas.py`

```python
class RipeAtlasClient:
    BASE = "https://atlas.ripe.net/api/v2"

    async def create_traceroute(self, target_ip: str, probe_count: int = 10) -> str:
        # POST /measurements/  — returns measurement_id

    async def get_results(self, meas_id: str) -> list[dict]:
        # GET /measurements/{meas_id}/results/
        # Poll until status = "stopped"

    async def path_to_prefix(self, prefix: str) -> dict:
        # Resolve BGP next-hop → create traceroute → return hop-by-hop path
```

New API endpoint: `POST /api/external/traceroute` (body: `{"prefix": "203.0.113.0/24"}`) — async, returns `measurement_id`; `GET /api/external/traceroute/{id}` for results.

---

### ML Epics (Carried from RV8)

#### RV9-ML1: Complete `to_pyg()` in topology_snapshot.py
`bmppy/ml/topology_snapshot.py`

Add Path Status TLV node features — these are the most valuable additions from RV7 to the graph model:

```python
def to_pyg(self) -> "HeteroData":
    from torch_geometric.data import HeteroData
    import torch
    data = HeteroData()
    # Node feature matrix — now includes RV7 path status columns
    feature_cols = [
        'route_count', 'churn_rate_1h', 'rpki_invalid_ratio',
        'session_uptime_secs', 'flap_count_24h',
        'best_count',         # from path_markings (RV7)
        'ecmp_count',         # from path_markings (RV7)
        'backup_count',       # from path_markings (RV7)
        'redundancy_ratio',   # best / (best + backup)
        'filtered_count',     # filtered-inbound + filtered-outbound
    ]
    data['peer'].x = torch.tensor(
        self.nodes_df[feature_cols].fillna(0).values,
        dtype=torch.float32
    )
```

#### RV9-ML2: STGNN Training Script
`bmppy/ml/train_bgp_stgnn.py`

GATv2 (spatial) + GRU (temporal) model trained on snapshot sequences from DuckDB. Training data: `topology_snapshots` table (add this table to schema — one row per 5-minute interval). Labels: anomaly present/absent per peer at each snapshot. Evaluation: AUC-ROC on held-out 20% of snapshots.

```python
class BgpStgnn(torch.nn.Module):
    def __init__(self, in_channels=10, hidden=64, heads=4):
        super().__init__()
        self.conv1 = GATv2Conv(in_channels, hidden, heads=heads)
        self.gru   = torch.nn.GRU(hidden * heads, hidden, batch_first=True)
        self.out   = torch.nn.Linear(hidden, 1)  # anomaly prob per node

    def forward(self, data_sequence):
        # data_sequence: list of PyG Data snapshots
        embeddings = [F.elu(self.conv1(d.x, d.edge_index)) for d in data_sequence]
        seq = torch.stack(embeddings, dim=1)  # [N, T, H]
        _, h = self.gru(seq)
        return torch.sigmoid(self.out(h.squeeze(0)))
```

#### RV9-ML3: Hijack Probability Classifier
`bmppy/ml/hijack_classifier.py`

Replace `HijackDetector` heuristic with a `GradientBoostingClassifier` (scikit-learn):

Features: `origin_asn_changed` (bool), `prefix_specificity` (int: /24 vs /25 etc.), `rpki_validity_enc` (0=valid, 1=not-found, 2=invalid), `as_path_len_delta` (vs 7-day median), `aspa_verdict_enc` (0=valid, 1=unknown, 2=invalid), `is_subprefix_of_known` (bool), `peer_as_historical` (bool: is this peer_as expected for this prefix?).

Training labels: BGPStream historical hijack events (fetch via `bgpstream` Python library).

Threshold tuning: optimize for recall > 0.95 (miss no real hijacks) even at cost of precision.

#### RV9-ML4: Convergence Event Detector
`bmppy/rbmppy/convergence_detector.py`

The `convergence_events` table was created in RV7. The detection logic was never written. This must poll `peer_events` for `peer_down` events and then measure time to EOR (End-of-RIB marker from StatsReport type 7/8 or route-monitoring cessation):

```python
class ConvergenceDetector:
    """
    Detect BGP convergence events from BMP data.

    Algorithm:
    1. On peer_down: record started_at, trigger_type = "peer_down"
    2. Wait for withdrawal cascade: count withdraw route_events from same speaker
    3. On EOR marker (StatsReport type 7/8) or withdrawal rate drops to <5/sec
       for 5 consecutive seconds: record eor_at
    4. convergence_ms = (eor_at - started_at).total_seconds() * 1000
    5. affected_prefixes = count of withdrawn + re-announced prefixes
    6. Insert into convergence_events
    """

    async def process_peer_event(self, event: PeerEvent) -> None: ...
    async def process_stats_event(self, event: StatsEvent) -> None: ...
    async def _check_convergence_complete(self, peer_addr: str) -> None: ...
```

Wire into the BMP processing pipeline in `rbmp-server/src/receiver.rs`.

#### RV9-ML5: Community Semantics Learner
`bmppy/ml/community_learner.py`

Use `mlxtend.frequent_patterns.fpgrowth` on pre/post policy community correlations:

```python
def learn_community_semantics(db_path: str) -> dict[str, str]:
    """
    Mine frequent community→attribute patterns from route_events.
    Returns: {community_value: inferred_meaning}

    Examples discovered:
      "65001:100" → "preferred transit (LP=200, always in post-policy)"
      "65535:666" → "blackhole (always absent from Loc-RIB)"
      "65001:200" → "backup path (LP=100, present pre-policy, filtered post-policy)"
    """
```

New API endpoint: `GET /api/communities/semantics` — returns learned community meanings + confidence scores.

---

### UI Epics (New and Carried from RV8)

#### RV9-UX1: `/query` Natural Language Query Page
SvelteKit page with: text input, submit button, SQL display (collapsible), results table. Sends to `nl_query` MCP tool or a new `POST /api/nl-query` REST endpoint. Includes example queries (chips): "Show RPKI invalid routes in the last hour", "Which peers have flapped more than 3 times today?", "What prefix has the longest AS path?".

#### RV9-UX2: `/adapters` Output Adapter Management Page
Per RV8-UX5. Shows each configured adapter with health badge. Inline test connection button. Links to docs for each adapter type.

#### RV9-UX3: Communities Explorer (`/communities`)
New page exposing community data from ML5 learner:
- Community frequency table (most-seen values, count of routes carrying each)
- Pre vs post policy presence (infer what the filter does to each community)
- Inferred semantic label from `GET /api/communities/semantics`
- Community timeline: when did `65001:100` first appear? Any sudden changes?

#### RV9-UX4: Topology LOD (Adaptive Rendering)
Per RV8-UI5 / RV6 D15. The force layout (`d3-force`) is currently used for all sizes. For >100 nodes it becomes unusable.

Thresholds:
- `<100 nodes`: current force layout (no change)
- `100-1000 nodes`: hierarchical layout — cluster by AS, force within cluster
- `>1000 nodes`: clustered/geographic — aggregate by AS, show AS-level graph

Implementation: detect node count in `topology/+page.svelte`, switch layout component. The `N` type already has `fx?: number|null; fy?: number|null` (D21) for drag pinning.

#### RV9-UX5: FlowSpec Rules Viewer
New section on `/policy` page or standalone `/flowspec` page. FlowSpec rules are parsed (RFC 5575 since RV4) but never visualized. Show:
- Active flowspec rules per speaker (type 1-12 match components)
- Rule action (traffic-rate-bytes=0 → drop; redirect → policy route)
- Rule source ASN + community encoding
- Alert when a flowspec rule covers a large prefix (potential DDoS mitigation in progress)

#### RV9-UX6: Multi-VRF Context Switcher
BMP captures VPN routes (EVPN, L3VPN per RFC 7432/6514) but the UI treats all routes as one flat table. For service providers with hundreds of VRFs, this is unusable.

Add: a VRF dropdown (populated from distinct `rd` values in `route_events`), filter all route/prefix views by selected VRF. On the EVPN page, group by route distinguisher. On the topology page, show VRF overlay toggle.

---

### New RV9 Features (Not in Any Prior Backlog)

#### RV9-NEW1: Grafana Dashboard Bundle
`grafana/rustybmp-dashboard.json` — a pre-built Grafana dashboard for the Prometheus metrics endpoint (`/api/metrics`).

Panels: BMP messages/sec, route events/sec, peer state timeline (state chart), RPKI validity distribution (pie), governor memory pressure (gauge), top anomaly kinds (bar), convergence event P50/P90/P99 (histogram), DuckDB query latency (heatmap).

Datasource: Prometheus scraping `http://{rbmp-host}:7878/api/metrics` at 15s interval.

This gives operators a one-click "import JSON" path to a production-ready dashboard.

#### RV9-NEW2: BGPsec Full Validation
**Carried from RV7 P2 deferral — elevate to P1 for RV9.**

The type-30 BGPsec_Path attribute is already parsed and raw signature blocks stored in `bgpsec_validations` table. RV9 adds:

1. Router certificate fetch via RPKI repository (RFC 8182 rsync/RRDP) — same infrastructure as the RPKI RTR client in `rbmp-enrichment`
2. ECDSA P-256 signature verification via the `ring` crate
3. `bgpsec_valid` / `bgpsec_invalid` / `bgpsec_not_covered` results stored per route
4. UI badge on route detail: similar to `RpkiBadge` component

```rust
// crates/rbmp-enrichment/src/bgpsec.rs
use ring::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};

pub async fn validate_bgpsec_path(
    signature_blocks: &[u8],
    router_cert: &[u8],
) -> BgpsecVerdict { ... }
```

#### RV9-NEW3: ACL / Prefix-List Generator
`bmppy/rbmppy/acl_generator.py`

Given a set of anomalous prefixes/ASNs from the ML detector, generate router ACL configs to null-route or rate-limit them:

```python
class AclGenerator:
    def generate_prefix_filter(self, prefixes: list[str], action: str = "deny") -> dict[str, str]:
        """
        Returns router-specific ACL/prefix-list configs:
        - IOS-XR: ip prefix-list RUSTYBMP-BLOCK ...
        - FRR: ip prefix-list RUSTYBMP-BLOCK ...
        - JunOS: policy-statement RUSTYBMP-BLOCK ...
        - Arista: ip prefix-list RUSTYBMP-BLOCK ...
        """

    def generate_as_path_filter(self, asns: list[int], action: str = "deny") -> dict[str, str]:
        """Generate AS_PATH access-list to block routes from specific ASNs."""
```

New API endpoint: `POST /api/ml/generate-acl` — takes anomaly IDs, returns vendor-specific configs. UI: "Generate ACL" button on anomaly detail page.

#### RV9-NEW4: OpenTelemetry Distributed Tracing
Trace the BMP processing pipeline end-to-end: TCP receive → parse → RIB update → DuckDB write → SSE push.

```toml
# Cargo.toml additions
opentelemetry       = { version = "0.23" }
opentelemetry-otlp  = { version = "0.16" }
tracing-opentelemetry = { version = "0.24" }
```

Each BMP message gets a trace span. Tags: `bmp.message_type`, `bmp.peer_addr`, `bmp.speaker_addr`, `bmp.prefix` (for route-monitoring). Export to OTLP collector (configurable endpoint, defaults to `localhost:4317`).

Config:
```toml
[telemetry]
otlp_endpoint = "http://localhost:4317"
sample_rate   = 0.01   # 1% sampling at internet scale
```

#### RV9-NEW5: GoBGP + OpenBGPD NOS Coverage
Extend the Tier 0 test lab to include GoBGP and OpenBGPD, completing open-source NOS coverage.

**GoBGP** (`quay.io/osrg/gobgp:latest`) — pure Go BMP speaker, no license required. Excellent for controlled testing because BMP behavior is deterministic and well-documented.

**OpenBGPD** (`openbgpd/openbgpd:latest` or custom Dockerfile on OpenBSD base) — reference implementation, different from FRR parser behavior. Important for edge-case BMP compatibility testing.

Add to `tests/scenarios/03_multi_vendor/topology.clab.yml`. Zero new image cost.

#### RV9-NEW6: YANG Push / gRPC Telemetry Correlation
`crates/rbmp-server/src/telemetry_correlator.rs`

Some operators run both BMP and gRPC telemetry (gNMI/MDT) on IOS-XR and SRL. Correlating these gives the "full picture": BMP shows BGP path changes, gRPC shows the interface stats at the same time.

Architecture: a lightweight gNMI subscription client that subscribes to interface operational data. When a BGP prefix change arrives via BMP, look up the corresponding interface metrics (BW utilization, error counts) at the same timestamp.

New table: `interface_events(occurred_at, speaker_addr, interface, oper_status, bandwidth_util_pct)`.

New API endpoint: `GET /api/correlation/prefix/{prefix}` — returns BGP events + correlated interface events on the same timeline.

#### RV9-NEW7: Policy Recommendation Engine
`bmppy/rbmppy/policy_advisor.py`

Use the filter engine's accept/reject decisions + community semantics learner + RPKI data to suggest Roto filter improvements:

```python
class PolicyAdvisor:
    def analyze_filter_gaps(self, recent_routes: list[RouteCtx]) -> list[PolicySuggestion]:
        """
        Detect routes that:
        1. Are accepted but probably shouldn't be (RPKI not-found + private ASN in path)
        2. Are rejected but probably should be accepted (known-good community, valid RPKI)
        3. Match no explicit rule (fall-through to accept-all)

        Returns: list of suggested filter rule additions with explanation
        """

    def explain_rejection(self, route: RouteCtx) -> str:
        """Why was this route rejected? Map filter verdict to human explanation."""
```

New API endpoint: `GET /api/filters/recommendations` — returns `PolicySuggestion[]` with before/after Roto snippets.

---

## Part 4 — RV9 Priority Matrix

### P0 — Must Do (Testing debt is critical; blocks Codex automation)

| ID | Title | Blocks |
|----|-------|--------|
| RV9-T0 | Unit test target: 200 tests | CI confidence |
| RV9-T1 | `scripts/check_wiring.sh` (Layer 1) | Layer 1 CI |
| RV9-T2 | BMP fixture corpus | Layer 2 |
| RV9-T3 | `POST /api/_test/seed` endpoint | Layer 7 (Playwright) |
| RV9-T4 | Layer 2 protocol tests | Protocol regression |
| RV9-T5 | Layer 3 API contract tests | API regression |
| RV9-T6 | `data-testid` on all pages | Playwright |
| RV9-T7 | FRR smoke lab (Layer 4) | ContainerLab CI |
| RV9-T9 | GitHub Actions CI | Automated testing |
| RV9-ML4 | Convergence event detector | `convergence_events` table has been empty since RV7 |

### P1 — High Value Sprint

| ID | Title |
|----|-------|
| RV9-T8 | Playwright suite (Layer 7) |
| RV9-ML3 | Hijack probability classifier (replace heuristic) |
| RV9-ML1 | Complete `to_pyg()` with RV7 path status features |
| RV9-OUT4 | ServiceNow EM adapter |
| RV9-OUT5 | Webhook adapter (Slack/PagerDuty) |
| RV9-ENR1 | NetBox enricher |
| RV9-EXT2 | IRR/RADB client |
| RV9-UX1 | `/query` NL query page |
| RV9-NEW2 | BGPsec full validation (ring crate) |
| RV9-NEW1 | Grafana dashboard bundle |

### P2 — Planned

| ID | Title |
|----|-------|
| RV9-ML2 | STGNN training script |
| RV9-ML5 | Community semantics learner |
| RV9-UX3 | Communities Explorer page |
| RV9-UX4 | Topology LOD (adaptive rendering) |
| RV9-UX5 | FlowSpec rules viewer |
| RV9-UX6 | Multi-VRF context switcher |
| RV9-OUT6 | `/adapters` management page |
| RV9-ENR2 | ServiceNow CMDB enricher |
| RV9-EXT3 | Looking glass: Cloudflare Radar + HE BGP |
| RV9-EXT4 | RIPE Atlas traceroute |
| RV9-NEW3 | ACL/prefix-list generator |
| RV9-NEW5 | GoBGP + OpenBGPD NOS coverage |

### P3 — Future / RV10

| ID | Title |
|----|-------|
| RV9-NEW4 | OpenTelemetry distributed tracing |
| RV9-NEW6 | YANG push / gRPC telemetry correlation |
| RV9-NEW7 | Policy recommendation engine |
| RV8-MC2 | Standalone NL→SQL REST endpoint |
| RV8-OA3 | Resolve endpoint |
| RV9-T-load | Load testing layers (RIPE RIS, CAIDA, MRT) |

---

## Part 5 — `docs/CODEX_TESTING.md` Specification

This is what RV9 must produce to replace `docs/UBUNTU_TESTING.md`.

**Format contract**: Shell commands only. No prose between commands. Each layer is independently executable. Machine-readable JSON output.

```markdown
# CODEX_TESTING.md — RustyBMP Automated Test Runbook
# Replaces: docs/UBUNTU_TESTING.md (deprecated)
# All commands run from repo root on Ubuntu 24.04.

## Prerequisites (one-time)
apt-get install -y python3 python3-pip duckdb
cargo build --workspace
pip install pytest requests pytest-json-report websockets httpx

## Layer 0 — Rust Unit Tests (<10s)
# Run: always, no dependencies
cargo test --workspace -- --test-output immediate
# Pass: exit 0, all tests green
# Output: printed to stdout by cargo

## Layer 1 — Wiring Checks (<15s)
bash scripts/check_wiring.sh
# Pass: exit 0, "All wiring checks passed"
# Fail: "WIRING CHECK FAILED: N error(s)" + exit 1

## Layer 2 — Protocol Integration (<60s)
# Requires: cargo build --workspace (collector binary)
./target/debug/rbmp-collector --bmp-port 17878 --api-port 17879 --db :memory: --no-auth &
COLLECTOR_PID=$!
sleep 0.5
pytest tests/protocol/ -v --json-report --json-report-file=runtime/test_results/layer2.json
EXIT=$?
kill $COLLECTOR_PID
exit $EXIT
# Pass: exit 0 + layer2.json has "status": "pass"

## Layer 3 — API Contract Tests (<90s)
# Requires: collector binary + seed.sql
duckdb /tmp/rbmp_test.duckdb < tests/seed.sql
./target/debug/rbmp-server --db /tmp/rbmp_test.duckdb --api-port 17880 --no-auth \
  --features test-endpoints &
SERVER_PID=$!
sleep 0.5
pytest tests/api/ -v --json-report --json-report-file=runtime/test_results/layer3.json
EXIT=$?
kill $SERVER_PID
exit $EXIT

## Layer 4 — FRR Smoke Lab (<3min)
# Requires: containerlab, docker, FRR image
docker pull quay.io/frrouting/frr:10.6.1
pytest tests/scenarios/01_frr_minimal/ -v --timeout=180 \
  --json-report --json-report-file=runtime/test_results/layer4.json

## Layer 7 — UI End-to-End (<5min)
# Requires: server running with test-endpoints + UI built
./target/debug/rbmp-server --db /tmp/rbmp_test.duckdb --api-port 7878 \
  --features test-endpoints &
cd ui && npm ci && npm run build && npm run preview &
sleep 2
npx playwright test --reporter=github,json \
  --output=runtime/test_results/layer7.json
```

The key principle: Codex reads `runtime/test_results/<layer>.json`, finds the failing `"name"`, maps it to the test function in `tests/`, reads the assertion message, fixes the code, reruns only that layer's test.

---

## Part 6 — State Summary Table

```
Feature                          RV5  RV6  RV7  RV8  RV9
──────────────────────────────── ───  ───  ───  ───  ───
RFC 7854 BMP core                 ✅   ✅   ✅   ✅   ✅
RFC 9972 Stats (types 0-38)       ✅   ✅   ✅   ✅   ✅
EVPN RFC 7432 (types 1-11)        ✅   ✅   ✅   ✅   ✅
BGP-LS RFC 7752                   ✅   ✅   ✅   ✅   ✅
SR Policy + SRv6 uSID             ✅   ✅   ✅   ✅   ✅
Flowspec RFC 5575 (parse)         ✅   ✅   ✅   ✅   ✅
ASPA RFC 9319 (parse+validate)    ✅   ✅   ✅   ✅   ✅
BGPsec parse (type 30 attr)            ✅   ✅   ✅   ✅
BGPsec ECDSA validation                         ☐    RV9
Path Status TLV draft-05                   ✅   ✅   ✅
Roto JIT filter engine                     ✅   ✅   ✅
RPKI RTR client + VrpCache             ✅   ✅   ✅   ✅
SSH policy fetch                           ✅   ✅   ✅
Resource governor (3 loops)                     ✅   ✅
Convergence event table                    ✅   ✅   ✅
Convergence event detection                         RV9
MCP server (11 tools)                           ✅   ✅
OpenAPI + Swagger UI                            ✅   ✅
Elasticsearch adapter                           ✅   ✅
Splunk HEC adapter                              ✅   ✅
ServiceNow EM adapter                               RV9
Webhook adapter                                     RV9
NetBox enricher                                     RV9
RIPE STAT client                                ✅   ✅
External prefix visibility                      ✅   ✅
IRR/RADB client                                     RV9
Looking glass integrations                          RV9
Adaptive homepage                               ✅   ✅
Speakers summary API                            ✅   ✅
Communities explorer                                RV9
Topology LOD                                        RV9
FlowSpec viewer                                     RV9
Multi-VRF switcher                                  RV9
Grafana dashboard                                   RV9
BGPsec full validation                              RV9
ACL generator                                       RV9
──────────────────────────────────────────────────────
Layer 0 unit tests (count)        20   77  77   90  200
Layer 1 wiring checks              ☐    ☐   ☐    ☐   RV9
Layer 2 protocol tests             ☐    ☐   ☐    ☐   RV9
Layer 3 API contract tests         ☐    ☐   ☐    ☐   RV9
Layer 4 FRR smoke lab              ☐    ☐   ☐    ☐   RV9
GitHub Actions CI                  ☐    ☐   ☐    ☐   RV9
Playwright E2E                     ☐    ☐   ☐    ☐   RV9
```

---

## Part 7 — Upload Pattern

Next diff: `rv9_all_changes.patch`

---

*End of RUSTYBMP_BACKLOG_RV9.md — Sprint RV9*
*Synthesized from: rv8_all_changes.patch · RUSTYBMP_BACKLOG_RV8.md ·
 RUSTYBMP_TESTING_STRATEGY.md · RUSTYBMP_PROJECT_CONTEXT.md*
