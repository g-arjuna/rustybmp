# RustyBMP — Sprint RV7 Backlog
## Roto JIT · Path Status TLV · SSH Policy Fetch · BGPsec · Scale

> **Version**: RV7
> **Date**: 2026-06-20
> **Basis**: Full RV6 diff analysis (77 tests, 0 warnings, 0 npm errors) +
> deep conversation analysis across:
> — Thomas Graf's BMP article (counters vs gauges, Path Marking TLV, Type 30)
> — draft-ietf-grow-bmp-path-marking-tlv-05 (May 2026, full wire format read)
> — RFC 9972 §4 (per-status counters TBD1-TBD14)
> — Roto v0.11 documentation (cranelift JIT, hot-reload, embedding API)
> — Bonsai credentials.rs + managed_devices.rs + bootstrap_agent.py (full read)
> — PyATS/Genie/TextFSM/Batfish ecosystem analysis
> — SP/AI-DC operator requirements (Kentik, ThousandEyes, NLNOG research)
>
> **Decision D15 from RV6**: RouteCtx scaffold committed; Roto embed deferred to RV7.
> **Decision D16 from RV6**: YAML DSL kept alongside RouteCtx scaffold through RV7 cutover.

---

## Part 1 — RV6 Completion Audit

### What RV6 delivered (confirmed from diff + results_and_decisions.md)

| Bundle | Status | Notes |
|--------|--------|-------|
| RV6-1 Filter Engine | ✅ | `filter_reload/test/stats` endpoints; `RouteCtx` + `roto_ctx.rs` scaffold; YAML DSL intact |
| RV6-2 Protocol | ✅ | ASPA RFC 9319 `validate_as_path()` + tests; MCAST-VPN RFC 6514 types 1-7 (`bgp/mvpn.rs`); BGPsec_Path parse (type 30 attribute, stores raw signature blocks); SRv6 uSID scaffold |
| RV6-3 UI Components | ✅ | `TimelineChart.svelte` (D3), `AsnSankey.svelte` (d3-sankey), `VirtualTable.svelte`, `MetricCard.svelte`, `RpkiBadge.svelte`, `sse.ts` (RAF batch + reconnect) |
| RV6-4 Schema | ✅ | `srpolicy_events`, `aspa_validations` tables; composite indexes on `(prefix, occurred_at)` and `(peer_addr, counter_name, occurred_at)` |
| RV6-5 API | ✅ | 18 new endpoints: aspath_graph, bmpstats_history, srpolicy, peer_capabilities, rpki_coverage, bgpls_path, ml_model_status, filter CRUD |
| RV6-6 UI Pages | ✅ | 4 new: `/filters`, `/srpolicy`, `/bgpls-path`, `/rpki-coverage`; 5 upgraded: aspath (Sankey+cards), ml (model status), stats (history), peers/[addr] ($derived fix), dashboard (typed API) |
| RV6-7 Quality Gate | ✅ | `cargo build --workspace` 0 warnings (18 files); `npm run check` 0 errors (60→0); 77 tests pass |

### What RV6 explicitly deferred (README 🔲 RV7 section)

1. **Embed Roto v0.11** — RouteCtx scaffold ready, JIT embed deferred (D15)
2. **BGPsec full cryptographic validation** — parse is done (type 30), validation needs router certs
3. **Topology level-of-detail >500 nodes** — force simulation freezes above ~500

### What was designed but not yet implemented (from conversations)

| Topic | Designed where | Implementation status |
|-------|---------------|----------------------|
| Path Status TLV parsing | Conversation analysis + draft-05 full read | ❌ Not started |
| RFC 9972 Type 30 trend analytics | Thomas Graf article analysis | ❌ Not started |
| Path Pipeline UI visualization | Deep visual design with diagrams | ❌ Not started |
| Redundancy Health Matrix UI | Deep visual design | ❌ Not started |
| Max-prefix Fuel Gauge UI | Deep visual design | ❌ Not started |
| BGP Convergence Timeline | Deep visual design | ❌ Not started |
| SSH policy fetching (bonsai vault) | Full bonsai code read + architecture | ❌ Not started |
| `policy_fetcher.py` | Full implementation designed | ❌ Not started |
| `rbmppy/policy/` parser ecosystem | Genie/TextFSM/Batfish architecture | ❌ Not started |
| BGP convergence event detection | Article analysis | ❌ Not started |

---

## Part 2 — RV7 Theme 1: Embed Roto as Live Filter Engine

### 2.1 Why now

RouteCtx is scaffolded in `crates/rbmp-rib/src/roto_ctx.rs`. Roto v0.11 is the current stable release (January 2026). The cranelift JIT API has stabilised across the 0.7-0.11 series. D15 said "before Roto embed would add build-time complexity" — that risk is now manageable given the scaffold is already accepted.

The practical gap today: operators can configure the YAML DSL but cannot write expressions like `as_path_len > 20 AND rpki == 'invalid'`. Every SP operator we researched uses some form of prefix-length + RPKI combined filters, which our current YAML cannot express without two separate filter entries.

### 2.2 Implementation

#### RV7-F T1 — Add Roto to rbmp-rib

```toml
# crates/rbmp-rib/Cargo.toml
roto = "0.11"
```

Roto v0.11 brings cranelift as an optional feature. Enable it:
```toml
roto = { version = "0.11", features = ["cranelift"] }
```

#### RV7-F T2 — Register BGP types with Roto runtime

**File**: `crates/rbmp-rib/src/roto_ctx.rs` (scaffold already exists — expand it)

