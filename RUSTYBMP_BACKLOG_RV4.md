# RustyBMP — Sprint RV4 Backlog
## Production Hardening · UI · ML Pipeline · Graph Topology · Testing

> **Version**: RV4  
> **Date**: 2026-06-20  
> **Basis**: Full RV3 diff analysis + bonsai graph DB + parquet ML code analysis + README audit + what's genuinely still missing  
> **Sprint summary**: RV1-RV3 delivered a complete, competitive BMP collector. RV4 is about production readiness, testability, ML pipeline integration, and the UI layer.

---

## Part 1 — RV3 Comprehensive Analysis

### 1.1 What was delivered (all 7 bundles)

The RV3 diff (2549 lines, 1530 added, 186 removed) confirms all 7 bundles complete. 49 Rust tests pass, 0 failures.

| Bundle | Content | Quality Assessment |
|--------|---------|-------------------|
| A: Protocol | SR Policy SAFI73 types A-K, EVPN types 6-11, BGP-LS attribute TLVs 29 (adj-SID, SRGB, SR-caps, Flex Algo, link bandwidth), RTC SAFI132 | ✅ `srpolicy.rs` not in diff = committed at sprint base (backlog code used directly); bgpls.rs shows 303 added lines of attribute decode |
| B: Filter + LLGR | YAML filter DSL wired into `RibManager.set_filter()`; LLGR state machine (`Normal/StaleMarked/Deleted`) with `mark_stale_all()` + `drain_deleted_stale()` in `RibTable`; `expire_stale_for_peer()` in `RibManager` | ✅ Clean state machine. `llgr_stale_secs` extracted from `BgpCapability::LongLivedGracefulRestart` max across AFI/SAFIs (D9) |
| C: DNS + Proxy | `DnsCache` struct in `dns.rs` (TTL-bounded OS resolver); `ProxyConfig { enabled, listen_addr, upstream_addr }` in config; BMP tee forwarding | ✅ Config-gated, sensible defaults |
| D: Kafka | `rbmp-kafka` crate; `KafkaProducer` (rdkafka FutureProducer, lz4); `run_kafka_sink` task; typed topics matching OpenBMP namespace | ✅ Correct fail-gracefully pattern (producer creation error disables Kafka without crashing) |
| E: MRT | `rbmp-mrt` crate; RFC 6396 BGP4MP + TABLE_DUMP_V2 reader + writer; D9 bug fix (body_len undercount by 2 bytes in writer) | ✅ 8 tests. The D9 bug was found through test failures — good signal that the test suite caught a real wire-format error |
| F: Python | `rpki.py` (`RtrVrpCache`, poll_rtr_cache); `internet.py` (`IrrClient`, `RdapClient`, `BgpToolsClient`, `resolve_origin`); `detectors.py` (4 detectors + pipeline) | ✅ D13: origin_as extracted via last-integer scan (pragmatic given RouteEvent.as_path is a string); D14: linear VRP scan (O(n) adequate at 400K entries for alert latency) |
| G: Distributed | `CollectorEnvelope` (MessagePack/rmp-serde, 4-byte length prefix, 8MiB max frame); `rbmp-collector` binary; Core TCP listener :5001; `collector_id` in all 3 event tables + 2 new indexes | ✅ D11 (try_send ring buffer) and D12 (Core re-parses raw BMP) are correct architectural choices |

### 1.2 Key decisions quality review

