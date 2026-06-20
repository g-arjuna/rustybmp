# RustyBMP — Sprint RV8 Backlog
## Swagger · MCP · Output Adapters · Testing · Resource Governor · UX · External APIs

> **Version**: RV8  
> **Date**: 2026-06-20  
> **Basis**: RV7 patch full read · All previous session analysis ·
> Three supplementary documents integrated with proper references:
>
> — **RUSTYBMP_RV8_ANALYSIS.md** (Swagger, MCP 11 tools, output adapters, ML depth, enrichers)  
>   Source: deep read of bonsai src/output/*.rs, src/mcp_server.rs, src/mcp_client.rs,
>   src/http_server/schema.rs, src/http_server/nl_query.rs, src/http_server/ml_jobs.rs,
>   src/enrichment/servicenow.rs, src/enrichment/netbox.rs, src/integrations/servicenow_aiops.rs
>
> — **RUSTYBMP_TESTING_STRATEGY.md** (seven-layer testing pyramid, ContainerLab scenarios,
>   internet-scale free sources, Playwright E2E, Codex-compatible fixtures)  
>   Source: bonsai testing_discipline.md + signal-test-lab topology YAML + soak_test.py +
>   BGPStream v2 documentation + ContainerLab XRd/cEOS/SRL/FRR docs + Playwright 2026 docs
>
> — **Session conversations** (resource governor gap analysis, internet.py/PeeringDB status,
>   looking glass APIs, homepage first-time user UX, Nokia SR-OS BMP limitations)

---

## Part 1 — RV7 Completion Audit

### Delivered in RV7 (from patch analysis)

| File | Feature | Notes |
|------|---------|-------|
| `path_status_tlv.rs` | ✅ Path Status TLV parser | 12 bitmap bits + 11 reason codes. `PATH_STATUS_TLV_TYPE = 6` (Huawei VRP NE8000 default — configurable) |
| `roto_filter.rs` | ✅ Roto filter engine | Feature-gated `roto-jit`; falls through to accept-all when feature absent; `AtomicU64` stats |
| `vault.rs` | ✅ Credential vault | Copied from bonsai credentials.rs; `RUSTYBMP_VAULT_PASSPHRASE`; `ResolvePurpose::SshFetch` |
| `capacity.rs` | ✅ Max-prefix capacity API | `GET/POST /api/capacity/max-prefix`; RFC 9972 type 30 headroom fuel gauge |
| `path_status.rs` | ✅ Path status API | `/api/path-status/matrix` + `/api/path-status/history` |
| `policy_fetch.rs` | ✅ SSH policy fetch handler | Spawns `policy_fetcher.py`; creds as env vars (bonsai pattern) |
| `credentials.rs` | ✅ Credential CRUD API | list / add / delete endpoints |
| `filter_watcher.rs` | ✅ inotify hot-reload | File change → `RotoFilterEngine::reload()` |
| `schema.rs` additions | ✅ Three new tables | `bgpsec_validations`, `convergence_events`, `path_markings` + indexes |
| `writer.rs` | ✅ Path markings writer | Inserts into `path_markings` when `attrs.path_status` present |
| `config/filters.roto` | ✅ Default filter | Bogon + RPKI-invalid + OTC + blackhole; all RouteCtx fields documented |
| `/capacity/+page.svelte` | ✅ Capacity page | Gauge bars, warning/critical state, trend + ETA string |
| `/path-status/+page.svelte` | ✅ Path status page | STATUS_DEFS with icon/label/cls per bit |
| `+layout.svelte` | ✅ Nav updated | Path Status + Capacity added |
| `policy_fetcher.py` | ✅ SSH fetcher | Genie testbed + paramiko; RUSTYBMP_SSH_USERNAME/PASSWORD env vars |
| `bmppy/rbmppy/policy/ast.py` | ✅ Policy AST | PolicyAst, RouteMap, RouteTerm, MatchCondition, SetAction |
| `bmppy/rbmppy/policy/parsers.py` | ✅ Parser chain | Genie → TextFSM → regex heuristic fallback |

### What RV7 explicitly deferred

| Item | Why deferred | RV8 action |
|------|-------------|-----------|
| BGPsec ECDSA validation | P2, parse already done, crypto needs ring crate | RV8-P1 |
| Convergence event detection (Python) | Table created, no detection logic | RV8-P2 |
| BGP-LS topology LOD (hierarchical/clustered) | Low priority vs BMP features | RV8-UI5 |
| Batfish Tier 2 parser | Optional enrichment | RV8-V8 |

---

## Part 2 — Resource Governor Assessment

**The gap**: Bonsai has a production resource governor (`resource_governor.rs`, fully read).
rustybmp has the basic batched-write pressure concept but lacks the three-loop governor.

### Bonsai's governor (source: `src/resource_governor.rs`, full read)

Three independent background loops:

**Loop 1 — Memory pressure** (5s poll):
- Soft threshold: 80% of `memory_budget_bytes` → shrinks LRU caches, triggers early DuckDB flush
- Hard threshold: 95% → aggressive response, signals ingest to shed low-priority messages
- Callback: registered by server_startup, invoked with shrink percentage (50 for soft, 25 for hard)

**Loop 2 — Write pressure** (write_coordinator queue_pct):
- When >50% queue fill sustained for 60s → expands batch size to reduce transaction overhead
- `write_batch_expand_count` Prometheus counter when triggered

**Loop 3 — Rate governance** (inbound events/sec):
- Measures aggregate events/second via `inbound_event_counter` AtomicU64
- When rate budget exceeded → `rate_shedding_active = true`
- Ingest paths call `should_shed()` — drop low-priority BMP stats/counter noise
- `rate_shed_count` Prometheus counter

### Why this matters for internet-scale testing

When injecting a full RouteViews MRT dump (~900K IPv4 + 190K IPv6 prefixes), rustybmp will:
1. Receive ~1.1M route-monitoring PDUs in rapid succession
2. Try to write all of them to DuckDB (batched, but still very high throughput)
3. Without rate governance: DuckDB write queue fills → tokio channel blocks → TCP backpressure → BMP sender stalls

The governor's `should_shed()` path lets the system gracefully degrade by dropping BMP stats counter updates (less critical) while preserving route-monitoring events (critical).

### RV8 Resource Governor Implementation

**New file**: `crates/rbmp-server/src/resource_governor.rs`

Port directly from bonsai with these rustybmp-specific changes:

1. **Budget configuration** (in `rustybmp.toml`):
```toml
[governor]
# Memory budget in MB. Default: 80% of available RAM, min 2GB.
memory_budget_mb = 12000   # 12GB for 32GB Ubuntu machine at lab
rate_budget_eps  = 500000  # 500K events/sec (internet table injection rate)

[governor.shedding]
# BMP message types that can be shed under pressure
# "stats" = BMP Stats Reports (RFC 7854 §4.8) — least critical
# "bgpls" = BGP-LS updates (topology, not routing state)
sheddable_types = ["stats", "bgpls_prefix"]
```

2. **Ingest path integration** in `crates/rbmp-server/src/bin/collector.rs`:
```rust
// Before dispatching each BMP PDU to the pipeline:
if governor.should_shed() {
    // Only shed non-critical message types
    if let BmpPayload::Stats(_) = &msg.payload {
        governor_shed_counter.increment(1);
        continue;  // drop this stats report
    }
}
governor.record_event();
```

3. **API exposure**: `GET /api/governance` → returns `GovernanceSnapshot`:
```json
{
  "profile": "internet_scale",
  "memory_budget_mb": 12000,
  "rate_budget_eps": 500000,
  "memory_pressure_active": false,
  "write_pressure_active": true,
  "rate_shedding_active": false,
  "memory_shrink_count": 0,
  "write_batch_expand_count": 3,
  "rate_shed_count": 0
}
```

4. **Prometheus metrics**: `rustybmp_governor_action_total{action="memory_shrink"|"write_expand"|"rate_shed"}`

### DuckDB write queue sizing for internet scale

Current default batch: `BATCH_SIZE=500, BATCH_TIMEOUT=50ms`.

For internet table injection (1.1M routes in ~30s = ~36K routes/sec):
```toml
[store]
batch_size    = 5000   # increase from 500 to 5000 for bulk injection
batch_timeout_ms = 200  # increase from 50ms to 200ms — fewer transactions
write_queue_capacity = 100000  # increase channel depth
```

These are configurable. Add to the capacity section of `rustybmp.toml.example`.

---

## Part 3 — XRd + FRR Testing Scenarios (Nokia SR-OS excluded)

**Decision context**: Nokia SR-OS had BMP connection issues in previous testing. XRd image is already imported and working on the Ubuntu ContainerLab. Inotify settings already applied. BMP testing focuses on XRd + FRR only.

### Scenario 01: FRR BMP Smoke (Tier 0, ~30s)

Uses `quay.io/frrouting/frr:10.6.1` (free, no account required).

```yaml
# tests/scenarios/01_frr_minimal/topology.clab.yml
name: rustybmp-frr-bmp-smoke

topology:
  nodes:
    rustybmp:
      kind: linux
      image: ubuntu:24.04
      binds:
        - ../../../target/debug/rbmp-collector:/usr/local/bin/rbmp-collector:ro
        - configs/rustybmp.toml:/etc/rustybmp/rustybmp.toml:ro
      exec:
        - "rbmp-collector --config /etc/rustybmp/rustybmp.toml --no-auth &"

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
        - configs/frr-pe2/daemons:/etc/frr/daemons
        - configs/frr-pe2/frr.conf:/etc/frr/frr.conf

  links:
    - endpoints: ["frr-pe1:eth1", "frr-pe2:eth1"]
    - endpoints: ["frr-pe1:eth2", "rustybmp:eth1"]
```

**FRR BMP config** (critical section — must be in startup config, not post-boot):
```
# configs/frr-pe1/frr.conf
router bgp 65001
 bgp router-id 10.0.0.1
 neighbor 10.0.0.2 remote-as 65002
 bmp targets rustybmp
  bmp connect 172.20.20.1 port 11019 min-retry 1000 max-retry 5000
  bmp monitor ipv4 unicast pre-policy
  bmp monitor ipv4 unicast post-policy
  bmp monitor ipv4 unicast loc-rib
 exit
 address-family ipv4 unicast
  network 203.0.113.0/24
  network 203.0.114.0/24
  network 203.0.115.0/24
  neighbor 10.0.0.2 activate
 exit-address-family
```

**Tests** (`tests/scenarios/01_frr_minimal/test_frr_bmp_smoke.py`):
- BMP PeerUp received within 60s
- Pre-policy RIB and post-policy RIB both populated
- 3 IPv4 routes visible in `/api/routes`
- `/api/filters/test` with bogon prefix → reject verdict
- SSE stream connects and delivers events

### Scenario 02: XRd BMP RFC 9972 Validation (Tier 1, ~3min)

The most important scenario — validates that XRd's BMP implementation satisfies RFC 9972.

```yaml
# tests/scenarios/02_xrd_rfc9972/topology.clab.yml
name: rustybmp-xrd-rfc9972

topology:
  nodes:
    rustybmp:
      kind: linux
      image: ubuntu:24.04
      # inotify limits already set on host — no need to add them per-container

    xrd-pe1:
      kind: cisco_xrd
      image: ios-xr/xrd-control-plane:24.4.1
      mgmt-ipv4: 172.20.20.20
      startup-config: configs/xrd-pe1.cfg
      # Note: XRd only supports control-plane flavor for vlink topologies

    xrd-pe2:
      kind: cisco_xrd
      image: ios-xr/xrd-control-plane:24.4.1
      mgmt-ipv4: 172.20.20.21
      startup-config: configs/xrd-pe2.cfg

  links:
    - endpoints: ["xrd-pe1:Gi0/0/0/0", "xrd-pe2:Gi0/0/0/0"]
    - endpoints: ["xrd-pe1:Gi0/0/0/1", "rustybmp:eth1"]
```

**XRd BMP config** (inside startup-config, avoids post-boot SSH issues):
```
# configs/xrd-pe1.cfg
!
bmp server 1
 host 172.20.20.1 port 11019
 flapping-delay 30
!
router bgp 65100
 bgp router-id 10.100.0.1
 bmp servers 1
  initial-delay 5
  stats-reporting-period 30
 !
 neighbor 10.100.0.2
  remote-as 65200
  bmp-activate server 1
  address-family ipv4 unicast
   maximum-prefix 1000 90  !-- 90% threshold triggers warning
  !
 !
 address-family ipv4 unicast
  maximum-prefix 1000
!
```

**Tests** (`tests/scenarios/02_xrd_rfc9972/test_xrd_rfc9972.py`):
```python
class TestXrdRfc9972:
    def test_stats_type_30_received(self, clab_xrd):
        """RFC 9972 type 30 = routes left before max-prefix limit."""
        stats = get_stats_by_type(API_BASE, counter_type=30)
        assert len(stats) > 0, "RFC 9972 type 30 (max-prefix headroom) not in stats_events"
        # headroom should be < 1000 (configured limit) and > 0
        assert 0 < stats[0]["counter_value"] <= 1000

    def test_stats_include_afi_safi(self, clab_xrd):
        """RFC 9972 stats must carry AFI/SAFI breakdown."""
        stats = get_all_stats(API_BASE)
        with_afi = [s for s in stats if s.get("afi")]
        assert len(with_afi) > 0, "RFC 9972 stats missing AFI/SAFI"

    def test_bmp_peer_up_with_capabilities(self, clab_xrd):
        """XRd PeerUp must include BGP capability negotiation."""
        caps = api_get(f"{API_BASE}/api/peers/{XRD_PE1_PEER}/capabilities")
        assert caps.get("add_path") is not None or caps.get("graceful_restart") is not None

    def test_path_status_tlv_received(self, clab_xrd):
        """Verify Path Status TLV parsed from XRd route-monitoring."""
        # XRd 24.4.1+ supports Path Status TLV on BMP local-RIB
        matrix = api_get(f"{API_BASE}/api/path-status/matrix?limit=10")
        if matrix.get("count", 0) > 0:
            row = matrix["rows"][0]
            assert "status_label" in row, "path_status.status_label missing"
            assert "reason_label" in row, "path_status.reason_label missing"
        # If no rows: TLV not enabled on this XRd version — skip rather than fail
        else:
            pytest.skip("XRd version does not emit Path Status TLV (needs 24.4+)")

    def test_capacity_fuel_gauge(self, clab_xrd):
        """Max-prefix capacity endpoint reflects RFC 9972 type 30."""
        cap = api_get(f"{API_BASE}/api/capacity/max-prefix")
        rows = cap.get("rows", [])
        assert len(rows) > 0, "No capacity rows from XRd"
        peer_row = next((r for r in rows if r.get("peer_addr") == XRD_PEER_ADDR), None)
        assert peer_row is not None
        assert peer_row["configured_limit"] <= 1000  # what we configured
        assert peer_row["used_pct"] < 100.0          # not at limit
```

### Scenario 03: XRd + FRR BGP Anomaly Injection

Combined XRd + FRR topology where FRR injects anomalous routes and XRd relays via BMP.

**Purpose**: Test hijack detector, route leak detector, and RPKI filter with real BMP stream.

**Fault patterns**:
1. FRR advertises `203.0.113.0/25` (subprefix, more-specific than existing /24 ROA) → RPKI-invalid, should trigger filter and anomaly
2. FRR advertises prefix with private ASN `64512` in path → RouteLeakDetector should fire
3. FRR rapidly withdraws and re-announces prefix 3 times in 10s → FlapScorer should fire

### Scenario 04: Internet-Scale Load (MRT replay)

Uses `rbmp-mrt` binary to inject RouteViews MRT data directly:

```bash
# Download latest RouteViews update file (~50MB compressed)
python3 tests/load/mrt_replay.py \
    --collector route-views2 \
    --type updates \
    --bmp-host 127.0.0.1 \
    --bmp-port 11019 \
    --measure-throughput

# Expected output:
# {"driver": "mrt_replay", "messages": 45231, "elapsed_s": 42.1,
#  "throughput_mps": 1074, "routes_in_db": 44892,
#  "governor_sheds": 0, "write_pressure_active": false}
```

**Throughput assertion**: `throughput_mps >= 1000` (the cargo bench baseline of 1M msgs/sec, degraded by DuckDB writes).

---

## Part 4 — Internet-Scale Free Data Sources

*Reference: RUSTYBMP_TESTING_STRATEGY.md §Part 7*

### Source 1: RIPE RIS Live (WebSocket, free, real-time)

```python
# tests/load/ripe_ris_bridge.py
# WebSocket: wss://ris-live.ripe.net/v1/ws/
# No registration, no cost. 600+ BGP peers globally.
# Subscribe: {"type": "ris_subscribe", "data": {"type": "UPDATE"}}
```

Use case: Run for 15 minutes and measure rustybmp's ability to ingest
continuous production internet BGP update traffic without degradation.

**Pass criterion**: Governor stays stable (`rate_shedding_active = false`) for the
first 10 minutes of a 15-minute run at normal RIS Live message rate (~100-500 msgs/sec).

### Source 2: CAIDA BGPStream BMP Kafka (raw BMP, free)

BGPStream v2 provides a publicly-accessible, read-only Kafka cluster at
`bmp.bgpstream.caida.org:9092` containing raw BMP data from multiple collectors.

```python
# tests/load/caida_bmp_relay.py
# Kafka bootstrap: bmp.bgpstream.caida.org:9092
# Topic: openbmp.parsed.router (project: caida-bmp)
# No conversion needed — actual BMP protocol frames
```

This is the most authentic source: real router BMP sessions, real path attributes, real
timing. Ideal for testing the governor's behaviour under real production traffic patterns.

### Source 3: RouteViews MRT (full internet table, ~1.1M prefixes)

Download latest RIB dump from `http://archive.routeviews.org/bgpdata/` and inject via
`rbmp-mrt`. The `mrt-inject` binary already exists from RV3. Full table injection in one
shot — the best stress test for DuckDB write throughput and memory pressure loop.

---

## Part 5 — Swagger / OpenAPI

*Reference: RUSTYBMP_RV8_ANALYSIS.md §Part 11*

**Source pattern**: bonsai `src/http_server/schema.rs` — full OpenAPI 3.0.3 spec with 17
tag groups, inline JSON examples from fixture files, served via Swagger UI at `/api/swagger`.

### RV8-OA1: `api/schema.rs` — OpenAPI 3.0.3 specification

Tag groups mapped to rustybmp's API surface:

| Tag | Key endpoints |
|-----|--------------|
| Routing | /api/routes, /api/routes/prefix/{p}/*, /api/prefixes |
| Peers | /api/peers, /api/peers/{addr}/capabilities |
| RPKI | /api/rpki/analysis, /api/rpki/coverage |
| Topology | /api/bgpls/graph, /api/bgpls/path, /api/srpolicy |
| Policy | /api/policy, /api/filters/*, /api/policy/fetch, /api/policy/configs |
| Analytics | /api/aspath/graph, /api/capacity/max-prefix, /api/bmpstats/history |
| Path Status | /api/path-status/matrix, /api/path-status/history (RV7 ✅) |
| Convergence | /api/convergence |
| ML | /api/ml/anomalies, /api/ml/model/status |
| Credentials | /api/credentials (RV7 ✅) |
| Onboarding | /api/onboard/* |
| Output Adapters | /api/adapters/*, /api/adapters/{name}/test |
| MCP | /mcp |
| SSE | /api/events |
| Operations | /health, /api/metrics, /api/governance |

### RV8-OA2: Swagger UI served inline

```rust
// crates/rbmp-server/src/api/schema.rs
pub async fn swagger_ui_handler() -> Html<&'static str> {
    Html(r##"<!DOCTYPE html>
<html lang="en"><head>
  <meta charset="utf-8"/><title>RustyBMP API</title>
  <link rel="stylesheet"
    href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
  <style>body{margin:0}.topbar{display:none}</style>
</head><body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>
window.onload = () => SwaggerUIBundle({
  url: "/api/openapi.json", dom_id: "#swagger-ui",
  presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
  layout: "BaseLayout", deepLinking: true, tryItOutEnabled: true,
  filter: true, docExpansion: "none", defaultModelsExpandDepth: 2,
});
</script></body></html>"##)
}
```

Routes: `GET /api/openapi.json`, `GET /api/swagger`, `GET /api/resolve?q=X`

---

## Part 6 — MCP Server (11 BGP Tools)

*Reference: RUSTYBMP_RV8_ANALYSIS.md §Part 10*

**Why this is the most important RV8 addition**: ThousandEyes announced MCP at Cisco Live
2026 Amsterdam for their cloud product. rustybmp's MCP would be the only BMP/BGP-native
MCP server for on-premises networks. AI agents (Claude, Cursor, GPT-4) can point directly
at a BGP collector and ask questions in plain English.

### Tool definitions (11 tools)

```rust
// crates/rbmp-server/src/mcp_server.rs
// POST /mcp — JSON-RPC 2.0

pub static BGP_TOOLS: &[McpTool] = &[
    McpTool { name: "get_prefix_status",
      description: "Get BGP routing status for a prefix: path states per peer (best/ECMP/backup/filtered/invalid), RPKI, AS path, convergence time.",
      parameters: &[Param { name: "prefix", required: true }, Param { name: "afi", required: false }] },
    McpTool { name: "get_peer_health",
      description: "Get peer session health: state, route counts, flap history, RPKI invalid rate, max-prefix utilization.",
      parameters: &[Param { name: "peer_addr", required: true }, Param { name: "speaker_addr", required: false }] },
    McpTool { name: "query_route_events",
      description: "Query recent BGP route changes. Filter by prefix, peer AS, speaker, action, RPKI state.",
      parameters: &[Param { name: "prefix", required: false }, Param { name: "peer_as", required: false },
                    Param { name: "rpki", required: false }, Param { name: "since_hours", required: false },
                    Param { name: "limit", required: false }] },
    McpTool { name: "get_anomalies",
      description: "Get recent ML anomaly detections: hijack, route leak, flap, origin change, RPKI violation with confidence scores.",
      parameters: &[Param { name: "kind", required: false }, Param { name: "severity", required: false },
                    Param { name: "prefix", required: false }, Param { name: "since_hours", required: false }] },
    McpTool { name: "get_rpki_status",
      description: "Get RPKI validation for a prefix: validity, ROA details, max-length, per-peer validity states.",
      parameters: &[Param { name: "prefix", required: true }] },
    McpTool { name: "get_topology",
      description: "Get BGP-LS network topology: routers, links, IGP metrics, SR adjacency SIDs.",
      parameters: &[Param { name: "protocol", required: false }, Param { name: "max_nodes", required: false }] },
    McpTool { name: "compute_igp_path",
      description: "Compute shortest IGP path between two routers using BGP-LS topology data.",
      parameters: &[Param { name: "src_router_id", required: true }, Param { name: "dst_router_id", required: true }] },
    McpTool { name: "test_filter",
      description: "Test a Roto filter rule against a synthetic BGP route. Returns accept/reject with evaluation time.",
      parameters: &[Param { name: "prefix", required: true }, Param { name: "rpki", required: false },
                    Param { name: "peer_as", required: false }, Param { name: "communities", required: false }] },
    McpTool { name: "get_capacity_status",
      description: "Get max-prefix capacity per peer/AFI-SAFI: utilization, growth trend, ETA to limit (RFC 9972 type 30).",
      parameters: &[Param { name: "peer_addr", required: false }, Param { name: "min_pct", required: false }] },
    McpTool { name: "natural_language_query",
      description: "Convert plain English BGP question to DuckDB SQL and execute. Examples: 'Show RPKI invalid routes today', 'Which peers have >10000 routes?'",
      parameters: &[Param { name: "question", required: true }] },
    McpTool { name: "get_convergence_events",
      description: "Get BGP convergence events: duration from peer-down to EOR, affected prefix count.",
      parameters: &[Param { name: "peer_addr", required: false }, Param { name: "since_hours", required: false }] },
];
```

### Natural language → DuckDB SQL

The LLM receives a compact DuckDB schema description and returns only SQL. Daily token budget: 500K (AtomicU64, resets midnight UTC — identical to bonsai's pattern from nl_query.rs).

DuckDB schema context injected as system prompt:
```
Tables: route_events(occurred_at, speaker_addr, peer_addr, peer_as, rib_type,
action, prefix, afi, as_path, origin_asn, rpki_validity, local_pref, med,
communities), peer_events(occurred_at, speaker_addr, peer_addr, event_type),
ml_anomalies(detected_at, kind, prefix, score, severity),
path_markings(occurred_at, prefix, peer_addr, path_status, status_label, reason_label),
stats_events(occurred_at, peer_addr, counter_type, counter_value, afi, safi),
convergence_events(started_at, convergence_ms, peer_addr, affected_prefixes)
Return ONLY the SQL, no explanation. Use LIMIT. SELECT only.
```

### ANOMALY_CATALOGUE (equivalent to bonsai's RULE_CATALOGUE)

Per anomaly kind, DuckDB verification queries for AI agents to check whether the issue resolved:

```rust
pub static ANOMALY_CATALOGUE: &[AnomalyMeta] = &[
    AnomalyMeta {
        kind: "origin_change",
        description: "BGP prefix hijack — origin ASN changed to unexpected ASN",
        severity: "critical",
        verification_queries: &[
            "SELECT prefix, origin_asn, occurred_at FROM route_events WHERE prefix = $prefix ORDER BY occurred_at DESC LIMIT 5 — verify origin_asn returned to expected value",
            "SELECT result FROM aspa_validations WHERE prefix = $prefix ORDER BY occurred_at DESC LIMIT 1 — ASPA should be 'valid' when resolved",
        ],
    },
    AnomalyMeta {
        kind: "route_leak",
        description: "Route leak — private or unexpected ASN in AS path",
        severity: "critical",
        verification_queries: &[
            "SELECT as_path FROM route_events WHERE peer_addr = $peer AND occurred_at > NOW() - INTERVAL '5 minutes' ORDER BY occurred_at DESC LIMIT 3",
        ],
    },
    AnomalyMeta {
        kind: "slow_convergence",
        description: "BGP convergence took longer than historical baseline",
        severity: "warn",
        verification_queries: &[
            "SELECT convergence_ms, affected_prefixes FROM convergence_events WHERE peer_addr = $peer ORDER BY started_at DESC LIMIT 5 — compare with baseline",
        ],
    },
];
```

---

## Part 7 — Output Adapters (from bonsai)

*Reference: RUSTYBMP_RV8_ANALYSIS.md §Parts 3-7*
*Source code: bonsai src/output/elastic.rs, splunk_hec.rs, servicenow_em.rs, traits.rs*

### OutputAdapter trait (copy from bonsai traits.rs, adapted)

```rust
// crates/rbmp-server/src/output/traits.rs

pub enum OutputTopic {
    RouteEvents, PeerEvents, AnomalyDetections,
    PathStatusEvents, RpkiEvents, AllEvents,
}

#[async_trait]
pub trait OutputAdapter: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn topics(&self) -> &[OutputTopic];
    fn speaker_scope(&self) -> &[String];   // empty = all speakers
    async fn run(&self, creds: Arc<CredentialVault>, audit: OutputAdapterAuditLog,
                 shutdown: watch::Receiver<bool>) -> anyhow::Result<()>;
    async fn test_connection(&self, creds: Arc<CredentialVault>) -> anyhow::Result<()>;
}
```

**Shared patterns from bonsai** (copy verbatim):
- Cursor persistence: `runtime/cursors/{name}.cursor` — survives restarts, prevents re-push
- In-memory dedup: `HashMap<(speaker_addr, peer_addr, anomaly_kind), last_pushed_ns>`
- Dedup window: configurable `dedup_window_secs` (default 300)
- Audit log: every push cycle writes to `OutputAdapterAuditLog`

### Adapters

**RV8-OUT2: Elasticsearch** — ECS-compliant BGP events via `_bulk` ndjson API.
Auth: basic or API key from vault. Index: `rustybmp-bgp-events`. ECS fields:
`@timestamp`, `event.kind="alert"|"event"`, `event.category=["network"]`,
`host.ip=[speaker_addr]`, `source.as.number=peer_as`, `rustybmp.*` namespace for BGP fields.

**RV8-OUT3: Splunk HEC** — `POST /services/collector/event`. Token from vault
(`password` field). Sourcetype: `rustybmp:bgp`. Index configurable.

**RV8-OUT4: ServiceNow EM** — `POST /api/now/table/em_event`. Same endpoint as bonsai.
Severity mapping: critical→1, warn→3. `message_key` = `speaker:peer:anomaly_kind` for
ServiceNow dedup.

**RV8-OUT5: Webhook** — Slack/PagerDuty/OpsGenie/Teams profiles via Handlebars body
templates. Not in bonsai — new addition for rustybmp. Profiles include severity-to-emoji
mapping, routing_key resolution (PagerDuty), and `deduplicate_by` field list.

---

## Part 8 — External API Integrations

*Reference: bmppy/rbmppy/internet.py (designed in RV3, needs verification and extension)*

### 8.1 PeeringDB (existing in internet.py)

From the RV3 backlog design, `InternetIntelligenceClient` should have:

```python
# bmppy/rbmppy/internet.py
class InternetIntelligenceClient:
    async def asn_info(self, asn: int) -> AsnInfo:
        """PeeringDB ASN lookup with 24h TTL cache.
        Returns: name, country, networks, IX presence list.
        Endpoint: https://www.peeringdb.com/api/net?asn={asn}"""

    async def prefix_info(self, prefix: str) -> PrefixInfo:
        """RIPE STAT prefix overview with 1h cache.
        Endpoints:
          https://stat.ripe.net/data/prefix-overview/data.json?resource={prefix}
          https://stat.ripe.net/data/announced-prefixes/data.json?resource={asn}"""

    async def bulk_asn_info(self, asns: list[int]) -> dict[int, AsnInfo]:
        """Async parallel fetcher for multiple ASNs."""

    async def ix_presence(self, asn: int) -> list[IxInfo]:
        """PeeringDB netixlan API — which IXPs does this ASN peer at?
        Endpoint: https://www.peeringdb.com/api/netixlan?asn={asn}"""
```

**Verify this is implemented and test against production PeeringDB API.**

### 8.2 RIPE STAT — Comprehensive BGP analytics

RIPE STAT has multiple BGP-specific endpoints relevant to rustybmp:

```python
# bmppy/rbmppy/ripe_stat.py  (NEW — extend internet.py or separate module)

RIPE_STAT_BASE = "https://stat.ripe.net/data"

class RipeStatClient:
    async def bgp_state(self, prefix: str) -> dict:
        """Current BGP routing table state for a prefix from all RIS peers.
        GET /bgp-state/data.json?resource={prefix}
        Use: cross-reference with internal BMP observation.
        'Is our local view consistent with what RIPE sees from the internet?'"""

    async def announced_prefixes(self, asn: int) -> list[str]:
        """All prefixes announced by an ASN (per RIS observations).
        GET /announced-prefixes/data.json?resource={asn}
        Use: validate that our BMP pre-policy data matches external visibility."""

    async def prefix_overview(self, prefix: str) -> dict:
        """Ownership, abuse contact, RIR, RPKI status.
        GET /prefix-overview/data.json?resource={prefix}"""

    async def bgp_updates(self, prefix: str, hours: int = 24) -> list[dict]:
        """Recent BGP updates for a prefix from RIS vantage points.
        GET /bgp-updates/data.json?resource={prefix}&starttime={t}
        Use: correlate internal BMP observations with external BGP events."""

    async def routing_history(self, prefix: str, days: int = 7) -> list[dict]:
        """Historical BGP routing status — was this prefix reachable a week ago?
        GET /routing-history/data.json?resource={prefix}"""

    async def ris_peers(self) -> list[dict]:
        """All RIS route collector peers and their coverage.
        GET /ris-peers/data.json
        Use: understand the vantage point coverage for internet-scale testing."""
```

**New API endpoint**: `GET /api/external/prefix-visibility?prefix=203.0.113.0/24`
Returns side-by-side: internal BMP observation + RIPE STAT external visibility.
This is the "are we the only ones who see this route?" answer.

### 8.3 IRR/RADB Route Object Validation

IRR (Internet Routing Registry) stores authoritative route objects. Cross-referencing
BMP announcements against IRR is the complement to RPKI:

```python
# bmppy/rbmppy/irr_client.py  (NEW)

class IrrClient:
    """Query IRR databases via whois (TCP port 43) for route object validation.

    Databases: RIPE, ARIN, APNIC, LACNIC, AFRINIC, RADB, NTTCOM
    Route object: {prefix} owned by {origin-AS} according to IRR?
    """

    async def validate_route(self, prefix: str, origin_as: int) -> IrrResult:
        """Check if a BGP announcement matches an IRR route object.
        Returns: match (in IRR), no_match (not in IRR), unknown (query failed)"""

    async def get_as_set(self, as_set_name: str) -> list[int]:
        """Expand an AS-SET to constituent ASNs.
        Example: AS-CLOUDFLARE → [13335, 209242, ...]"""

    async def get_route_objects(self, asn: int) -> list[str]:
        """All route objects registered for an ASN across IRR databases."""
```

### 8.4 Looking Glass Integrations

Looking glasses provide external vantage point data. Three levels of integration:

**Level 1: RIPE Atlas** (API-based, most comprehensive)
```python
# bmppy/rbmppy/looking_glass.py

RIPE_ATLAS_BASE = "https://atlas.ripe.net/api/v2"

class RipeAtlasClient:
    def __init__(self, api_key: str = ""):
        self.api_key = api_key  # Optional; anonymous access limited

    async def create_traceroute(self, prefix: str, probe_count: int = 10) -> str:
        """Create a traceroute measurement to the BGP next-hop of a prefix.
        Returns: measurement_id (string)
        Endpoint: POST /measurements/"""

    async def get_measurement_results(self, meas_id: str) -> list[dict]:
        """Poll measurement results.
        Endpoint: GET /measurements/{meas_id}/results/"""

    async def get_prefix_routing(self, prefix: str) -> dict:
        """What do RIPE Atlas probes see for this prefix?
        Uses Atlas anchors as looking glass vantage points."""
```

**Level 2: Public BGP Looking Glasses (HTTP scraping)**

Many ISPs and IXPs run public looking glasses with no API. BGPalerter and similar tools
scrape these. For rustybmp, a lightweight approach:

```python
KNOWN_LOOKING_GLASSES = {
    "hurricane_electric":  "https://bgp.he.net/api/",
    "cloudflare_radar":    "https://radar.cloudflare.com/api/v4/bgp/",
    "pch_lg":              "https://lg.pch.net/api/",
    "nlnog_ring":          None,  # SSH-only, no REST
}

async def cloudflare_radar_prefix(prefix: str) -> dict:
    """Cloudflare Radar BGP data: route leak detection, prefix visibility.
    GET https://radar.cloudflare.com/api/v4/bgp/routes?prefix={prefix}"""

async def he_bgp_prefix(prefix: str) -> dict:
    """Hurricane Electric BGP prefix info.
    GET https://bgp.he.net/api/5/{prefix}"""
```

**Level 3: rustybmp as a looking glass**

The MCP tool `get_prefix_status` already makes rustybmp a looking glass from the internal
vantage point. The combined view (internal BMP + external RIPE STAT + HE BGP) gives
operators "what we see" vs "what the internet sees":

```
GET /api/external/prefix-visibility?prefix=203.0.113.0/24

{
  "prefix": "203.0.113.0/24",
  "internal": {
    "peers_announcing": 3,
    "best_path_peer": "10.0.0.1",
    "rpki": "valid",
    "as_path": "65001 64496",
    "path_status": "best"
  },
  "external": {
    "ripe_ris_peers_visible": 127,
    "cloudflare_radar_visible": true,
    "he_bgp_origin": "AS64496",
    "irr_route_object": "valid"
  },
  "discrepancies": []
}
```

### 8.5 API Integration Matrix

| Source | Endpoint | Auth | Rate limit | TTL cache | Use in rustybmp |
|--------|----------|------|-----------|----------|-----------------|
| PeeringDB | `api.peeringdb.com/api/net` | None | 100/5min | 24h | ASN info, IX presence |
| RIPE STAT | `stat.ripe.net/data/` | None | Generous | 1h | BGP state, prefix visibility |
| RIPE RIS Live | `ris-live.ripe.net/v1/ws/` | None | None | N/A | Internet-scale testing |
| RouteViews MRT | `archive.routeviews.org` | None | None | N/A | Full table injection |
| CAIDA BGPStream | `bmp.bgpstream.caida.org:9092` | None | None | N/A | Raw BMP relay |
| ARIN RDAP | `rdap.arin.net/registry/ip/` | None | Generous | 24h | IP ownership |
| Cloudflare Radar | `radar.cloudflare.com/api/v4` | API key | 10K/day | 1h | Route leak detection |
| Hurricane Electric | `bgp.he.net/api/` | None | Limited | 1h | External AS/prefix lookup |
| RIPE Atlas | `atlas.ripe.net/api/v2` | Optional | 100/1h anon | N/A | External traceroutes |

---

## Part 9 — Homepage / First-Time User Experience

**The problem**: A new rustybmp deployment has zero BMP sessions. The current dashboard
shows empty metric cards — it looks broken. Operators don't know what to do next.

### 9.1 Adaptive homepage state machine

The homepage (`/`) detects its state and renders accordingly:

```
State A (no speakers, no data): ONBOARDING
State B (speakers configured, no BMP sessions yet): WAITING
State C (BMP sessions active): FULL DASHBOARD
```

### 9.2 State A: Onboarding mode

When `speakers.length === 0` and `totalRoutes === 0`, show a full-screen onboarding flow
instead of the dashboard:

```svelte
<!-- ui/src/routes/+page.svelte — empty state -->
{#if isEmpty}
  <div data-testid="onboarding-empty-state"
       class="flex flex-col items-center justify-center min-h-[80vh] gap-8">
    <div class="text-center max-w-xl">
      <h1 class="text-2xl font-medium mb-2">Welcome to RustyBMP</h1>
      <p class="text-secondary">
        No BMP sessions yet. Configure your router to send BMP data here.
      </p>
    </div>

    <!-- Inline config generator for the most common router types -->
    <div data-testid="quick-config-panel" class="w-full max-w-2xl">
      <div class="flex gap-2 mb-4">
        {#each ['IOS-XR', 'FRRouting', 'Arista EOS', 'JunOS'] as vendor}
          <button data-testid="vendor-tab-{vendor}"
                  class:active={activeVendor === vendor}
                  on:click={() => activeVendor = vendor}>
            {vendor}
          </button>
        {/each}
      </div>

      <!-- Config snippet that operators can paste directly into their router -->
      <pre data-testid="router-config-snippet" class="font-mono text-sm bg-secondary p-4 rounded-lg">
{configSnippets[activeVendor]}
      </pre>
      <button on:click={() => navigator.clipboard.writeText(configSnippets[activeVendor])}>
        Copy to clipboard
      </button>
    </div>

    <p class="text-tertiary text-sm">
      BMP collector listening on <strong>{bmpHost}:{bmpPort}</strong>
    </p>
  </div>
{/if}
```

Config snippets per vendor (generated from server address):
```javascript
const configSnippets = {
  'IOS-XR': `bmp server 1
 host ${serverAddr} port ${bmpPort}
!
router bgp 65001
 bmp servers 1
  initial-delay 5
 !
 neighbor <PEER_IP>
  remote-as <PEER_AS>
  bmp-activate server 1
 !`,
  'FRRouting': `router bgp 65001
 bmp targets rustybmp
  bmp connect ${serverAddr} port ${bmpPort} min-retry 1000
  bmp monitor ipv4 unicast pre-policy
  bmp monitor ipv4 unicast post-policy
 exit
!`,
  // ... JunOS, EOS ...
};
```

### 9.3 State B: Waiting for BMP sessions

Speakers are configured in `rustybmp.toml` but no BMP sessions have connected yet.
Show a live status panel:

```svelte
{:else if hasSpeakers && !hasActiveSessions}
  <div data-testid="waiting-state">
    <h2>Waiting for BMP sessions</h2>
    <p>Configured speakers:</p>
    {#each speakers as speaker}
      <div data-testid="speaker-waiting-{speaker.addr}" class="speaker-card">
        <span class="speaker-addr">{speaker.addr}</span>
        <span class="status-badge waiting">⏳ No BMP session</span>
        <details>
          <summary>Troubleshoot</summary>
          <ul>
            <li>Verify TCP port {bmpPort} is reachable from {speaker.addr}</li>
            <li>Check router BMP configuration</li>
            <li>Check firewall rules</li>
          </ul>
        </details>
      </div>
    {/each}
  </div>
```

### 9.4 State C: Full dashboard (speaker-centric layout)

The full dashboard should lead with **devices** (BMP speakers), not abstract metrics.
Each router gets a card showing its BMP session state and a summary of what it's
sending:

```svelte
{:else}
  <!-- DEVICE CARDS — top section -->
  <section data-testid="speaker-section">
    <h2>BGP Speakers</h2>
    <div class="speaker-grid">
      {#each speakers as speaker}
        <div data-testid="speaker-card-{speaker.addr}"
             class="speaker-card {speaker.state}">
          <!-- Router identity -->
          <div class="speaker-header">
            <span data-testid="speaker-hostname" class="hostname">
              {speaker.hostname || speaker.addr}
            </span>
            <span data-testid="speaker-addr" class="addr">
              {speaker.addr}
            </span>
            <span data-testid="speaker-vendor" class="vendor-badge">
              {speaker.vendor}
            </span>
            <span data-testid="speaker-status" class="status-dot {speaker.bmp_state}" />
          </div>

          <!-- Key metrics -->
          <div class="speaker-metrics">
            <MetricCard label="Active Peers" value={speaker.peers_up} testid="speaker-peers-up-{speaker.addr}" />
            <MetricCard label="Routes" value={speaker.route_count} testid="speaker-routes-{speaker.addr}" />
            <MetricCard label="RPKI Valid%" value="{speaker.rpki_valid_pct}%" testid="speaker-rpki-{speaker.addr}" />
          </div>

          <!-- Quick links -->
          <div class="speaker-actions">
            <a href="/peers?speaker={speaker.addr}">View peers</a>
            <a href="/prefixes?speaker={speaker.addr}">View routes</a>
          </div>
        </div>
      {/each}
    </div>
  </section>

  <!-- SUMMARY METRICS — below speakers -->
  <section data-testid="summary-metrics">
    <MetricCard label="Total Peers Up"    value={totalPeersUp}    testid="dashboard-peers-up-count" />
    <MetricCard label="Total Routes"      value={totalRoutes}      testid="dashboard-total-routes" />
    <MetricCard label="Anomalies (24h)"   value={anomalyCount24h}  testid="dashboard-anomaly-count" />
    <MetricCard label="RPKI Invalid"      value={rpkiInvalidCount} testid="dashboard-rpki-invalid" />
  </section>

  <!-- RECENT EVENTS — live SSE -->
  <section data-testid="live-events">
    <h3>Live Events</h3>
    <!-- SSE feed from sse.ts -->
  </section>
```

This makes the homepage feel like a network management UI where **devices come first**,
not abstract statistics. Operators recognize this pattern from NMS/IPAM tools.

### 9.5 New API endpoint needed

`GET /api/speakers/summary` — per-speaker aggregated data:
```json
{
  "speakers": [
    {
      "addr": "10.0.0.100",
      "hostname": "xrd-pe1.example.com",
      "vendor": "Cisco IOS-XR",
      "bmp_state": "active",
      "peers_up": 3,
      "peers_down": 1,
      "route_count": 12450,
      "rpki_valid_pct": 87.3,
      "last_message_at": "2026-06-20T14:22:33Z"
    }
  ]
}
```

---

## Part 10 — Unified Testing Epic Index

*Reference: RUSTYBMP_TESTING_STRATEGY.md (full document)*

### Testing Layer Map

| Layer | Name | Tool | Time | Dependency |
|-------|------|------|------|-----------|
| 0 | Rust unit tests | `cargo test` | <10s | None |
| 1 | Wiring checks | `scripts/check_wiring.sh` | <15s | cargo build |
| 2 | Protocol integration | pytest | <60s | Local server + fixtures |
| 3 | API contracts | pytest | <90s | Local server + seed.sql |
| 4 | FRR smoke | pytest + clab | <3min | FRR (free) |
| 5 | XRd RFC 9972 | pytest + clab | <8min | XRd (imported ✅) |
| 6 | Internet-scale load | pytest/python | <30min | RouteViews MRT |
| 7 | UI E2E | playwright | <5min | Local server + UI |

### Protocol-specific test epics

| Epic | Description | Priority |
|------|-------------|----------|
| RV8-T1 | Capture BMP fixtures from XRd (Scenario 02 run) | P0 |
| RV8-T2 | `tests/fixtures/seed.sql` — DuckDB deterministic seed | P0 |
| RV8-T3 | `POST /api/_test/seed` — fixture injection endpoint | P0 |
| RV8-T4 | Layer 2 tests: `test_bmp_parsing.py` (20+ checks) | P0 |
| RV8-T5 | Layer 3 tests: `test_all_endpoints.py` (every endpoint) | P0 |
| RV8-T6 | `data-testid` tagging on all interactive UI elements | P0 |
| RV8-T7 | `tests/scenarios/01_frr_minimal/` — FRR BMP smoke | P0 |
| RV8-T8 | `tests/scenarios/02_xrd_rfc9972/` — RFC 9972 validation | P1 |
| RV8-T9 | `tests/scenarios/04_anomaly_injection/` — fault inject | P1 |
| RV8-T10 | `tests/load/mrt_replay.py` — RouteViews full table | P1 |
| RV8-T11 | `tests/load/ripe_ris_bridge.py` — RIPE RIS Live bridge | P1 |
| RV8-T12 | Playwright suite — all pages + data-testid | P1 |
| RV8-T13 | GitHub Actions CI — Layers 0-4 + 7 | P1 |
| RV8-T14 | `tests/load/caida_bmp_relay.py` — CAIDA Kafka BMP | P2 |

---

## Part 11 — ML Depth Additions

*Reference: RUSTYBMP_RV8_ANALYSIS.md §Part 12*

### Complete to_pyg() (stub since RV5)

```python
# bmppy/ml/topology_snapshot.py
def to_pyg(self) -> "HeteroData":
    from torch_geometric.data import HeteroData
    import torch
    data = HeteroData()
    x = torch.tensor(
        self.nodes_df[['route_count', 'churn_rate_1h', 'rpki_invalid_ratio',
                       'session_uptime_secs', 'flap_count_24h',
                       'best_count', 'ecmp_count', 'backup_count',  # RV7 new
                       'redundancy_ratio', 'filtered_count']].fillna(0).values,
        dtype=torch.float32
    )
    data['peer'].x = x
    # ... edges from bgpls_links ...
```

The Path Status TLV features (best_count, ecmp_count, etc.) added to the node feature
matrix make the STGNN significantly more powerful for detecting redundancy loss.

### Hijack probability classifier

Replace `HijackDetector` heuristic with `LogisticRegression` or `GradientBoostingClassifier`:

Features: `origin_asn_changed`, `prefix_specificity`, `rpki_validity_enc`,
`as_path_len_delta`, `aspa_verdict_enc`, `is_subprefix_of_known`.

Training data: BGPStream historical hijack events (labeled).

### Community semantics learner

`fpgrowth` on pre/post policy attribute correlations (mlxtend library) to infer
what communities mean operationally:
- Community 65001:100 → always correlates with LP=200 → "preferred transit" tag
- Community 65535:666 → always absent from Loc-RIB → "blackhole" tag

---

## Part 12 — RV8 Epic Index

### Testing Epics

| ID | Title | Priority |
|----|-------|----------|
| RV8-T1 through T14 | See Part 10 above | P0-P2 |

### Integration Epics

| ID | Title | Priority |
|----|-------|----------|
| RV8-OA1 | OpenAPI 3.0.3 spec (`api/schema.rs`) | P0 |
| RV8-OA2 | Swagger UI at `/api/swagger` | P0 |
| RV8-OA3 | Resolve endpoint for AI disambiguation | P1 |
| RV8-MC1 | MCP server at `/mcp` — 11 BGP tools | P0 |
| RV8-MC2 | NL → DuckDB SQL endpoint | P1 |
| RV8-MC3 | Daily token budget (500K, AtomicU64) | P1 |
| RV8-MC4 | ANOMALY_CATALOGUE with DuckDB verification queries | P1 |
| RV8-OUT1 | OutputAdapter trait + cursor persistence | P0 |
| RV8-OUT2 | Elasticsearch adapter (ECS BGP schema) | P0 |
| RV8-OUT3 | Splunk HEC adapter | P0 |
| RV8-OUT4 | ServiceNow EM adapter | P1 |
| RV8-OUT5 | Webhook adapter (Slack/PagerDuty/OpsGenie profiles) | P1 |
| RV8-ENR1 | NetBox enricher (dual REST/MCP transport, bonsai copy) | P1 |
| RV8-ENR2 | ServiceNow CMDB enricher (router CI context) | P2 |
| RV8-EXT1 | RIPE STAT client (`ripe_stat.py`) — prefix-visibility | P1 |
| RV8-EXT2 | IRR/RADB route object validation (`irr_client.py`) | P1 |
| RV8-EXT3 | Looking glass: Cloudflare Radar + HE BGP | P2 |
| RV8-EXT4 | RIPE Atlas measurement creation | P2 |
| RV8-EXT5 | `/api/external/prefix-visibility` combined view | P1 |

### Infrastructure Epics

| ID | Title | Priority |
|----|-------|----------|
| RV8-GOV1 | Resource governor (3-loop: memory/write/rate) | P0 |
| RV8-GOV2 | `GET /api/governance` monitoring endpoint | P0 |
| RV8-GOV3 | DuckDB write queue sizing for internet scale | P0 |
| RV8-GOV4 | Governor-aware batch size expansion under write pressure | P1 |

### UX Epics

| ID | Title | Priority |
|----|-------|----------|
| RV8-UX1 | Adaptive homepage: empty / waiting / active states | P0 |
| RV8-UX2 | Speaker cards on dashboard (hostname, vendor, metrics) | P0 |
| RV8-UX3 | `GET /api/speakers/summary` per-speaker aggregation | P0 |
| RV8-UX4 | Inline router config snippets (IOS-XR, FRR, EOS, JunOS) | P1 |
| RV8-UX5 | Output adapter management page (`/adapters`) | P1 |
| RV8-UX6 | NL query interface page (`/query`) | P1 |
| RV8-UI5 | Topology LOD: adaptive force/hierarchical/clustered | P2 |

### ML Epics

| ID | Title | Priority |
|----|-------|----------|
| RV8-ML1 | Complete `to_pyg()` with Path Status TLV features | P1 |
| RV8-ML2 | `train_bgp_stgnn.py` — GATv2-GRU training script | P1 |
| RV8-ML3 | Hijack probability classifier (replace heuristic) | P1 |
| RV8-ML4 | Convergence anomaly detector | P2 |
| RV8-ML5 | Community semantics learner (fpgrowth) | P2 |

---

## Part 13 — Implementation Priority for Next Code Session

**P0 — Do first** (foundational, blocks everything else):
1. Resource governor (`RV8-GOV1` through `GOV3`) — needed before internet-scale testing
2. FRR smoke lab (`RV8-T7`) — validates BMP pipeline end-to-end with zero friction
3. Adaptive homepage (`RV8-UX1` through `UX3`) — the first thing every operator sees
4. API seed endpoint + fixtures (`RV8-T2` through `T3`) — enables all higher layers

**P1 — High value, plan for sprint**:
5. XRd RFC 9972 validation (`RV8-T8`) — the core protocol promise
6. OpenAPI spec + Swagger UI (`RV8-OA1` through `OA2`)
7. MCP server with 11 tools (`RV8-MC1`) — competitive differentiator
8. Elasticsearch + Splunk adapters (`RV8-OUT2` through `OUT3`)
9. RIPE STAT client (`RV8-EXT1`) — prefix visibility
10. Playwright suite (`RV8-T12`)

---

## Part 14 — Upload Next Diff As

`rv8_all_changes.patch`

---

*End of RUSTYBMP_BACKLOG_RV8.md — Sprint RV8*
*Synthesized from: RUSTYBMP_RV8_ANALYSIS.md · RUSTYBMP_TESTING_STRATEGY.md · session conversations*