```rust
use roto::{Runtime, Compiler};

/// Build a Roto Runtime with all BGP route fields registered.
/// The Runtime is cheap to clone; each thread holds one reference.
pub fn build_roto_runtime() -> roto::Runtime {
    let mut rt = Runtime::new();

    // Register RouteCtx as a Roto type — all fields become accessible
    // in Roto scripts as `route.prefix`, `route.rpki`, etc.
    rt.register_type::<RouteCtx>("RouteCtx")
        .field("prefix",         |r: &RouteCtx| r.prefix.clone())
        .field("prefix_len",     |r: &RouteCtx| r.prefix_len)
        .field("afi",            |r: &RouteCtx| r.afi.clone())
        .field("peer_as",        |r: &RouteCtx| r.peer_as)
        .field("peer_addr",      |r: &RouteCtx| r.peer_addr.clone())
        .field("rib_type",       |r: &RouteCtx| r.rib_type.clone())
        .field("action",         |r: &RouteCtx| r.action.clone())
        .field("as_path",        |r: &RouteCtx| r.as_path.clone())
        .field("as_path_len",    |r: &RouteCtx| r.as_path_len)
        .field("origin_asn",     |r: &RouteCtx| r.origin_asn)
        .field("has_prepend",    |r: &RouteCtx| r.has_prepend)
        .field("next_hop",       |r: &RouteCtx| r.next_hop.clone())
        .field("local_pref",     |r: &RouteCtx| r.local_pref)
        .field("med",            |r: &RouteCtx| r.med)
        .field("rpki",           |r: &RouteCtx| r.rpki.clone())
        .field("otc_asn",        |r: &RouteCtx| r.otc_asn)
        .field("is_evpn",        |r: &RouteCtx| r.is_evpn)
        .field("is_srpolicy",    |r: &RouteCtx| r.is_srpolicy)
        .field("evpn_type",      |r: &RouteCtx| r.evpn_type)
        .field("aspa_verdict",   |r: &RouteCtx| r.aspa_verdict.clone());

    // Helper functions callable from Roto
    rt.register_function("community_has",
        |ctx: &RouteCtx, c: String| -> bool {
            ctx.communities.contains(&c)
        }
    );
    rt.register_function("as_path_contains",
        |ctx: &RouteCtx, asn: u32| -> bool {
            ctx.as_path.split_whitespace()
               .any(|s| s.parse::<u32>().ok() == Some(asn))
        }
    );
    rt.register_function("prefix_in_range",
        |ctx: &RouteCtx, cidr: String| -> bool {
            if let (Ok(target), Ok(route)) = (
                cidr.parse::<ipnet::IpNet>(),
                ctx.prefix.parse::<ipnet::IpNet>(),
            ) {
                route.subnet_of(&target) || route == target
            } else { false }
        }
    );
    rt
}
```

#### RV7-F T3 — RotoFilterEngine replaces FilterEngine in filter.rs

```rust
// crates/rbmp-rib/src/filter.rs

pub struct RotoFilterEngine {
    runtime:  roto::Runtime,
    script:   Option<roto::CompiledScript>,
    path:     String,            // .roto file path for hot-reload
    stats:    Arc<FilterStats>,
}

impl RotoFilterEngine {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let rt     = build_roto_runtime();
        let source = std::fs::read_to_string(path)?;
        let script = roto::Compiler::new(&rt).compile(&source)
            .map_err(|e| anyhow::anyhow!("Roto compile error in {path}: {e}"))?;
        info!(path, "Roto filter JIT-compiled via cranelift");
        Ok(Self { runtime: rt, script: Some(script), path: path.to_string(),
                  stats: Arc::new(FilterStats::default()) })
    }

    /// Hot-reload: recompile from file. Keeps current script on failure.
    pub fn reload(&mut self) -> anyhow::Result<bool> {
        let source = std::fs::read_to_string(&self.path)?;
        match roto::Compiler::new(&self.runtime).compile(&source) {
            Ok(new_script) => {
                self.script = Some(new_script);
                info!(path = %self.path, "Roto filter hot-reloaded (JIT)");
                Ok(true)
            }
            Err(e) => {
                warn!(%e, "Roto reload failed — retaining current filter");
                Ok(false)
            }
        }
    }

    pub fn evaluate(&self, ctx: &RouteCtx) -> FilterVerdict {
        let script = match &self.script {
            Some(s) => s,
            None    => return FilterVerdict::Accept,
        };
        let t0 = std::time::Instant::now();
        let verdict = match script.call::<RouteCtx, bool>("bgp_filter", ctx) {
            Ok(true)  => FilterVerdict::Accept,
            Ok(false) => FilterVerdict::Reject,
            Err(e)    => { warn!(%e, "Roto eval error — accepting route"); FilterVerdict::Accept }
        };
        self.stats.record(verdict, t0.elapsed());
        verdict
    }
}
```

#### RV7-F T4 — Default `config/filters.roto`

The shipped default must cover the four operator-universal policies: bogons, RPKI invalid + too-specific (likely hijack), OTC route leak, and blackhole community:

```roto
# rustybmp default BGP filter — Roto scripting language (JIT via cranelift)
# See: https://roto.docs.nlnetlabs.nl/
#
# Available route context fields:
#   route.prefix          "203.0.113.0/24"
#   route.prefix_len      24 (u8)
#   route.afi             "ipv4" | "ipv6"
#   route.peer_as         65001 (u32)
#   route.as_path_len     3 (u32)
#   route.origin_asn      64496 (u32)
#   route.has_prepend     true | false
#   route.local_pref      100 (u32)
#   route.med             0 (u32)
#   route.rpki            "valid" | "invalid" | "not-found" | "unknown"
#   route.otc_asn         0 when absent; set to customer ASN when OTC present
#   route.aspa_verdict    "valid" | "invalid" | "unknown"
#   route.is_evpn         true | false
#   route.is_srpolicy     true | false
#   route.evpn_type       0-11 (0 = not EVPN)
#
# Available helpers:
#   community_has(route, "64512:100")
#   as_path_contains(route, 64496)
#   prefix_in_range(route, "10.0.0.0/8")

fn is_bogon(route: RouteCtx) -> bool {
    prefix_in_range(route, "10.0.0.0/8")
    || prefix_in_range(route, "172.16.0.0/12")
    || prefix_in_range(route, "192.168.0.0/16")
    || prefix_in_range(route, "192.0.2.0/24")
    || prefix_in_range(route, "198.51.100.0/24")
    || prefix_in_range(route, "203.0.113.0/24")
    || prefix_in_range(route, "0.0.0.0/8")
    || prefix_in_range(route, "240.0.0.0/4")
    || prefix_in_range(route, "100.64.0.0/10")
    || prefix_in_range(route, "127.0.0.0/8")
}

fn looks_like_hijack(route: RouteCtx) -> bool {
    route.rpki == "invalid" && route.prefix_len > 24
}

fn is_route_leak(route: RouteCtx) -> bool {
    route.otc_asn > 0 && route.rib_type == "adj-out-pre"
}

fn bgp_filter(route: RouteCtx) -> bool {
    if route.action == "withdraw" { return true }
    if is_bogon(route)            { return false }
    if looks_like_hijack(route)   { return false }
    if is_route_leak(route)       { return false }
    if community_has(route, "65535:666") { return false }  # blackhole
    true
}
```

#### RV7-F T5 — Wire inotify hot-reload

**New file**: `crates/rbmp-server/src/filter_watcher.rs`

```rust
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Config};
use std::sync::Arc;
use tokio::sync::RwLock;

pub fn spawn_filter_watcher(
    filter_path: String,
    engine: Arc<RwLock<RotoFilterEngine>>,
) {
    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let mut watcher = RecommendedWatcher::new(
            move |res| { let _ = tx.blocking_send(res); },
            Config::default(),
        ).expect("inotify watcher");
        watcher.watch(std::path::Path::new(&filter_path), RecursiveMode::NonRecursive)
               .expect("watch filter file");
        while let Some(Ok(_event)) = rx.recv().await {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let mut w = engine.write().await;
            match w.reload() {
                Ok(true)  => info!("Filter auto-reloaded on file change"),
                Ok(false) => warn!("Filter parse error on reload — kept previous"),
                Err(e)    => error!(%e, "Filter reload failed"),
            }
        }
    });
}
```