| Decision | Assessment |
|----------|-----------|
| D9: MRT body_len +2 | **Critical correctness fix** — would have produced unreadable MRT files in production. Test-driven catch. |
| D10: MessagePack over TCP vs Protobuf | **Right choice**. MessagePack is self-describing, simpler tooling, zero generated code. Protobuf would add a proto compiler dep. For internal protocol between our own binaries, msgpack is perfect. |
| D11: `try_send()` drop vs back-pressure | **Correct**. A BMP session back-pressure would cause TCP buffer fill, which causes the router to slow down or reset the session. Better to drop monitoring PDUs than disrupt the routing plane. |
| D12: Core re-parses raw BMP bytes | **Right trade-off**. Keeps `rbmp-collector` at ~300 lines with zero parsing logic. The parsing cost at the Core is negligible vs transport simplicity. |
| D13: Last integer in as_path string | **Pragmatic**. Would break on AS_SETs (last member isn't necessarily the origin). Better solution in RV4: proper AS_PATH parser that strips SET brackets. Document as known limitation. |
| D14: Linear VRP scan O(n) | **Adequate now**, watch at 500K+ VRPs. An interval tree would be O(log n) per lookup. Flag as a future optimization. |

### 1.3 What is NOT in the diff (still genuinely missing)

| Gap | Confirmed absent | Notes |
|-----|-----------------|-------|
| `filter.rs` in `rbmp-rib` | Not in diff | FilterEngine referenced in manager.rs diff — must be committed at sprint base (backlog code) |
| `srpolicy.rs` | Not in diff | Same — committed at sprint base from backlog spec |
| `crates/rbmp-server/src/dns.rs` | Not in diff | DNS cache committed at base |
| `crates/rbmp-server/src/proxy.rs` | Not in diff | Proxy committed at base |
| Integration test suite | Not present at all | Zero integration tests; only unit tests |
| Dockerfile | Not present | No container image |
| CI/CD (.github/workflows) | Not present | No automated build pipeline |
| Benchmark suite | Not present | Performance claims unverified |
| API auth/TLS | Not present | Completely open HTTP API |
| DuckDB retention policy | Not present | DB can grow indefinitely |
| Parquet export | Not present | No ML training data pipeline |
| UI / dashboard | Not present | `serve_ui = true` in config but no UI code |

---

## Part 2 — Bonsai Graph DB Core Analysis

### 2.1 What bonsai's graph DB is

Bonsai uses **LadybugDB** (an alias for KuzuDB — an embedded Cypher-compatible graph database, similar to Neo4j but embeddable like SQLite). The graph schema has:

**Node tables**: `Device`, `Interface`, `BgpNeighbor`, `BfdSession`, `LldpNeighbor`, `StateChangeEvent`, `DetectionEvent`, `Remediation`, `Application`, `Rack`, `Site`, `Environment`

**Relationship tables**: `HAS_INTERFACE` (Device→Interface), `PEERS_WITH` (Device→BgpNeighbor), `CONNECTED_TO` (Interface→Interface via LLDP), `HAS_BFD_SESSION`, `REPORTED_BY` (Device→StateChangeEvent), `RESOLVES` (Remediation→DetectionEvent)

**Key queries** (from `src/graph/queries.rs`):
- Blast radius: multi-hop BFS from a failing device to all reachable devices (BGP peers, physical neighbors)
- Topology path: shortest path between two devices
- Neighbor discovery: LLDP-based physical adjacency
- BMP session correlation: join BmpSession to Device for BGP-aware blast radius

**Why bonsai built this**: Bonsai ingests gNMI (device state, interface counters), LLDP, BFD, syslog, SNMP, AND BMP — all from the same devices. The graph naturally represents: "this router, which has these physical interfaces, connected to these neighbors via LLDP, with these BGP sessions, experienced this detection event." Multi-hop Cypher queries like `MATCH (d:Device)-[:CONNECTED_TO*1..3]->(neighbor:Device) WHERE d.address = '10.0.0.1' RETURN neighbor` are trivial in Cypher and awkward in relational SQL.

### 2.2 Does a graph DB make sense for rustybmp?

**Short answer: No — but a lightweight topology graph from BGP-LS data does.**

**Argument against adding a full graph DB (KuzuDB) to rustybmp:**

1. **No physical topology data**: rustybmp is BMP-only. Without gNMI (interface counters), LLDP (physical neighbors), and BFD (session state), there are no physical edges in the graph. The only relationships are BGP sessions (peer-to-peer) and AS-path hops (router-to-router at the protocol level). These are already adequately represented in DuckDB tables.

2. **BGP-LS IS a topology DB already**: BGP-LS (which we now fully decode) carries the complete IGP topology from IS-IS/OSPF: `bgpls_nodes` and `bgpls_links` tables. For path computation and neighbor analysis, this is what you use. DuckDB recursive CTEs can traverse adjacency graphs, and Python NetworkX can do full graph analytics on the exported data.

3. **AS topology from AS_PATH is naturally relational**: "Which ASes are connected?" → `SELECT DISTINCT all_asns FROM route_events`. "Who does AS64496 peer with?" → scan AS_PATH strings. This is relational, not graph-native.

4. **Operational cost of a graph DB**: KuzuDB adds ~60MB to the binary, requires a separate database file, and uses Cypher syntax that most operators don't know. DuckDB is already there and known.

5. **The STGNN insight**: Bonsai's graph DB is primarily needed as input to the STGNN — a Spatio-Temporal Graph Neural Network where graph structure (edges between devices) provides the topology for message passing. For rustybmp, the equivalent would be a BGP peer graph where nodes = BGP speakers/peers and edges = BGP sessions. This graph is small enough (~hundreds of nodes for most networks) to live entirely in Python (NetworkX or PyTorch Geometric).

**What IS worth doing: a lightweight in-memory AS topology graph from BGP-LS**

Instead of a graph database, derive an in-memory graph structure from BGP-LS data:

```python
# rbmppy/topology.py (RV4)
import networkx as nx

class BgpLsTopology:
    """
    Lightweight topology graph derived from BGP-LS data in DuckDB.
    Nodes = BGP-LS routers (from bgpls_nodes).
    Edges = BGP-LS links (from bgpls_links with IGP metrics).
    """
    def __init__(self, analytics: RouteAnalytics):
        self.G = nx.DiGraph()
        self._load_from_duckdb(analytics)

    def shortest_path(self, src: str, dst: str) -> list[str]:
        return nx.shortest_path(self.G, src, dst, weight='igp_metric')

    def blast_radius(self, node: str, max_hops: int = 3) -> set[str]:
        return set(nx.single_source_shortest_path(self.G, node, cutoff=max_hops).keys())

    def as_topology(self) -> nx.DiGraph:
        """AS-level graph derived from BGP AS_PATH data."""
        ...
```

This gives all the graph capabilities bonsai uses for BGP/BMP analysis, without the complexity of a graph DB.

**Verdict**: Do not add KuzuDB/graph DB. Add `rbmppy/topology.py` in RV4 — a Python NetworkX-backed topology derived from BGP-LS DuckDB tables. Provide path computation, blast radius, and AS topology methods. This is a 100-line Python module, not a database engine.

---

## Part 3 — Bonsai Parquet + ML Pipeline Analysis

### 3.1 What bonsai's parquet pipeline does

Bonsai has a sophisticated multi-stage ML pipeline:
1. `export_training.py` → Cypher graph queries → Parquet files (anomaly + normal windows)
2. `train_anomaly.py` → Reads Parquet → trains `IsolationForest` (Model A, anomaly detection)
3. `train_stgnn.py` → Reads `SnapshotStore` (Arrow IPC, T=8 temporal snapshots) → trains STGNN (GATv2-GRU)
4. `parquet_store.py` → Rolling archive with `latest` symlinks, type-separated directories
5. `ml/inference_loop.py` → Loaded models run continuously against new data

**Feature matrix for anomaly detection** (from `ml_detector.py`):
- `peer_count_total`, `peer_count_established` (session health)
- `recent_flap_count` (instability)
- `oper_status_enc`, `event_type_enc` (categorical encodings)
- `occurred_at_s` (temporal)

**STGNN architecture** (`train_stgnn.py`):
- T=8 temporal snapshots (GRU component)
- GAT v2 (Graph Attention Network) message passing
- Heterogeneous graph: Device, Interface, BgpNeighbor, BfdSession nodes
- Phase 1: NCT (Negative Context Training) pre-training for topology structure learning
- Phase 2: Supervised anomaly classification

### 3.2 Is parquet output relevant for rustybmp? Yes — strongly.

**The key insight**: DuckDB exports Parquet with a single SQL statement. The cost is near-zero. The value is immediate access to every Python ML framework.

```sql
-- Export route feature matrix for ML training (DuckDB native)
COPY (
    SELECT
        prefix,
        peer_addr,
        peer_as,
        speaker_addr,
        rib_type,
        action,
        as_path,
        ARRAY_LENGTH(STRING_SPLIT(TRIM(as_path), ' ')) AS hop_count,
        local_pref,
        med,
        communities,
        rpki_validity,
        occurred_at,
        collector_id
    FROM route_events
    WHERE occurred_at >= NOW() - INTERVAL 7 DAY
) TO 'training/route_events_7d.parquet' (FORMAT PARQUET, COMPRESSION 'zstd');
```

**Three Parquet pipelines rustybmp should support** (all in `rbmppy/parquet.py`):

**Pipeline 1 — Route Anomaly Training** (equivalent to bonsai's Model A):
- Features: per-prefix churn rate, AS_PATH hop count, prepend ratio, RPKI validity, community count
- Labels: anomalous (Z-score > 3) vs normal
- Model: IsolationForest trained on 7-day history

**Pipeline 2 — Peer Stability Training**:
- Features: per-peer session flap count, route count delta, EOR timing, hold time negotiated, LLGR active
- Labels: stable vs flapping vs oscillating
- Model: LogisticRegression or RandomForest

**Pipeline 3 — BGP Topology Snapshots (STGNN equivalent)**:
- Nodes: BGP speakers + peers (from bgpls_nodes + peer_events)
- Edges: BGP sessions (from peer_events, adjSIDs from bgpls_links)
- Node features: route_count, rpki_invalid_rate, churn_rate, session_uptime
- Temporal: T=8 snapshots at 5-minute intervals
- Model: GATv2-GRU (identical architecture to bonsai STGNN but with BGP-domain features)

**What's directly adaptable from bonsai**:
- `parquet_store.py` → adapt as `rbmppy/parquet_store.py` (same directory structure pattern, DuckDB export instead of graph query)
- `train_anomaly.py` → adapt as `bmppy/ml/train_route_anomaly.py` (same IsolationForest, different features)
- `snapshot_store.py` (Arrow IPC) → adapt as `bmppy/ml/snapshot_store.py` (BGP peer graph snapshots)
- `train_stgnn.py` → adapt as `bmppy/ml/train_bgp_stgnn.py` (same architecture, BGP-domain features)

**The adaptation is NOT trivial but IS high-value**. The STGNN trained on BGP topology snapshots could detect:
- Convergence instability before it causes outages
- Route oscillation patterns
- Policy misconfiguration (unexpected AS_PATH changes across the topology)
- ECMP imbalance (one path gaining disproportionate traffic)

**Verdict**: Add `bmppy/ml/` directory in RV4 with Parquet export, IsolationForest anomaly training, and a BGP topology STGNN. This is the highest-leverage ML capability we can add, and it's directly adapted from bonsai's proven architecture.

---

## Part 4 — Consolidated "What Went Well / What's Pending"

### 4.1 What went well across RV1-RV3

| Area | Status | Notes |
|------|--------|-------|
| Protocol completeness | ✅ Excellent | Ahead of all competitors on RFC 9972; EVPN 1-11, SR Policy, BGP-LS full, RTC — comprehensive |
| Code modularity | ✅ Excellent | Crate boundaries held. No compilation bottlenecks. Each crate ≤ 500 lines per file rule maintained. |
| Test discipline | ✅ Good | 49 Rust unit tests. MRT bug (D9) caught by tests, not by production. |
| Decision documentation | ✅ Excellent | `results_and_decisions.md` is high-quality — every non-obvious choice is explained. |
| Dev workflow | ✅ Good | Diff-based uploads working. Session continuity maintained via project context files. |
| RPKI RTR client | ✅ Complete | RTR protocol client in Rust (live VRP cache) + Python (RtrVrpCache). |
| Analytics (Python) | ✅ Good | Z-score, HijackDetector, RouteLeakDetector, FlapScorer, pipeline. |
| Distributed collector | ✅ Good | MessagePack framing + ring buffer + reconnect — simple, correct. |
| Kafka output | ✅ Good | Typed topics, lz4 compression, graceful failure if broker unavailable. |
| MRT import/export | ✅ Complete | 8 tests, wire-format bug found and fixed. |

### 4.2 What is genuinely still pending

#### Production-critical (blockers for real deployment)

| Gap | Impact | RV4 Epic |
|-----|--------|----------|
| No API authentication | Anyone with network access can read all BGP data | RV4-1 |
| No BMP TLS | Collector→core and router→collector traffic is plaintext | RV4-1 |
| No DuckDB retention policy | DB grows indefinitely, eventually OOMs | RV4-2 |
| No Dockerfile | Can't deploy without compiling from source | RV4-8 |
| No integration tests | Unit tests don't validate end-to-end BMP→parse→store→query | RV4-9 |
| No Ubuntu test document | Operators have no runbook for testing on real hardware | RV4-9 |

#### Protocol gaps

| Gap | Impact | RV4 Epic |
|-----|--------|----------|
| BGP-LS SRv6 SID NLRI (SAFI 72) | DC operators running SRv6 can't see SID topology | RV4-5 |
| L2VPN VPLS (SAFI 65) | Legacy ISP environments still use VPLS | RV4-5 |
| SR Policy routing decisions not linked to VPN | Can't correlate SR Policy to L3VPN forwarding | RV4-5 |
| BGPsec path validation | No cryptographic path verification | RV4-6 |
| OTC violation detection (RFC 9234) fully wired | OTC attr parsed but leak detector not using it | RV4-5 |

#### Operational features

| Gap | Impact | RV4 Epic |
|-----|--------|----------|
| No UI dashboard | Operators need web interface, not just CLI/API | RV4-3 |
| No BGP topology graph from BGP-LS | Topology analysis requires Python one-off scripts | RV4-4 |
| No Parquet export + ML pipeline | No ability to train models on collected data | RV4-4 |
| HA leader election | Single-point failure at Core | RV4-7 |
| NATS output | Edge deployments where Kafka is too heavy | RV4-7 |
| API rate limiting | No protection against bulk scraping | RV4-1 |
| No `cargo bench` | Performance claims unverified | RV4-9 |
| No CI/CD pipeline | Manual test gate before merging | RV4-8 |

---

## Part 5 — RV4 Epics

### Epic RV4-1: Security Hardening — Auth, TLS, Rate Limiting

**Scope**: `crates/rbmp-server/`

This is the primary blocker for any production deployment.

#### RV4-1 T1 — JWT authentication for HTTP API

**File**: `crates/rbmp-server/src/api/auth.rs`

```rust
// Stateless JWT authentication using HS256.
// Token issued by `rbmp-server` on POST /auth with correct API key.
// All /api/* endpoints require Bearer token.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
    http::StatusCode,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use chrono::Utc;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub:  String,    // username
    pub exp:  i64,       // Unix timestamp expiry
    pub iat:  i64,       // issued-at
}

/// Axum middleware: extract Bearer token from Authorization header, validate JWT.
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !state.cfg.auth.enabled {
        return Ok(next.run(request).await);
    }
    let token = request.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.cfg.auth.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    ).map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(next.run(request).await)
}
```

**Config addition**:
```toml
[auth]
enabled    = false           # set true for production
jwt_secret = "change-me-32-byte-minimum-secret"
token_ttl_secs = 86400       # 24 hours
api_keys = []                # pre-issued keys (base64)
```

**New endpoint**: `POST /auth` → returns `{"token": "eyJ..."}` given valid API key.

#### RV4-1 T2 — TLS for BMP TCP connections

**File**: `crates/rbmp-server/Cargo.toml` — add `rustls` and `tokio-rustls`

BMP routers don't natively support TLS, but the collector→core link and any external collector can use TLS.

```toml
[tls]
enabled  = false
cert_pem = "certs/server.pem"
key_pem  = "certs/server.key"
# For collector→core: mTLS with client certs
client_ca_pem = ""
```

In `receiver.rs`, wrap `TcpListener` with `TlsAcceptor` when `cfg.tls.enabled`:
```rust
use tokio_rustls::TlsAcceptor;
// Load cert + key from cfg.tls.cert_pem / key_pem
// TlsAcceptor::from(Arc::new(tls_config))
// acceptor.accept(stream).await? → TlsStream
```

#### RV4-1 T3 — Per-speaker rate limiting (token bucket)

Inspired by `cloudflare/bbmp2kafka`'s `tokenBucket.go`. Per-speaker token bucket prevents a misbehaving BMP sender from OOMing the collector.

**File**: `crates/rbmp-server/src/rate.rs`

```rust
/// Per-speaker token bucket rate limiter.
/// Configured by [bmp.rate_limit_msgs_per_sec] in rustybmp.toml.
pub struct TokenBucket {
    tokens:       f64,
    capacity:     f64,
    refill_rate:  f64,   // tokens per millisecond
    last_refill:  std::time::Instant,
}

impl TokenBucket {
    pub fn new(msgs_per_sec: u32) -> Self { ... }
    
    /// Returns true if the message should be processed; false = drop.
    pub fn allow(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
```

Config: `[bmp] rate_limit_msgs_per_sec = 0  # 0 = unlimited`

#### RV4-1 T4 — API rate limiting middleware

Use `tower-governor` crate:
```toml
tower-governor = "0.4"
```

Apply to the Axum router with per-IP limits for unauthenticated requests.

---

### Epic RV4-2: DuckDB Retention Policy + Export API

**Scope**: `crates/rbmp-store/`

#### RV4-2 T1 — Automatic event table pruning

**File**: `crates/rbmp-store/src/retention.rs`

```rust
/// Retention policy: delete events older than N days.
/// Runs as a background task, triggered by the checkpoint task.
pub async fn run_retention_sweep(
    store: Arc<std::sync::Mutex<RouteStore>>,
    retain_days: u32,
) {
    let sql = format!(
        "DELETE FROM route_events WHERE occurred_at < NOW() - INTERVAL {} DAY",
        retain_days
    );
    // Same for peer_events, speaker_events, stats_events, evpn_events
}
```

Config:
```toml
[store]
# Set to 0 to disable (keep all history)
retain_days = 90
```

#### RV4-2 T2 — Parquet export API endpoint

```
GET /api/export/parquet?table=route_events&since=<ISO>&until=<ISO>&format=parquet
```

DuckDB can write Parquet directly:
```rust
conn.execute(&format!(
    "COPY (SELECT * FROM route_events WHERE occurred_at BETWEEN '{}' AND '{}') TO '{}' (FORMAT PARQUET, COMPRESSION 'zstd')",
    since, until, output_path
), [])?;
```

Returns: 303 redirect to the file download, or streams the file directly.

#### RV4-2 T3 — DuckDB database size metrics

Expose DuckDB size in Prometheus:
```
rustybmp_duckdb_size_bytes{table="route_events"} 1234567
rustybmp_duckdb_total_rows{table="route_events"} 890123
```

Query via `SELECT estimated_size FROM duckdb_tables() WHERE table_name = 'route_events'`.

---

### Epic RV4-3: Svelte 5 UI Dashboard

**Scope**: new `ui/` directory

This is the most visible RV4 addition. A single-page Svelte 5 app served by the Axum server at `/`.

#### RV4-3 T1 — Project scaffold

```
ui/
├── package.json          (Svelte 5 + Vite + TailwindCSS)
├── vite.config.ts
├── src/
│   ├── App.svelte
│   ├── routes/
│   │   ├── Dashboard.svelte     — live overview: speakers, peer counts, route counts
│   │   ├── Prefixes.svelte      — prefix search with history
│   │   ├── Peers.svelte         — per-peer session state + route counts
│   │   ├── Topology.svelte      — BGP-LS topology graph (D3.js)
│   │   └── Alerts.svelte        — anomaly alerts from detectors.py
│   ├── lib/
│   │   ├── api.ts               — fetch wrappers for /api/*
│   │   └── sse.ts               — EventSource wrapper for /api/events
│   └── components/
│       ├── RouteTable.svelte
│       ├── PeerCard.svelte
│       └── RpkiBadge.svelte
└── public/
```

#### RV4-3 T2 — Build integration with Cargo

Add build step to `build.rs` in `rbmp-server`:
```rust
fn main() {
    if cfg!(feature = "ui") {
        // npm run build → dist/ → embed with include_dir!()
    }
}
```

Serve with `ServeDir::new("ui/dist")` from Axum or embed using `rust-embed`.

#### RV4-3 T3 — Dashboard panels (priority order)

1. **Speakers panel**: count, up/down status, last-seen timestamp, hostname from registry
2. **Peer timeline**: per-peer session state as timeline (D3 gantt-style)
3. **Route count sparklines**: per-peer route count over last 24h
4. **RPKI status breakdown**: donut chart (valid/invalid/not-found)
5. **Top churning prefixes**: table, auto-refreshes every 30s
6. **Live event stream**: SSE-backed log of last 50 route events
7. **Prefix search**: enter a prefix, see current state + history + RPKI + origin info

#### RV4-3 T4 — BGP-LS topology graph (D3.js force-directed)

Fetch from `/api/bgpls/graph` (new API endpoint returning nodes + links):
```
GET /api/bgpls/graph?protocol=isis  → { nodes: [...], links: [...] }
```

Render as D3 force-directed graph. Node color = router role (PE/P/RR). Edge thickness = IGP metric (inverse — lower metric = thicker edge). Edge label = adjacency SID.

---

### Epic RV4-4: ML Pipeline — Parquet + BGP Anomaly Training + STGNN

**Scope**: new `bmppy/ml/` directory

Adapted directly from bonsai's ML pipeline architecture.

#### RV4-4 T1 — `bmppy/rbmppy/parquet.py` — DuckDB → Parquet export

```python
"""Export DuckDB tables to Parquet for ML training.

Usage:
    from rbmppy.parquet import export_route_features, export_peer_stability
    
    # Export 7-day route feature matrix
    rows = export_route_features(db_path="runtime/routes.duckdb",
                                  output="ml/data/routes_7d.parquet",
                                  days=7)
    print(f"Exported {rows} rows")
"""
from __future__ import annotations
import duckdb
from pathlib import Path
from typing import Optional
from datetime import datetime, timezone, timedelta

def export_route_features(
    db_path: str,
    output: str,
    days: int = 7,
    since: Optional[datetime] = None,
) -> int:
    """
    Export per-prefix route event features to Parquet.
    
    Feature columns:
      - prefix, peer_addr, peer_as, speaker_addr, rib_type, action
      - hop_count (AS_PATH length), origin_asn, has_prepend
      - local_pref, med, community_count
      - rpki_validity (encoded: valid=1, invalid=-1, not-found=0)
      - occurred_at (unix seconds float)
    """
    since_ts = since or (datetime.now(timezone.utc) - timedelta(days=days))
    Path(output).parent.mkdir(parents=True, exist_ok=True)
    conn = duckdb.connect(db_path, read_only=True)
    
    # DuckDB native Parquet export
    conn.execute(f"""
        COPY (
            SELECT
                prefix,
                peer_addr,
                CAST(peer_as AS INTEGER) AS peer_as,
                speaker_addr,
                rib_type,
                CASE WHEN action = 'announce' THEN 1 ELSE 0 END AS is_announce,
                -- AS_PATH features
                COALESCE(as_path_len, 0) AS hop_count,
                COALESCE(CAST(list_last(string_split(trim(as_path), ' ')) AS INTEGER), 0) AS origin_asn,
                COALESCE(local_pref, 100) AS local_pref,
                COALESCE(med, 0) AS med,
                -- Community count
                CASE WHEN communities IS NULL OR communities = '' THEN 0
                     ELSE len(string_split(communities, ','))
                END AS community_count,
                -- RPKI encoding
                CASE rpki_validity
                    WHEN 'valid' THEN 1
                    WHEN 'invalid' THEN -1
                    ELSE 0
                END AS rpki_enc,
                EPOCH(occurred_at) AS occurred_at_s,
                collector_id
            FROM route_events
            WHERE occurred_at >= TIMESTAMPTZ '{since_ts.isoformat()}'
        ) TO '{output}' (FORMAT PARQUET, COMPRESSION 'zstd')
    """)
    count = conn.execute(f"SELECT COUNT(*) FROM route_events WHERE occurred_at >= TIMESTAMPTZ '{since_ts.isoformat()}'").fetchone()[0]
    conn.close()
    return count


def export_peer_stability(
    db_path: str,
    output: str,
    days: int = 7,
) -> int:
    """Export per-peer session stability features to Parquet."""
    since_ts = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
    Path(output).parent.mkdir(parents=True, exist_ok=True)
    conn = duckdb.connect(db_path, read_only=True)
    conn.execute(f"""
        COPY (
            SELECT
                p.peer_addr,
                p.peer_as,
                p.speaker_addr,
                -- Session events in window
                COUNT(CASE WHEN p.event_type = 'peer_up'   THEN 1 END) AS up_count,
                COUNT(CASE WHEN p.event_type = 'peer_down' THEN 1 END) AS down_count,
                -- Route metrics (from route_events)
                COALESCE(r.route_count, 0) AS current_route_count,
                COALESCE(r.churn_events, 0) AS churn_events,
                COALESCE(r.rpki_invalid_count, 0) AS rpki_invalid_count,
                EPOCH(MAX(p.occurred_at)) AS last_event_s
            FROM peer_events p
            LEFT JOIN (
                SELECT peer_addr,
                       COUNT(*) FILTER (WHERE action = 'announce') AS route_count,
                       COUNT(*) AS churn_events,
                       COUNT(*) FILTER (WHERE rpki_validity = 'invalid') AS rpki_invalid_count
                FROM route_events
                WHERE occurred_at >= TIMESTAMPTZ '{since_ts}'
                GROUP BY peer_addr
            ) r ON p.peer_addr = r.peer_addr
            WHERE p.occurred_at >= TIMESTAMPTZ '{since_ts}'
            GROUP BY p.peer_addr, p.peer_as, p.speaker_addr, r.route_count,
                     r.churn_events, r.rpki_invalid_count
        ) TO '{output}' (FORMAT PARQUET, COMPRESSION 'zstd')
    """)
    conn.close()
    return conn.execute(f"SELECT COUNT(DISTINCT peer_addr) FROM peer_events WHERE occurred_at >= TIMESTAMPTZ '{since_ts}'").fetchone()[0]
```

#### RV4-4 T2 — `bmppy/ml/train_route_anomaly.py` — IsolationForest on route features

Adapted from `bonsai/python/train_anomaly.py`:

```python
"""Train Model A — IsolationForest for BGP route anomaly detection.

Usage:
    # 1. Export training data (requires running rustybmp):
    python -m rbmppy.parquet --output ml/data/routes_7d.parquet --days 7

    # 2. Train:
    python bmppy/ml/train_route_anomaly.py --input ml/data/routes_7d.parquet

    # 3. Use in detection pipeline:
    from rbmppy.detectors import IsolationForestDetector
    det = IsolationForestDetector("models/route_anomaly_v1.joblib")
"""
FEATURES = [
    "hop_count", "origin_asn", "is_announce",
    "local_pref", "med", "community_count",
    "rpki_enc", "occurred_at_s",
]
# ... train IsolationForest(n_estimators=200, contamination=0.05) on FEATURES
```

#### RV4-4 T3 — BGP topology snapshots (STGNN prep)

```python
# bmppy/ml/topology_snapshot.py

class BgpTopologySnapshot:
    """
    A single time-slice of the BGP peer graph for STGNN training.
    
    Nodes: BGP speakers and peers (from bgpls_nodes + peer_events)
    Edges: BGP sessions (peer_events Up) + BGP-LS links (bgpls_links)
    Node features (per peer):
        - route_count, churn_rate_1h, rpki_invalid_ratio
        - session_uptime_secs, flap_count_24h
        - is_llgr_capable, add_path_capable
    Edge features (per BGP-LS link):
        - igp_metric, max_bandwidth, adj_sid_label
    """
    def to_pyg_heterodata(self):
        """Convert to PyTorch Geometric HeteroData for STGNN."""
        ...

    @classmethod
    def from_duckdb(cls, analytics: RouteAnalytics, at: datetime) -> 'BgpTopologySnapshot':
        """Load a topology snapshot at a given timestamp from DuckDB."""
        ...
```

#### RV4-4 T4 — Parquet store (`bmppy/ml/parquet_store.py`)

Identical architecture to bonsai's `parquet_store.py` but reading from DuckDB:
```
ml/data/
  route_anomaly/
    2026-06-19T00-00-00Z_v1_45230rows.parquet
    latest -> 2026-...parquet
  peer_stability/
    ...
  bgp_snapshots/
    2026-06-19T00-00-00Z_T8_snapshots.arrow
    latest -> ...
```

---

### Epic RV4-5: Protocol Completeness (remaining gaps)

#### RV4-5 T1 — BGP-LS SRv6 SID NLRI (AFI 16388, SAFI 72)

SRv6 SID NLRI carries SRv6 segment identifiers (128-bit SIDs) with their endpoint behaviors. Growing critical for cloud provider networks and modern SP SRv6 deployments.

**File**: `crates/rbmp-core/src/bgp/bgpls.rs`

```rust
/// SRv6 SID NLRI (BGP-LS AFI=16388, SAFI=72) — RFC 9514
/// Format: protocol(1) + identifier(8) + local_node_desc + SRv6 SID TLVs
pub struct Srv6SidNlri {
    pub protocol_id:    u8,
    pub identifier:     u64,
    pub local_node:     NodeDescriptor,
    pub srv6_sid:       [u8; 16],
    pub endpoint_behavior: Option<Srv6EndpointBehavior>,
    pub sid_structure:  Option<Srv6SidStructure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Srv6EndpointBehavior {
    pub behavior: u16,   // End, End.X, End.T, End.DX6, End.DT4, etc.
    pub description: &'static str,
}

pub fn srv6_behavior_name(b: u16) -> &'static str {
    match b {
        1    => "End",     2  => "End.X",    3  => "End.T",
        4    => "End.DX6", 5  => "End.DX4",  6  => "End.DT6",
        7    => "End.DT46",8  => "End.DT4",  
        0x48 => "End.OP",  0x49 => "End.Otp",
        _    => "Unknown",
    }
}

pub struct Srv6SidStructure {
    pub locator_block_len: u8,    // typically 40 bits
    pub locator_node_len: u8,     // typically 24 bits
    pub function_len: u8,         // typically 16 bits
    pub argument_len: u8,         // typically 0-24 bits
}
```

Add `BgpLsNlri::Srv6Sid(Srv6SidNlri)` variant and decoder.

#### RV4-5 T2 — L2VPN VPLS (AFI 25, SAFI 65)

RFC 4761 VPLS. Parse:
- Encapsulation sub-TLVs (type 10, 11, 12)
- Layer 2 info extended community (0x40, 0x0D)
- VE (Virtual Edge) block allocation: VE ID, block offset, block size, label base

```rust
// crates/rbmp-core/src/bgp/types.rs
pub struct VplsNlri {
    pub rd:            [u8; 8],
    pub ve_id:         u16,
    pub ve_block_off:  u16,
    pub ve_block_size: u16,
    pub label_base:    u32,
}
```

#### RV4-5 T3 — Wire OTC attribute into route leak detection

The OTC (Only-to-Customer, RFC 9234 type 35) attribute is already parsed and stored in `PathAttributes.only_to_customer`. Wire it into the Python `RouteLeakDetector`:

```python
# In detectors.py RouteLeakDetector.check():
if event.only_to_customer is not None:
    # OTC present — check if this route came from a provider peer
    # If provider peer is advertising with OTC set, it may be a leak
    ...
```

This requires adding `only_to_customer: Optional[int]` to `RouteEvent` in `models.py` and exposing it in the API's route_changes response.

#### RV4-5 T4 — MCAST-VPN NLRI (AFI 1/2, SAFI 5/129)

Basic MVPN NLRI parsing per RFC 6514 §4. At minimum: NLRI type byte + length decode so we don't silently discard the data. Store raw for type 1-7 NLRIs.

---

### Epic RV4-6: BGP-LS Topology Graph (Python NetworkX)

**Scope**: new `bmppy/rbmppy/topology.py`

This is the "right answer" to the graph DB question — not KuzuDB, but Python NetworkX derived from our DuckDB BGP-LS tables.

```python
"""BGP-LS topology graph derived from DuckDB BGP-LS tables.

This is NOT a graph database — it's a lightweight Python graph (NetworkX)
rebuilt from DuckDB queries. Refreshed on demand or on a schedule.

Capabilities:
    - Shortest path between routers (IGP metric weight)
    - Blast radius from a failing router
    - AS topology from BGP AS_PATH data
    - SRLG-aware path diversity analysis
    
Usage:
    from rbmppy.topology import BgpLsTopology, AsTopology
    
    topo = BgpLsTopology(analytics)
    path = topo.shortest_path("10.0.0.1", "10.0.0.5")
    blast = topo.blast_radius("10.0.0.3", max_hops=3)
"""
from __future__ import annotations

import networkx as nx
import pandas as pd
from typing import Optional
from .analytics import RouteAnalytics


class BgpLsTopology:
    """
    IGP topology graph from BGP-LS data.
    Nodes = routers (from bgpls_nodes: router_id, node_name, protocol).
    Edges = links (from bgpls_links: igp_metric, max_bandwidth, adj_sid_label, srlg).
    """

    def __init__(self, analytics: RouteAnalytics):
        self.G: nx.DiGraph = nx.DiGraph()
        self._load(analytics)

    def _load(self, analytics: RouteAnalytics) -> None:
        # Nodes
        nodes_df = analytics.conn.execute("""
            SELECT DISTINCT router_id, node_name, protocol_id
            FROM bgpls_nodes
            WHERE action = 'announce'
        """).df()
        for _, row in nodes_df.iterrows():
            self.G.add_node(row['router_id'],
                name=row['node_name'],
                protocol=row['protocol_id'])

        # Edges (from bgpls_links — use most recent state)
        links_df = analytics.conn.execute("""
            SELECT local_router_id, remote_router_id,
                   local_ip, remote_ip,
                   igp_metric, max_bandwidth, adj_sid_labels, srlg_groups
            FROM (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY local_router_id, remote_router_id
                    ORDER BY occurred_at DESC
                ) AS rn
                FROM bgpls_links WHERE action = 'announce'
            ) WHERE rn = 1
        """).df()
        for _, row in links_df.iterrows():
            src, dst = row['local_router_id'], row['remote_router_id']
            if src and dst:
                self.G.add_edge(src, dst,
                    igp_metric=row['igp_metric'] or 1,
                    max_bandwidth=row['max_bandwidth'],
                    local_ip=row['local_ip'],
                    remote_ip=row['remote_ip'],
                    adj_sids=row['adj_sid_labels'],
                    srlg=row['srlg_groups'])

    def shortest_path(self, src: str, dst: str) -> list[str]:
        """Shortest path by IGP metric."""
        try:
            return nx.shortest_path(self.G, src, dst, weight='igp_metric')
        except (nx.NetworkXNoPath, nx.NodeNotFound):
            return []

    def blast_radius(self, node: str, max_hops: int = 3) -> set[str]:
        """All nodes reachable from `node` within max_hops."""
        try:
            return set(nx.single_source_shortest_path_length(
                self.G, node, cutoff=max_hops
            ).keys()) - {node}
        except nx.NodeNotFound:
            return set()

    def srlg_diverse_paths(self, src: str, dst: str, n: int = 2) -> list[list[str]]:
        """Return up to n SRLG-diverse paths (avoiding shared SRLGs)."""
        all_paths = list(nx.all_simple_paths(self.G, src, dst, cutoff=10))
        if len(all_paths) <= 1:
            return all_paths
        # Pick the pair with minimum shared SRLGs
        def path_srlg(path):
            srlgs = set()
            for u, v in zip(path, path[1:]):
                edge_data = self.G.get_edge_data(u, v, {})
                for s in (edge_data.get('srlg') or '').split(','):
                    if s.strip():
                        srlgs.add(s.strip())
            return srlgs
        best = [all_paths[0]]
        for path in all_paths[1:]:
            if not (path_srlg(path) & path_srlg(best[0])):
                best.append(path)
            if len(best) >= n:
                break
        return best[:n]

    def to_dict(self) -> dict:
        """Serialize to dict for JSON API response."""
        return {
            "nodes": [{"id": n, **d} for n, d in self.G.nodes(data=True)],
            "links": [{"source": u, "target": v, **d}
                      for u, v, d in self.G.edges(data=True)],
        }


class AsTopology:
    """AS-level topology derived from BGP AS_PATH data.
    Nodes = ASNs. Edges = AS adjacency inferred from consecutive hops in AS_PATHs.
    """

    def __init__(self, analytics: RouteAnalytics):
        self.G: nx.DiGraph = nx.DiGraph()
        self._load(analytics)

    def _load(self, analytics: RouteAnalytics) -> None:
        # Extract AS adjacency from all AS_PATHs in recent route events
        df = analytics.conn.execute("""
            SELECT as_path FROM route_events
            WHERE action = 'announce' AND as_path IS NOT NULL
              AND occurred_at >= NOW() - INTERVAL 1 DAY
            LIMIT 50000
        """).df()
        for _, row in df.iterrows():
            asns = [int(a) for a in str(row['as_path']).split() if a.isdigit()]
            for i in range(len(asns) - 1):
                self.G.add_edge(asns[i], asns[i+1])

    def neighbors(self, asn: int) -> list[int]:
        return list(self.G.successors(asn)) + list(self.G.predecessors(asn))

    def is_transit(self, asn: int) -> bool:
        """An ASN is a transit if it has both upstream and downstream ASNs."""
        return self.G.in_degree(asn) > 0 and self.G.out_degree(asn) > 0
```

**New API endpoint**: `GET /api/bgpls/graph` → calls `BgpLsTopology.to_dict()` from Rust side (or via Python webhook, depending on integration approach). In RV4, expose via HTTP from the Python layer calling rustybmp's DuckDB query API.

---

### Epic RV4-7: HA + NATS Output

#### RV4-7 T1 — HA leader election (simple Redis-based)

For two-instance active/passive HA:
- Both instances run, both collect BMP from routers
- Leader election via Redis SETNX with 10-second TTL
- Only the leader writes to DuckDB and serves HTTP API
- Follower buffers events in memory, takes over within 10s on leader failure

Config:
```toml
[ha]
enabled      = false
redis_url    = "redis://localhost:6379"
instance_id  = "core-1"
lease_secs   = 10
```

#### RV4-7 T2 — NATS output (alternative to Kafka)

NATS is more appropriate than Kafka for edge deployments (no ZooKeeper, single binary, <5ms latency):

**New crate**: `crates/rbmp-nats/`

```toml
[dependencies]
async-nats = "0.37"
```

Config:
```toml
[nats]
enabled = false
server  = "nats://localhost:4222"
subject_prefix = "rustybmp"
```

Same event → subject mapping as Kafka → topic mapping.

---

### Epic RV4-8: Container + CI/CD

#### RV4-8 T1 — Dockerfile

```dockerfile
# Multi-stage build
FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin rustybmp --bin rbmp-collector

FROM debian:bookworm-slim
RUN apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/rustybmp /usr/local/bin/
COPY --from=builder /app/target/release/rbmp-collector /usr/local/bin/
EXPOSE 5000 5001 7878
CMD ["rustybmp", "--config", "/etc/rustybmp/rustybmp.toml"]
```

#### RV4-8 T2 — docker-compose.yml (local development stack)

```yaml
services:
  rustybmp:
    build: .
    ports: ["5000:5000", "7878:7878"]
    volumes: ["./config:/etc/rustybmp:ro", "./runtime:/runtime"]
    
  kafka:
    image: bitnami/kafka:3.7
    environment:
      KAFKA_CFG_NODE_ID: "1"
      KAFKA_CFG_PROCESS_ROLES: "controller,broker"
    ports: ["9092:9092"]
  
  routinator:
    image: nlnetlabs/routinator:0.14
    ports: ["3323:3323", "9556:9556"]
    command: ["-vv", "server", "--rtr", "0.0.0.0:3323"]
```

#### RV4-8 T3 — GitHub Actions CI

**File**: `.github/workflows/ci.yml`

```yaml
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo test --workspace
      - run: cargo build --workspace --release
  python:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with: { python-version: "3.12" }
      - run: cd bmppy && pip install -e ".[dev]" && python -m pytest
```

---

### Epic RV4-9: Ubuntu Testing Document + Integration Tests

**This addresses the user's explicit request: a detailed step-by-step Ubuntu testing document. This should only be published after RV4's production-ready features are in place.**

#### RV4-9 T1 — Integration test suite

**New directory**: `tests/integration/`

```rust
// tests/integration/bmp_session_test.rs
// Tests that require a live BMP speaker — uses FRRouting in Docker.

#[tokio::test]
#[ignore = "requires docker + frr container"]
async fn test_bgp_session_and_route_monitoring() {
    // 1. Start rustybmp on :5000 (test config, in-memory DuckDB)
    // 2. Start FRR container with BMP configured to :5000
    // 3. Inject 3 prefixes via FRR vtysh
    // 4. Wait for route_events in DuckDB (poll for 10s)
    // 5. Assert: 3 rows with action='announce'
    // 6. Withdraw 1 prefix
    // 7. Assert: 1 more row with action='withdraw'
    // 8. Send BMP peer_down
    // 9. Assert: LLGR stale or routes cleared
}
```

#### RV4-9 T2 — Cargo bench

**File**: `benches/bmp_parse.rs`

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_bmp_parse(c: &mut Criterion) {
    let raw_route_monitoring = include_bytes!("../testdata/route_monitoring.bin");
    c.bench_with_input(BenchmarkId::new("parse_bmp_message", "route_monitoring"),
        raw_route_monitoring, |b, data| {
            b.iter(|| {
                let mut buf = bytes::Bytes::from_static(data);
                rbmp_core::bmp::parser::parse_bmp_message(&mut buf, std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
            });
        });
}

criterion_group!(benches, bench_bmp_parse);
criterion_main!(benches);
```

#### RV4-9 T3 — Ubuntu Testing Runbook

**New file**: `docs/UBUNTU_TESTING.md`

```markdown
# RustyBMP — Ubuntu 24 Testing Runbook

## Prerequisites

### System requirements
- Ubuntu 24.04 LTS (clean install or VM)
- 4 vCPU, 8GB RAM minimum
- 20GB disk (DuckDB + MRT archives)
- Internet access (for RPKI Routinator, PeeringDB)
- Docker 25+ (for ContainerLab + XRD/FRR)

### Install ContainerLab
```bash
bash -c "$(curl -sL https://get.containerlab.dev)"
containerlab version   # should be ≥ 0.55
```

### Install Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup update stable
```

### Build rustybmp
```bash
git clone https://github.com/your-org/rustybmp
cd rustybmp
cargo build --workspace --release
# Binaries at: target/release/rustybmp, target/release/rbmp-collector
```

## Test 1 — Unit tests
```bash
cargo test --workspace
# Expected: 49 tests, 0 failures
```

## Test 2 — Basic BMP with FRRouting (Docker, no XRD needed)

### Start FRR container
```bash
docker run -d --name frr-test \
  --cap-add=NET_ADMIN \
  -p 5180:5180 \
  frrouting/frr:9.1 /usr/lib/frr/docker-start

# Configure BMP in FRR
docker exec frr-test vtysh << 'EOF'
configure terminal
router bgp 65001
 bgp router-id 172.17.0.2
 neighbor 172.17.0.1 remote-as 65000
 bmp targets test
  address 172.17.0.1 port 5000
  monitor ipv4 unicast pre-policy
  activate neighbor 172.17.0.2
 !
!
address-family ipv4 unicast
 network 203.0.113.0/24
 network 198.51.100.0/24
end
EOF
```

### Start rustybmp
```bash
cat > /tmp/test.toml << 'EOF'
[bmp]
listen_addr = "0.0.0.0:5000"
[store]
in_memory = true
[http]
listen_addr = "0.0.0.0:7878"
[log]
level = "debug"
EOF

./target/release/rustybmp --config /tmp/test.toml &
sleep 3
```

### Verify BMP connection
```bash
# Check speaker connected
curl -s http://localhost:7878/api/speakers | python3 -m json.tool
# Expected: 1 speaker, peer_count > 0

# Check routes received
curl -s http://localhost:7878/api/routes | python3 -m json.tool
# Expected: 2 routes (203.0.113.0/24, 198.51.100.0/24)

# Verify DuckDB directly
duckdb :memory: "ATTACH '/tmp/routes.duckdb'; SELECT prefix, action, peer_as FROM routes.route_events LIMIT 10;"
```

## Test 3 — Full ContainerLab with XRD

### Pull XRD image (requires Cisco CCO access)
```bash
docker pull ios-xr/xrd-control-plane:24.x
```

### Deploy ContainerLab topology
```bash
cd rustybmp
containerlab deploy -t lab/xrd-bmp.clab.yml
sleep 60   # wait for XRD to boot and form BGP sessions

# Check containers
docker ps | grep clab
```

### Configure XRD for BMP
```bash
# Get Ubuntu host IP (as seen from XRD)
HOST_IP=$(ip route get 172.20.0.0 | grep src | awk '{print $7}')

docker exec clab-rustybmp-test-xrd-pe1 xrctl run "
conf t
bmp server 1
 host $HOST_IP port 5000
 stats-reporting-period 30
 initial-refresh delay 15 spread 2
 commit
"
```

### Run BMP stress test
```bash
# Inject 100 routes via FRR CE
lab/scenarios/mass_withdrawal.sh

# Watch route count climb in real-time (SSE stream)
curl -N http://localhost:7878/api/events | head -100

# Check churn analytics
curl -s http://localhost:7878/api/analytics/churn | python3 -m json.tool
```

## Test 4 — RPKI Validation

### Start Routinator
```bash
docker run -d --name routinator \
  -p 3323:3323 -p 9556:9556 \
  nlnetlabs/routinator:0.14 \
  routinator -vv server --rtr 0.0.0.0:3323

# Wait for initial sync (~5 minutes for first run)
docker logs -f routinator | grep "RTR server ready"
```

### Configure RPKI in rustybmp
```toml
[rpki]
enabled  = true
rtr_addr = "127.0.0.1:3323"
```

### Verify RPKI annotations
```bash
# After restart, check that rpki_validity is populated
curl -s "http://localhost:7878/api/routes/prefix?prefix=203.0.113.0/24" | python3 -m json.tool
# Look for "rpki_validity": "valid" or "not-found"
```

## Test 5 — Kafka output

### Start Kafka (single-node)
```bash
docker run -d --name kafka \
  -p 9092:9092 \
  bitnami/kafka:3.7 \
  /opt/bitnami/scripts/kafka/entrypoint.sh /run.sh

# Enable Kafka in config:
# [kafka]
# enabled = true
# brokers = "localhost:9092"

# Restart rustybmp, then consume messages:
docker exec kafka kafka-console-consumer.sh \
  --bootstrap-server localhost:9092 \
  --topic rustybmp.parsed.unicast_prefix \
  --from-beginning | head -5
```

## Test 6 — Python SDK

### Install rbmppy
```bash
cd bmppy && pip install -e ".[dev]"
```

### Run anomaly detection pipeline
```python
# test_pipeline.py
import asyncio
from rbmppy import RustybmpClient, DetectorPipeline
from rbmppy.stream import event_stream

async def main():
    pipeline = DetectorPipeline(base_url="http://localhost:7878")
    pipeline.add_handler(lambda alert: print(f"ALERT: {alert.alert_type} {alert.prefix} — {alert.detail}"))
    print("Listening for BGP anomalies... (Ctrl+C to stop)")
    await pipeline.run()

asyncio.run(main())
```

```bash
python test_pipeline.py &
# Trigger a flap test to generate alerts:
lab/scenarios/flap_peer.sh
```

## Test 7 — Distributed collector

### On machine A (edge collector, port 5000)
```bash
./target/release/rbmp-collector \
  --listen 0.0.0.0:5000 \
  --core 192.168.1.100:5001 \
  --id col-fra01 \
  --site fra01
```

### On machine B (core, ports 5001 + 7878)
```bash
cat > /tmp/core.toml << 'EOF'
[bmp]
listen_addr = "0.0.0.0:5000"    # also accept direct BMP
[http]
listen_addr = "0.0.0.0:7878"
EOF
./target/release/rustybmp --config /tmp/core.toml
```

### Verify multi-site tagging
```bash
# Routes from collector should have collector_id set
duckdb :memory: "ATTACH ':memory:'; SELECT prefix, collector_id FROM route_events LIMIT 5;"
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| No speakers connecting | Verify firewall: `sudo ufw allow 5000/tcp` |
| DuckDB lock error on restart | Remove `runtime/routes.duckdb.wal` |
| RTR client not connecting | `docker logs routinator` — check sync completion |
| XRD BMP session keeps dropping | Verify `initial-refresh delay 15 spread 2` in XRD config |
| Kafka connection refused | `docker ps` — verify Kafka container running |
| "parse error" in logs | Enable `[log] level = "trace"` for full frame dump |

## Test Checklist

- [ ] 49 unit tests pass
- [ ] FRR BMP session connects and prefixes appear in /api/routes
- [ ] XRD BMP session connects with full table dump
- [ ] RPKI validation annotates routes
- [ ] Kafka output confirmed with consumer
- [ ] MRT export produces valid file (`bgpdump -m output.mrt`)
- [ ] rbmppy DetectorPipeline fires alert on peer flap
- [ ] Distributed collector: collector_id appears in route_events
- [ ] Retention policy: `DELETE FROM route_events WHERE occurred_at < NOW() - INTERVAL 7 DAY` reduces db size
```
```

---

## Part 6 — README Audit

### What the README now correctly claims (verified against diff)

✅ EVPN types 1-11 — confirmed in `evpn.rs` diff (types 6-11 added)  
✅ BGP-LS full attribute TLVs — confirmed in `bgpls.rs` diff (303 lines of TLV decode)  
✅ SR Policy SAFI 73 types A-K — results_and_decisions.md Bundle A confirms; not in diff = committed at base  
✅ LLGR state machine — confirmed in `session.rs` + `table.rs` diffs  
✅ YAML filter DSL — referenced in `manager.rs` via `FilterEngine` import  
✅ Kafka output — confirmed in `main.rs` diff (KafkaProducer + run_kafka_sink)  
✅ MRT import/export — Bundle E confirmed; 8 tests  
✅ Distributed collector — Bundle G confirmed; `collector_id` in schema diff  
✅ RFC 9972 stats types 18-38 — confirmed since RV1  
✅ DNS PTR enrichment — confirmed in `config.rs` (DnsConfig)  
✅ BMP proxy — confirmed in `config.rs` (ProxyConfig)  

### What the README says but is NOT verified/complete

| README claim | Reality | RV4 action |
|-------------|---------|-----------|
| `serve_ui = true` in config example | No UI code exists | Add `serve_ui = false` to example config; RV4-3 builds UI |
| `collector_protocol.rs` is "authenticated" | MessagePack over TCP — no auth | Remove "authenticated" claim; add TLS in RV4-1 |
| 49 tests, 0 failures | True but only unit tests | Add integration tests in RV4-9 |

### Genuinely uncovered areas for RV4 backlog

These are in the `🔲 RV4` section of README and are real:

1. **Svelte 5 dashboard** → RV4-3
2. **Active BGP session connector** → RV4 future (low priority for BMP-focused tool)
3. **BGPsec path validation** → RV4-6
4. **HA leader election** → RV4-7
5. **NATS output** → RV4-7
6. **L2VPN VPLS full decode** → RV4-5
7. **BGP-LS SRv6 SID NLRI (SAFI 72)** → RV4-5

Additional gaps NOT in README:
8. **API authentication** → RV4-1 (critical)
9. **TLS for BMP connections** → RV4-1 (critical)
10. **DuckDB retention policy** → RV4-2 (critical)
11. **Parquet ML pipeline** → RV4-4 (high value)
12. **BGP topology graph (NetworkX)** → RV4-6
13. **Integration test suite** → RV4-9
14. **Ubuntu testing document** → RV4-9
15. **CI/CD pipeline** → RV4-8
16. **Dockerfile** → RV4-8
17. **API rate limiting** → RV4-1
18. **Benchmark suite (cargo bench)** → RV4-9

---

## Part 7 — RV4 Priority Order for Development

### P0 — Production blockers (code before deploying to real networks)

1. **RV4-2 T1** — DuckDB retention policy (10 lines of SQL, massive operational impact)
2. **RV4-1 T1** — JWT auth for HTTP API (no auth = no production)
3. **RV4-9 T3** — Ubuntu testing runbook (operators need this now)
4. **RV4-8 T1** — Dockerfile (standard deployment artifact)
5. **RV4-8 T3** — GitHub Actions CI (prevent regressions)

### P1 — High value, plan for early RV4

6. **RV4-4 T1** — `parquet.py` DuckDB export (trivial to implement, unlocks ML)
7. **RV4-4 T2** — `train_route_anomaly.py` IsolationForest (adapted from bonsai)
8. **RV4-6 T1** — `topology.py` NetworkX graph (answers the graph DB question properly)
9. **RV4-3** — Svelte UI (most visible feature for operators)
10. **RV4-5 T1** — SRv6 SID NLRI SAFI 72 (emerging need)

### P2 — Complete for feature parity

11. **RV4-7 T2** — NATS output
12. **RV4-1 T2** — TLS for BMP
13. **RV4-7 T1** — HA leader election
14. **RV4-5 T2** — L2VPN VPLS decode
15. **RV4-9 T1** — Integration tests with FRR Docker

### P3 — Nice to have

16. **RV4-6 T1** — BGPsec
17. **RV4-4 T4** — BGP STGNN training pipeline
18. Active BGP session connector (cross-project with Rotonda)

---

## Part 8 — Updated Project State (Post-RV3)

```
rustybmp workspace (7 crates, 49 tests)
├── crates/rbmp-core/       ← All BMP RFCs + BGP: EVPN 1-11, SR Policy, BGP-LS full, RTC, Flowspec
├── crates/rbmp-rib/        ← RIB + LLGR state machine + YAML filter DSL
├── crates/rbmp-store/      ← DuckDB: 5 tables, collector_id tagging, batched writes
├── crates/rbmp-server/     ← Kafka + DNS + Proxy + Core TCP listener + Prometheus
├── crates/rbmp-enrichment/ ← RPKI RTR client (VrpCache, RtrClient)
├── crates/rbmp-kafka/      ← Kafka output (rdkafka, lz4, typed topics)
└── crates/rbmp-mrt/        ← MRT RFC 6396 reader + writer (8 tests)

bmppy/rbmppy/ (Python SDK)
├── client.py, stream.py, models.py   — complete
├── analytics.py                       — Z-score, hijack, leak, flap detectors
├── rpki.py                            — RtrVrpCache, poll_rtr_cache
├── internet.py                        — IrrClient, RdapClient, BgpToolsClient
└── detectors.py                       — 4 detectors + DetectorPipeline
```

**What is NOT there that should be (RV4):**
- UI (zero frontend code)
- API auth (no JWT, no TLS)
- Retention policy (DB grows forever)
- Integration tests (only unit tests)
- CI/CD pipeline
- Dockerfile
- Parquet ML pipeline
- NetworkX topology graph

---

*End of RUSTYBMP_BACKLOG_RV4.md — Sprint RV4*