Config addition:
```toml
[filters]
enabled    = true
script     = "config/filters.roto"   # Roto script (JIT compiled)
hot_reload = true                     # watch for file changes
# Legacy YAML fallback (kept until all operators migrate):
yaml       = "config/filters.yaml"
```

#### RV7-F T6 — YAML DSL retirement path

Keep YAML DSL as a read-only fallback. When `script` is configured and compiles successfully, Roto takes precedence. If the Roto script fails to compile at startup, fall back to YAML. Log a deprecation warning when YAML is used. This gives operators a migration window without breaking existing deployments.

---

## Part 3 — RV7 Theme 2: Path Status TLV + RFC 9972 Capacity

This is the most technically novel RV7 addition. It comes directly from Thomas Graf's article and the draft-ietf-grow-bmp-path-marking-tlv-05 specification (May 2026).

### 3.1 The operator problem this solves

From the article: "Sometimes you don't need to know all the details. It is good enough if we know wherever the amount of BGP best paths for a given VRF and AFI has reduced or not after the BGP table has converged. If it has, then you might want to know which BGP path is missing."

The Path Status TLV tells rustybmp exactly what the router decided about each path — best, ECMP, backup, non-selected, filtered, stale, suppressed — and critically WHY (which BGP decision step eliminated it). Without the TLV, we infer path states from RIB type comparisons; with it, the router tells us directly.

### 3.2 Wire format (from draft-ietf-grow-bmp-path-marking-tlv-05 §2)

```
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-------------------------------+-------------------------------+
|       Common TLV Header (variable, per bmp-tlv draft)        |
+---------------------------------------------------------------+
|                    Path Status (4 octets bitmap)              |
+-------------------------------+
| Reason Code (2 oct, optional) |
+-------------------------------+
```

Path Status bitmap (Table 1 from draft):
```
0x00000001  Invalid
0x00000002  Best
0x00000004  Nonselected
0x00000008  Primary
0x00000010  Backup
0x00000020  Non-installed
0x00000040  Best-external
0x00000080  Add-Path
0x00000100  Filtered-in-inbound-policy
0x00000200  Filtered-in-outbound-policy
0x00000400  Stale (GR)
0x00000800  Suppressed (RFD)
```

Reason Codes (Table 2 from draft):
```
0x0001  Invalid: AS loop
0x0002  Invalid: unresolvable nexthop
0x0003  Not preferred: LOCAL_PREF
0x0004  Not preferred: AS_PATH length
0x0005  Not preferred: ORIGIN type
0x0006  Not preferred: MED
0x0007  Not preferred: peer type (eBGP > iBGP)
0x0008  Not preferred: IGP metric to nexthop
0x0009  Not preferred: router-ID
0x000A  Not preferred: peer address
0x000B  Not preferred: AIGP
```

Per-status aggregate counters (draft §4, TBD1-TBD14):
One counter per status bit, maintained per BMP session per monitored peer.

### 3.3 Rust parsing

**New file**: `crates/rbmp-core/src/bmp/path_status_tlv.rs`

```rust
/// Parsed Path Status TLV from BMP route-monitoring message.
/// draft-ietf-grow-bmp-path-marking-tlv-05 §2
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct PathStatusTlv {
    /// 4-byte bitmap. Multiple bits may be set simultaneously.
    pub status: u32,
    /// Optional 2-byte reason code (0 = not present).
    pub reason: u16,
}

impl PathStatusTlv {
    // Status bit accessors
    pub fn is_invalid(&self)            -> bool { self.status & 0x0001 != 0 }
    pub fn is_best(&self)               -> bool { self.status & 0x0002 != 0 }
    pub fn is_nonselected(&self)        -> bool { self.status & 0x0004 != 0 }
    pub fn is_primary(&self)            -> bool { self.status & 0x0008 != 0 }
    pub fn is_backup(&self)             -> bool { self.status & 0x0010 != 0 }
    pub fn is_non_installed(&self)      -> bool { self.status & 0x0020 != 0 }
    pub fn is_best_external(&self)      -> bool { self.status & 0x0040 != 0 }
    pub fn is_add_path(&self)           -> bool { self.status & 0x0080 != 0 }
    pub fn is_filtered_inbound(&self)   -> bool { self.status & 0x0100 != 0 }
    pub fn is_filtered_outbound(&self)  -> bool { self.status & 0x0200 != 0 }
    pub fn is_stale(&self)              -> bool { self.status & 0x0400 != 0 }
    pub fn is_suppressed(&self)         -> bool { self.status & 0x0800 != 0 }

    /// Human-readable label for the dominant status.
    pub fn label(&self) -> &'static str {
        if self.is_best()              { return "best" }
        if self.is_primary()           { return "primary" }
        if self.is_backup()            { return "backup" }
        if self.is_best_external()     { return "best-external" }
        if self.is_add_path()          { return "add-path" }
        if self.is_nonselected()       { return "nonselected" }
        if self.is_filtered_inbound()  { return "filtered-inbound" }
        if self.is_filtered_outbound() { return "filtered-outbound" }
        if self.is_stale()             { return "stale" }
        if self.is_suppressed()        { return "suppressed" }
        if self.is_invalid()           { return "invalid" }
        "unknown"
    }

    /// Human-readable reason for non-best selection.
    pub fn reason_label(&self) -> &'static str {
        match self.reason {
            0x0001 => "AS loop",
            0x0002 => "unresolvable nexthop",
            0x0003 => "not preferred: LOCAL_PREF",
            0x0004 => "not preferred: AS_PATH length",
            0x0005 => "not preferred: ORIGIN type",
            0x0006 => "not preferred: MED",
            0x0007 => "not preferred: peer type",
            0x0008 => "not preferred: IGP cost",
            0x0009 => "not preferred: router-ID",
            0x000A => "not preferred: peer address",
            0x000B => "not preferred: AIGP",
            _      => "",
        }
    }
}

/// Parse Path Status TLV from the TLV value bytes (after the common TLV header).
/// Returns None if the data is too short.
pub fn parse_path_status_tlv(data: &[u8]) -> Option<PathStatusTlv> {
    if data.len() < 4 { return None; }
    let status = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let reason = if data.len() >= 6 {
        u16::from_be_bytes([data[4], data[5]])
    } else { 0 };
    Some(PathStatusTlv { status, reason })
}
```

In `crates/rbmp-core/src/bmp/parser.rs`: scan the Route Monitoring message TLVs for the Path Status TLV type code (IANA TBD — use the registered value when assigned, or configurable value for early adoption).

Add to `PathAttributes`:
```rust
pub path_status: Option<PathStatusTlv>,
```

### 3.4 DuckDB storage

```sql
-- New: path_markings table
-- One row per BMP route-monitoring PDU that carries a Path Status TLV.
CREATE TABLE IF NOT EXISTS path_markings (
    occurred_at   TIMESTAMPTZ NOT NULL,
    speaker_addr  VARCHAR     NOT NULL,
    peer_addr     VARCHAR     NOT NULL,
    peer_as       UINTEGER    NOT NULL,
    prefix        VARCHAR     NOT NULL,
    afi           VARCHAR     NOT NULL,
    path_status   UINTEGER    NOT NULL,  -- 4-byte bitmap
    path_reason   USMALLINT   NOT NULL,  -- 2-byte reason code (0 = absent)
    status_label  VARCHAR     NOT NULL,  -- "best" | "backup" | "nonselected" | ...
    reason_label  VARCHAR     NOT NULL,  -- "not preferred: LOCAL_PREF" | ...
    collector_id  VARCHAR
);

CREATE INDEX IF NOT EXISTS idx_path_markings_prefix_peer
ON path_markings (prefix, peer_addr, occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_path_markings_status
ON path_markings (path_status, occurred_at DESC);
```

### 3.5 RFC 9972 Type 30: max-prefix capacity tracking

Type 30 = "Current number of routes in the per-AFI/SAFI in post-policy Adj-RIB-In left before exceeding the received-route threshold." This is stored in `stats_events` already (stats type 30). The gap is the trend analytics.

**New in `rbmppy/rbmppy/analytics.py`**:

```python
def max_prefix_capacity(self, days: int = 7) -> list[dict]:
    """
    Compute max-prefix capacity trend per peer per AFI/SAFI.
    Uses RFC 9972 stats type 30 (routes-left-until-max-prefix).

    Returns per-peer capacity record with:
      - current headroom (latest type-30 value)
      - configured limit (= headroom + current route count)
      - utilization percentage
      - linear regression slope (routes/day added)
      - ETA to limit (days)
    """
    df = self.conn.execute(f"""
        WITH latest AS (
            SELECT peer_addr, afi, safi,
                   LAST(counter_value ORDER BY occurred_at) AS headroom,
                   MAX(occurred_at) AS last_seen
            FROM stats_events
            WHERE counter_type = 30
              AND occurred_at >= NOW() - INTERVAL '{days} days'
            GROUP BY peer_addr, afi, safi
        ),
        routes AS (
            SELECT peer_addr,
                   afi,
                   COUNT(*) FILTER (WHERE action = 'announce') AS route_count
            FROM route_events
            WHERE occurred_at >= NOW() - INTERVAL '1 hour'
            GROUP BY peer_addr, afi
        ),
        trend AS (
            -- Linear regression on headroom over time to get slope
            SELECT peer_addr, afi, safi,
                   REGR_SLOPE(counter_value, EPOCH(occurred_at)) AS slope_per_sec
            FROM stats_events
            WHERE counter_type = 30
              AND occurred_at >= NOW() - INTERVAL '{days} days'
            GROUP BY peer_addr, afi, safi
        )
        SELECT l.peer_addr, l.afi, l.safi,
               l.headroom,
               COALESCE(r.route_count, 0) AS current_routes,
               l.headroom + COALESCE(r.route_count, 0) AS configured_limit,
               COALESCE(r.route_count, 0) * 100.0 /
                   NULLIF(l.headroom + COALESCE(r.route_count, 0), 0) AS utilization_pct,
               t.slope_per_sec * 86400 AS routes_per_day,
               CASE WHEN t.slope_per_sec < 0 THEN
                   l.headroom / ABS(t.slope_per_sec) / 86400
               ELSE NULL END AS days_to_limit,
               l.last_seen
        FROM latest l
        LEFT JOIN routes r ON l.peer_addr = r.peer_addr AND l.afi = r.afi
        LEFT JOIN trend  t ON l.peer_addr = t.peer_addr AND l.afi = t.afi AND l.safi = t.safi
        ORDER BY utilization_pct DESC NULLS LAST
    """).df().to_dict('records')
    return df
```

**New API endpoint**: `GET /api/capacity/maxprefix?days=7`

### 3.6 UI: Three new visualizations

#### Visualization A: Path Pipeline View (`/path-status` page)

Horizontal pipeline per prefix, showing all candidate paths flowing through stages:
- Stage 1: Adj-RIB-In pre-policy (raw arrivals per peer)
- Stage 2: Inbound policy (permit/deny from path_status bit 0x0100)
- Stage 3: BGP decision process (best/ECMP/backup/nonselected with reason)
- Stage 4: Forwarding state (in FIB or not, from primary/non-installed bits)

Color coding (consistent across all pages):
- ★ Emerald bright: Best (0x0002)
- ≡ Emerald soft: Primary/ECMP (0x0008 without 0x0002)
- ↻ Sky blue: Backup (0x0010)
- ⊕ Cyan: Best-external (0x0040)
- ✗ Amber: Nonselected (0x0004)
- ⊘ Red: Filtered-inbound (0x0100) or Invalid (0x0001)
- 💤 Gray: Stale (0x0400)
- ⚡ Purple: Suppressed/RFD (0x0800)

The reason code appears inside the decision box ("not preferred: LOCAL_PREF").

#### Visualization B: Redundancy Health Matrix

A grid where rows = prefixes, columns = peers, cells colored by path status:
```
         Peer A  Peer B  Peer C  Peer D  Peer E
Pfx 1    ★       ≡       ≡       ↻       ✗
Pfx 2    ★       —       ⊘       —       —   ← single path: WARN
Pfx 3    —       ★       ↻       —       —
Pfx 4    ⊘       ✗       —       —       —   ← NO PATHS: CRITICAL
```

Filter control: "Show only prefixes with < 2 active paths" — the most actionable view for on-call operators.

**New API endpoint**: `GET /api/path-status/matrix?afi=ipv4&min_active_paths=1&limit=1000`

This query leverages the `path_markings` table:
```sql
SELECT prefix, peer_addr, path_status, status_label,
       FIRST_VALUE(path_reason) OVER (
           PARTITION BY prefix, peer_addr
           ORDER BY occurred_at DESC
       ) AS latest_reason
FROM path_markings
WHERE occurred_at >= NOW() - INTERVAL '5 minutes'
  AND afi = 'ipv4'
QUALIFY ROW_NUMBER() OVER (PARTITION BY prefix, peer_addr ORDER BY occurred_at DESC) = 1
```

#### Visualization C: Max-prefix Fuel Gauge Dashboard

```
Max-prefix Capacity                             [Refresh] [7d | 30d]

Peer / AFI-SAFI          Headroom gauge          Used%   Trend    ETA
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
AS65001 IPv4 UC    [████████████████████░░░░]   84.7%   ↑+3/d    51d
AS65001 IPv6 UC    [███░░░░░░░░░░░░░░░░░░░░░]   12.0%   →stable   —
AS65003 IPv4 UC ⚠  [████████████████████████]   96.0%   ↑+2/d    4d
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
⚠ AS65003 will exhaust max-prefix in 4 days. Recommend increasing to 300.
```

Colors: green < 70%, amber 70-90%, red > 90%.

---

## Part 4 — RV7 Theme 3: SSH Policy Fetch (Bonsai Vault Reuse)

### 4.1 What to copy verbatim

**`crates/rbmp-enrichment/src/vault.rs`** — copy `bonsai/src/credentials.rs` with exactly two changes:
1. `BONSAI_VAULT_PASSPHRASE` → `RUSTYBMP_VAULT_PASSPHRASE`
2. Add `ResolvePurpose::SshFetch` to the enum:

```rust
pub enum ResolvePurpose {
    Subscribe, Remediate, Discover, Enrich, Test, Internal,
    SshFetch,       // ← new: SSH into router to fetch policy config
    Other(String),
}
```

Everything else (age encryption, HMAC-SHA256 integrity, atomic disk writes, zeroize on drop, debounced last_used_at_ns, audit logging to `bonsai.audit.credentials`) — unchanged. The 3 unit tests (round-trip, wrong-passphrase rejection, debounce) — copy them verbatim, adjust the env var name.

Dependencies to add to `rbmp-enrichment/Cargo.toml`:
```toml
age     = { version = "0.10", features = ["scrypt"] }
hmac    = "0.12"
sha2    = "0.10"
zeroize = { version = "1", features = ["derive"] }
```

Wire `Arc<CredentialVault>` into `AppState`:
```rust
pub struct AppState {
    // ... existing fields ...
    pub vault: Arc<CredentialVault>,
}
```

Credential CRUD API endpoints (copy from bonsai managed_devices.rs):
```
GET    /api/credentials                  list all (alias, created_at, last_used_at, device_count)
POST   /api/credentials/add              {alias, username, password}
POST   /api/credentials/update           {alias, username, password}
POST   /api/credentials/remove           {alias}
POST   /api/credentials/test             {alias, address} → SSH connectivity test
```

### 4.2 The Rust policy fetch handler

**New file**: `crates/rbmp-server/src/api/policy_fetch.rs`

The pattern is identical to bonsai's `bootstrap_device_handler`. Credentials resolved in Rust, injected as env vars, never as CLI args (visible in `ps -ef`) or HTTP (appear in access logs):

```rust
#[derive(Deserialize)]
pub struct PolicyFetchRequest {
    pub peer_addr:        String,   // Router IP to SSH into
    pub speaker_addr:     String,   // BMP speaker that monitors this peer
    pub credential_alias: String,   // Vault alias for SSH creds
    pub vendor:           String,   // "iosxr" | "iosxe" | "junos" | "eos" | "sros" | "frr"
    pub policy_name:      String,   // e.g. "PEER-AS65001-IN"
    pub direction:        String,   // "in" | "out"
    #[serde(default = "default_port")]
    pub port:             u16,
}
fn default_port() -> u16 { 22 }

pub async fn policy_fetch_handler(
    State(state): State<AppState>,
    Json(req): Json<PolicyFetchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Rust resolves credentials — identical to bonsai D4-17 pattern
    let cred = state.vault
        .resolve(&req.credential_alias, ResolvePurpose::SshFetch)
        .map_err(|e| (StatusCode::FAILED_DEPENDENCY,
                      format!("credential resolve failed: {e}")))?;

    let python_bin = if std::path::Path::new(".venv/bin/python").exists() {
        ".venv/bin/python"
    } else { "python3" };

    let mut cmd = tokio::process::Command::new(python_bin);
    cmd.arg("bmppy/policy_fetcher.py")
        .arg("--peer-addr").arg(&req.peer_addr)
        .arg("--vendor").arg(&req.vendor)
        .arg("--policy").arg(&req.policy_name)
        .arg("--direction").arg(&req.direction)
        .arg("--port").arg(req.port.to_string())
        // Credentials as env vars — NEVER as CLI args or HTTP body
        .env("RUSTYBMP_SSH_USERNAME", &cred.username)
        .env("RUSTYBMP_SSH_PASSWORD", &*cred.password)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = cmd.output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR,
                      format!("failed to spawn policy_fetcher: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
                    format!("policy_fetcher failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim())));
    }

    let result: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| serde_json::json!({ "status": "ok", "raw": stdout.trim() }));
    Ok(Json(result))
}
```

### 4.3 `bmppy/policy_fetcher.py` — focused subset of bonsai's bootstrap_agent.py

Same Genie testbed construction, same paramiko fallback for SRL/FRR, same credential env-var pattern. Runs exactly 3 show commands per vendor:

```python
#!/usr/bin/env python3
"""
RustyBMP policy fetcher — SSH-based NOS policy config retrieval.

Adapted from bonsai/python/bootstrap_agent.py.
Security contract: credentials from RUSTYBMP_SSH_USERNAME / RUSTYBMP_SSH_PASSWORD
env vars. Never accepted as CLI args.

Vendors:
  Genie testbed (SSH): iosxr, iosxe, ios, nxos, junos, eos
  Paramiko raw SSH:    sros, srl, frr
"""
from __future__ import annotations
import argparse, json, logging, os, sys, time
from typing import Any, Optional

logger = logging.getLogger("policy_fetcher")

# ── Vendor → OS map (identical to bonsai bootstrap_agent.py os_map) ───────────
VENDOR_TO_GENIE_OS = {
    "iosxr": "iosxr", "vxr": "iosxr",
    "iosxe": "iosxe", "ios": "ios",
    "nxos": "nxos",
    "junos": "junos", "juniper": "junos",
    "eos": "eos", "arista": "eos",
}

PARAMIKO_VENDORS = {"sros", "nokia_sros", "srl", "nokia_srl", "frr", "frrouting"}

def _creds_from_env() -> tuple[str, str]:
    u = os.environ.get("RUSTYBMP_SSH_USERNAME", "")
    p = os.environ.get("RUSTYBMP_SSH_PASSWORD", "")
    if not u or not p:
        logger.error("RUSTYBMP_SSH_USERNAME / RUSTYBMP_SSH_PASSWORD not set")
        sys.exit(1)
    return u, p

def _policy_commands(vendor: str, policy: str, peer: str, direction: str) -> list[str]:
    v = vendor.lower()
    if v in ("iosxr", "vxr"):
        return [
            f"show rpl route-policy {policy} detail",
            f"show bgp neighbor {peer} policy-statistics",
        ]
    elif v in ("iosxe", "ios", "nxos"):
        return [f"show route-map {policy}", f"show ip bgp neighbors {peer} policy"]
    elif v in ("junos", "juniper"):
        return [
            f"show policy-options policy-statement {policy}",
            f"show policy-options policy-statement {policy} statistics",
        ]
    elif v in ("eos", "arista"):
        return [f"show route-map {policy}", f"show route-map {policy} statistics"]
    elif v in ("sros", "nokia_sros"):
        return [f"show router policy-options policy {policy} statistics"]
    else:
        return [f"show route-map {policy}"]

def _connect_genie(address: str, username: str, password: str,
                   vendor: str, port: int):
    # Identical to bonsai bootstrap_agent.py testbed_dict construction
    from genie.testbed import load as genie_load
    genie_os = VENDOR_TO_GENIE_OS.get(vendor.lower(), "iosxe")
    ssh_host = address.split(":")[0]
    testbed_dict = {
        "devices": {
            address: {
                "os": genie_os,
                "type": "router",
                "credentials": {"default": {"username": username, "password": password}},
                "connections": {"cli": {"protocol": "ssh", "ip": ssh_host, "port": port}},
            }
        }
    }
    testbed = genie_load(testbed_dict)
    device  = testbed.devices[address]
    device.connect(log_stdout=False)
    return device

def _run_paramiko(address: str, username: str, password: str,
                  commands: list[str], port: int) -> dict[str, str]:
    # Identical to bonsai's paramiko fallback for SRL
    import paramiko
    ssh_host = address.split(":")[0]
    results  = {}
    client   = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        client.connect(ssh_host, port=port, username=username, password=password, timeout=30)
        for cmd in commands:
            exec_cmd = f'vtysh -c "{cmd}"' if "frr" in vendor else cmd
            _, stdout, _ = client.exec_command(exec_cmd, timeout=30)
            results[cmd] = stdout.read().decode(errors="replace")
    finally:
        client.close()
    return results

def main():
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)
    ap = argparse.ArgumentParser()
    ap.add_argument("--peer-addr",  required=True)
    ap.add_argument("--vendor",     required=True)
    ap.add_argument("--policy",     required=True)
    ap.add_argument("--direction",  default="in")
    ap.add_argument("--port",       type=int, default=22)
    args = ap.parse_args()

    username, password = _creds_from_env()
    t0 = time.time()
    commands = _policy_commands(args.vendor, args.policy, args.peer_addr, args.direction)

    parsed: dict[str, Any] = {}
    error = ""
    try:
        if args.vendor.lower() in PARAMIKO_VENDORS:
            raw = _run_paramiko(args.peer_addr, username, password, commands, args.port)
            parsed = {cmd: {"raw": text, "structured": None} for cmd, text in raw.items()}
        else:
            device = _connect_genie(args.peer_addr, username, password, args.vendor, args.port)
            try:
                for cmd in commands:
                    try:
                        parsed[cmd] = {"structured": device.parse(cmd), "raw": None}
                    except Exception:
                        parsed[cmd] = {"structured": None, "raw": device.execute(cmd)}
            finally:
                try: device.disconnect()
                except Exception: pass
    except Exception as e:
        error = str(e)

    print(json.dumps({
        "peer_addr": args.peer_addr, "vendor": args.vendor,
        "policy_name": args.policy, "direction": args.direction,
        "status": "ok" if not error else "failed", "error": error,
        "commands": parsed,
        "elapsed_s": round(time.time() - t0, 2),
    }))

if __name__ == "__main__": main()
```

### 4.4 Five-tier parser ecosystem — `bmppy/rbmppy/policy/`

```
bmppy/rbmppy/policy/
├── __init__.py          NosPolicy, PolicyClause, PolicyHitMatrix exports
├── ast.py               Vendor-neutral AST (NosPolicy, PolicyClause, MatchCondition, SetOperation)
├── confidence.py        Per-source confidence model
├── parsers/
│   ├── genie.py         Tier 1 — offline Genie (IOS-XR, IOS-XE, Junos, EOS, NXOS)
│   ├── textfsm.py       Tier 4 — TextFSM/NTC (Nokia SR-OS, SR Linux, FRR, Huawei)
│   ├── openconfig.py    Tier 3 — OpenConfig YANG (gNMI/NETCONF, any vendor)
│   └── batfish.py       Tier 2 — Batfish simulation (formal verification)
├── correlator.py        Merges Tiers 1-4 + BMP diff (Tier 5) + Path Status TLV
├── simulator.py         Batfish testRoutePolicies + searchRoutePolicies
└── detector.py          Policy change detection (KL divergence on attribute distribution)
```

Coverage matrix:
| Vendor | Tier 1 (Genie) | Tier 2 (Batfish) | Tier 3 (OC) | Tier 4 (TextFSM) | Tier 5 (BMP) |
|--------|---------------|-----------------|-------------|-----------------|--------------|
| Cisco IOS-XR | ✅ full | ✅ full | ✅ | ✅ | ✅ |
| Cisco IOS-XE | ✅ full | ✅ full | ✅ | ✅ | ✅ |
| Juniper JunOS | ✅ full | ✅ full | ✅ | ✅ | ✅ |
| Arista EOS | ✅ full | ✅ full | ✅ | ✅ | ✅ |
| Nokia SR-OS | ❌ | ❌ | ✅ | ✅ | ✅ |
| Nokia SR Linux | ❌ | ❌ | ✅ gNMI | ✅ | ✅ |
| FRRouting | ❌ | ✅ partial | ✅ | ✅ | ✅ |
| Any unknown | ❌ | ❌ | ❌ | ❌ | ✅ |

Correlation confidence model:
1. Router per-clause statistics (Genie `show bgp neighbor X policy-statistics`) — **0.99**
2. Batfish `testRoutePolicies` simulation — **0.95**
3. Path Status TLV filtered-inbound flag (0x0100) — **0.90**
4. BMP pre/post diff + attribute change detection — **0.70-0.85**

### 4.5 New schema and endpoints

```sql
CREATE TABLE IF NOT EXISTS policy_configs (
    fetched_at   TIMESTAMPTZ NOT NULL,
    peer_addr    VARCHAR     NOT NULL,
    speaker_addr VARCHAR     NOT NULL,
    policy_name  VARCHAR     NOT NULL,
    direction    VARCHAR     NOT NULL,  -- 'in' | 'out'
    vendor       VARCHAR     NOT NULL,
    clauses_json VARCHAR     NOT NULL,  -- serialized PolicyClause list
    source       VARCHAR     NOT NULL,  -- 'ssh_genie' | 'ssh_paramiko' | 'pasted' | 'bmp_inferred'
    confidence   DOUBLE      NOT NULL
);
```

New endpoints:
```
POST /api/policy/fetch              {peer_addr, credential_alias, vendor, policy, direction}
GET  /api/policy/configs            list fetched configs per peer
GET  /api/policy/configs/{peer}     config for a specific peer
GET  /api/credentials               list vault aliases
POST /api/credentials/add           {alias, username, password}
POST /api/credentials/test          {alias, address} → SSH connectivity
```

UI additions to `/policy` page:
- "Fetch from Router" button → credential alias dropdown + vendor selector + policy name input
- Confidence badge showing which tier provided the data
- Router ground-truth vs BMP-inferred side-by-side when both available

---

## Part 5 — RV7 Theme 4: BGPsec Full Validation

### 5.1 Status: parse done in RV6, validation in RV7

RV6 parses the BGPsec_Path attribute (type 30) and stores the raw signature blocks. RV7 adds:
1. Fetch RPKI router certificates (distinct from ROAs — these are the signing certificates for BGPsec)
2. Per-AS-hop ECDSA P-256 signature validation using `ring` crate
3. `bgpsec_validations` DuckDB table with per-route verdict

```toml
# crates/rbmp-enrichment/Cargo.toml
ring = "0.17"       # ECDSA P-256 signature validation
x509-cert = "0.2"  # X.509 router certificate parsing
```

**New file**: `crates/rbmp-enrichment/src/bgpsec.rs`

```rust
/// BGPsec path validation result per UPDATE message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BgpsecVerdict {
    Valid,
    Invalid { hop: u8, reason: String },
    NotFound,           // No router certificate for this AS
    Unverified,         // Router cert present but ASN not in RPKI router cert set
}

/// BGPsec validator — holds loaded router certificates from RPKI.
pub struct BgpsecValidator {
    /// Map: AS number → ECDSA public key (from router certificate)
    router_certs: HashMap<u32, Vec<u8>>,
}

impl BgpsecValidator {
    pub fn validate_path(
        &self,
        bgpsec_attr: &BgpsecPathAttribute,
        prefix: &Prefix,
    ) -> BgpsecVerdict { ... }
}
```

**New DuckDB table**:
```sql
CREATE TABLE IF NOT EXISTS bgpsec_validations (
    occurred_at    TIMESTAMPTZ NOT NULL,
    prefix         VARCHAR     NOT NULL,
    peer_addr      VARCHAR     NOT NULL,
    as_path        VARCHAR,
    verdict        VARCHAR     NOT NULL,  -- 'valid' | 'invalid' | 'not_found'
    invalid_hop    UTINYINT,
    invalid_reason VARCHAR
);
```

---

## Part 6 — RV7 Theme 5: Topology Scale + BGP Convergence

### 6.1 BGP-LS topology level-of-detail

Current D3 force simulation breaks above ~500 nodes. Three rendering modes:

**Mode A — Force-directed (< 100 nodes)**: Current implementation. Keep as-is.

**Mode B — Hierarchical layout (100-1000 nodes)**: Use D3 tree layout with spine-leaf hierarchy inferred from BGP-LS node roles. IS-IS Level-2 nodes → spine tier. Level-1 nodes → leaf tier.

**Mode C — Clustered AS-level (> 1000 nodes)**: Collapse nodes into AS-level clusters. Each circle = one AS, size = node count, edges = inter-AS adjacencies. Click to expand.

```svelte
<!-- ui/src/routes/topology/+page.svelte — adaptive rendering -->
<script lang="ts">
  const nodeCount = $derived(graph.nodes.length);
  const renderMode = $derived(
    nodeCount < 100  ? 'force' :
    nodeCount < 1000 ? 'hierarchical' :
                       'clustered'
  );
</script>

{#if renderMode === 'force'}
  <ForceGraph {graph} />
{:else if renderMode === 'hierarchical'}
  <HierarchicalGraph {graph} />
{:else}
  <ClusteredGraph {graph} />
{/if}
```

### 6.2 BGP convergence event detection

When a BMP PeerDown is received, the subsequent flood of withdrawal messages + eventual EOR marker constitutes a convergence event. Track and measure:

```python
# bmppy/rbmppy/analytics.py — new method

def detect_convergence_events(self, peer_addr: str, hours: int = 24) -> list[dict]:
    """
    Detect BGP convergence events: PeerDown → withdrawal flood → EOR.
    Returns list of events with:
      - start_ts: when PeerDown/mass-withdraw began
      - eor_ts: when EOR marker received
      - convergence_ms: end-to-end convergence time
      - affected_prefixes: count of prefixes withdrawn/re-announced
      - recovered: whether a new best path was found
    """
    ...
```

**New DuckDB table**:
```sql
CREATE TABLE IF NOT EXISTS convergence_events (
    event_id          UUID        NOT NULL,
    started_at        TIMESTAMPTZ NOT NULL,
    eor_at            TIMESTAMPTZ,
    convergence_ms    DOUBLE,
    speaker_addr      VARCHAR     NOT NULL,
    peer_addr         VARCHAR     NOT NULL,
    trigger_type      VARCHAR,    -- 'peer_down' | 'mass_withdraw' | 'eor_timeout'
    affected_prefixes UINTEGER,
    recovered_prefixes UINTEGER,
    unreachable_after  UINTEGER
);
```

**New API endpoint**: `GET /api/convergence?peer=X&hours=24`

**New UI panel** on Dashboard: "Convergence Events" timeline showing recent events with duration bars.

---

## Part 7 — RV7 Epic Index

| Epic | Title | Theme | Priority |
|------|-------|-------|----------|
| RV7-F1 | Add `roto = "0.11"` dependency + cranelift feature | Filter | P0 |
| RV7-F2 | `build_roto_runtime()` with all RouteCtx fields + helpers | Filter | P0 |
| RV7-F3 | `RotoFilterEngine` struct replacing `FilterEngine` | Filter | P0 |
| RV7-F4 | Default `config/filters.roto` with bogon+RPKI+OTC+blackhole | Filter | P0 |
| RV7-F5 | `filter_watcher.rs` — inotify hot-reload | Filter | P0 |
| RV7-F6 | YAML DSL retirement path (fallback mode, deprecation warning) | Filter | P1 |
| RV7-P1 | `path_status_tlv.rs` — parse 12 status bits + 11 reason codes | Protocol | P0 |
| RV7-P2 | Wire Path Status TLV into BMP route-monitoring parser | Protocol | P0 |
| RV7-P3 | `path_markings` DuckDB table + indexes + writer | Protocol | P0 |
| RV7-P4 | Per-status aggregate stats (TBD1-TBD14 from draft §4) | Protocol | P1 |
| RV7-P5 | RFC 9972 type 30 trend analytics (`max_prefix_capacity()`) | Protocol | P1 |
| RV7-P6 | BGPsec router cert fetch + ECDSA validation | Protocol | P2 |
| RV7-P7 | `bgpsec_validations` table | Protocol | P2 |
| RV7-V1 | `credentials.rs` copy → `vault.rs` (env var rename + SshFetch) | Vault | P0 |
| RV7-V2 | Credential CRUD API endpoints (add/update/remove/list/test) | Vault | P0 |
| RV7-V3 | `policy_fetch_handler` (spawn pattern from bonsai) | Vault | P0 |
| RV7-V4 | `bmppy/policy_fetcher.py` (Genie testbed + paramiko fallback) | Vault | P0 |
| RV7-V5 | `bmppy/rbmppy/policy/ast.py` — vendor-neutral AST | Vault | P1 |
| RV7-V6 | `parsers/genie.py` — offline Tier 1 parsing | Vault | P1 |
| RV7-V7 | `parsers/textfsm.py` — Tier 4 Nokia/FRR coverage | Vault | P1 |
| RV7-V8 | `parsers/batfish.py` — Tier 2 formal simulation | Vault | P2 |
| RV7-V9 | `correlator.py` — merge all sources into PolicyHitMatrix | Vault | P1 |
| RV7-V10 | `detector.py` — policy change detection | Vault | P2 |
| RV7-V11 | `policy_configs` DuckDB table | Vault | P1 |
| RV7-V12 | UI: credential manager in `/onboard` page | Vault | P1 |
| RV7-V13 | UI: "Fetch from Router" button on `/policy` page | Vault | P1 |
| RV7-UI1 | Path Pipeline page (`/path-status`) — per-prefix horizontal pipeline | UI | P1 |
| RV7-UI2 | Redundancy Health Matrix — prefix × peer grid with filter | UI | P1 |
| RV7-UI3 | Max-prefix Fuel Gauge dashboard (RFC 9972 type 30) | UI | P1 |
| RV7-UI4 | Dashboard: Convergence Events panel | UI | P2 |
| RV7-UI5 | Topology: adaptive rendering (force/hierarchical/clustered by node count) | UI | P1 |
| RV7-UI6 | `convergence_events` table + `/api/convergence` endpoint | UI | P2 |

---

## Part 8 — Development Priority Order

### P0 — Must complete first (foundational)

1. **RV7-F1 through F5**: Roto embed. The RouteCtx scaffold is ready; this is the mechanical step of wiring Roto into it. Unblocks the biggest operator-visible filter gap.

2. **RV7-P1 through P3**: Path Status TLV parse + store. Without this, the Redundancy Matrix and Path Pipeline pages have no data source. Parser is straightforward given the wire format is fully specified.

3. **RV7-V1 through V4**: Vault + policy_fetch_handler + policy_fetcher.py. The vault.rs copy is 30 minutes of work; the Python script is a focused subset of bonsai's bootstrap_agent.py.

### P1 — High value, plan for early sprint

4. **RV7-P5**: Type 30 trend analytics — linear regression on headroom, ETA to limit. The query is DuckDB SQL; the Python wrapper is 50 lines.

5. **RV7-UI2**: Redundancy Health Matrix — the single highest-value UI addition given Path Status TLV data. Operators need "show me where I've lost redundancy" more than any other view.

6. **RV7-UI3**: Max-prefix Fuel Gauge. Directly addresses the Thomas Graf article's capacity management use case. Operators forget their configured limits; this reminds them.

7. **RV7-V5 through V11**: Policy parser ecosystem. Genie offline parsing (V6) first — covers Cisco/Junos/Arista which is 80% of the installed base.

8. **RV7-UI5**: Topology level-of-detail. Required for SPs with full IGP topology in BGP-LS.

### P2 — Complete for feature parity

9. **RV7-P6 through P7**: BGPsec validation. Parse is done; this adds the crypto.

10. **RV7-V8 through V10**: Batfish + detector. Adds formal verification and automatic policy change detection.

11. **RV7-UI4 and UI6**: Convergence panel and event table.

---

## Part 9 — Updated Protocol Coverage Matrix

| RFC/Draft | Feature | Status |
|-----------|---------|--------|
| RFC 7854 | BMP core (Route Monitoring, Peer Up/Down, Stats) | ✅ RV1 |
| RFC 8671 | BMP TLVs | ✅ RV1 |
| RFC 9069 | BGP Local-RIB coverage | ✅ RV1 |
| RFC 9972 | BMP stats types 18-38 (gauges + AFI/SAFI) | ✅ RV1 |
| RFC 7432 | EVPN types 1-11 | ✅ RV2 |
| RFC 5575/8955 | Flowspec | ✅ RV3 |
| RFC 7752 | BGP-LS full TLVs (adj-SID, SRGB, Flex Algo) | ✅ RV3 |
| RFC 6514 | MCAST-VPN types 1-7 | ✅ RV6 |
| RFC 8205 | BGPsec_Path attribute parse (type 30) | ✅ RV6 |
| RFC 9319 | ASPA validation | ✅ RV6 |
| RFC 9514 | SRv6 SID NLRI SAFI 72 + uSID scaffold | ✅ RV4+RV6 |
| RFC 4761 | L2VPN VPLS SAFI 65 | ✅ RV4 |
| RFC 5549 | BGP unnumbered (IPv6 link-local next-hop) | ✅ RV4 |
| draft-ietf-grow-bmp-path-marking-tlv-05 | Path Status TLV | **🔲 RV7** |
| RFC 9972 type 30 trend analytics | Max-prefix capacity tracking | **🔲 RV7** |
| RFC 8205 full validation | BGPsec ECDSA path validation | **🔲 RV7** |

---

## Part 10 — Implementation Notes for Next Session

### On the Roto API

Roto v0.11 uses a slightly different embedding API than 0.10. Check `docs.rs/roto/0.11` for:
- `Runtime::new()` vs older `Engine::new()` naming
- `Compiler::new(&rt).compile(&source)` — verify the exact method signature
- How `call::<T, R>(fn_name, &arg)` is typed for RouteCtx

The RouteCtx scaffold in RV6 committed all the field names. The registration step in T2 must match those exact field names or Roto will error at parse time.

### On the vault copy

The key security invariant to preserve from bonsai: `ResolvedCredential.password` is `Zeroizing<String>`. When we pass it into the subprocess env var, `&*cred.password` dereferences the Zeroizing wrapper to get the &str, which is immediately copied into the process environment by the OS. The original `Zeroizing<String>` then drops (zeroing memory) when `cred` goes out of scope after the subprocess spawn. This is correct and intentional.

### On the Path Status TLV TLV type code

draft-ietf-grow-bmp-path-marking-tlv-05 uses `IANA TBD` for the TLV type code. Make this configurable via `[bmp] path_status_tlv_type = 6` in rustybmp.toml (with a default matching whatever Huawei VRP NE8000 ships, since they're the primary implementation per the draft's appendix). Add a note in the config comments.

### On the policy_fetcher.py paramiko FRR path

In bonsai, the FRR commands are sent via `vtysh -c "show route-map NAME"`. This requires the SSH user to have permission to run vtysh. For FRR the typical user is `frr` or the operator SSH user. Document this in the onboarding wizard.

### Upload next diff as

`rv7_all_changes.patch`

---

*End of RUSTYBMP_BACKLOG_RV7.md — Sprint RV7*
