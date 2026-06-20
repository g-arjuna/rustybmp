# RustyBMP — Sprint RV5 Backlog
## UI Completeness · Filter Maturity · ML Pipeline Hardening · Device Onboarding · SP/Enterprise Scale

> **Version**: RV5  
> **Date**: 2026-06-20  
> **Basis**: Full RV4 diff analysis + web research into Kentik/ThousandEyes/BGPalerter/BGPStream UI features + Rotonda filter gap analysis + ML code readiness review  
> **Framing**: RV4 delivered the skeleton. RV5 makes it an operator's tool.

---

## Part 1 — RV4 Comprehensive Analysis

### 1.1 What was delivered (all epics)

The RV4 diff (8193 lines, 6782 added) is the largest sprint yet. All scoped epics are complete.

| Epic | Delivered | Quality |
|------|-----------|---------|
| RV4-1: Security | `auth.rs` (JWT HS256 + middleware), `tls.rs` (rustls + tokio-rustls, `build_acceptor()` returns `Option<TlsAcceptor>`) | ✅ Both config-gated with sensible disable-by-default |
| RV4-2: Retention + Export | `retention.rs` (hourly sweep, skips startup tick), `api/export.rs` (table whitelist, zstd Parquet stream) | ✅ SQL injection blocked via `ALLOWED_TABLES` whitelist |
| RV4-3: Svelte 5 UI | 5 pages: Dashboard, Peers, Prefixes, Topology (D3 force-directed + zoom), Alerts | ⚠️ Skeleton complete, major SP-grade features missing — see §2 |
| RV4-4: ML pipeline | `parquet.py`, `topology.py`, `train_route_anomaly.py`, `topology_snapshot.py`, `parquet_store.py` | ⚠️ Foundation solid, specific gaps — see §3 |
| RV4-5: Protocol | SRv6 SID NLRI SAFI72, L2VPN VPLS, OTC leak detection wired | ✅ |
| RV4-6: NetworkX topology | `rbmppy/topology.py`: `BgpLsTopology` (shortest path, blast radius, SRLG-diverse paths), `AsTopology` | ✅ Clean, lazily imported networkx |
| RV4-7: HA + NATS | `ha.rs` (Redis SETNX lease, `HaState` `AtomicBool`), `crates/rbmp-nats/` (async-nats sink) | ✅ HA correctly defaults to always-leader when disabled |
| RV4-8: Container + CI | `Dockerfile` (multi-stage, bookworm-slim), `docker-compose.yml` (rustybmp + kafka + routinator), `.github/workflows/ci.yml` | ✅ |
| RV4-9: Testing | `docs/UBUNTU_TESTING.md` (204 lines, all 7 test scenarios), 4 integration test files, `benches/bmp_parse.rs` | ✅ Benchmark reports >1M msgs/sec for route monitoring PDU |

### 1.2 Explicitly out of scope — confirmed not addressed, tracked for RV6

The three explicitly-deferred items are confirmed absent from the diff and are correct call-outs:

| Item | Status | Correct deferral? |
|------|--------|-------------------|
| Active BGP session connector | NOT in diff — requires a full BGP FSM in Rust | ✅ Yes — BMP is the primary mechanism; a BGP session connector is a separate subsystem, better as a new crate `rbmp-bgp-session` in RV6 when operators ask for it. Cross-reference with Rotonda's `bgp-tcp-in` connector if borrowing architecture. |
| BGPsec path validation | NOT in diff — requires X.509 RPKI router certificates, AS_PATH crypto validation | ✅ Yes — needs `ring` or `rustls` for PKIX certificate validation, a separate BGPsec certificate cache updated from RPKI, and a per-UPDATE signature check. Significant scope; defer to RV6. |
| MCAST-VPN type decode | Stub arm in `bgp/types.rs` — stub compiles and preserves raw bytes | ✅ Yes — full RFC 6514 type 1-7 NLRI decode (C-multicast Join/Prune, I-PMSI A-D, S-PMSI A-D, etc.) is large but isolated to one file. Add to RV6 as `bgp/mvpn.rs`. |

---

## Part 2 — Rotonda Filter Gap: How to Close It

### 2.1 The gap defined honestly

Rotonda's Roto is a **compiled, statically-typed filter language** that:
- Compiles to native machine code before the first BMP message arrives
- Has zero runtime interpretation overhead — same speed as hand-written Rust
- Enforces type safety at compile time (not at message-receive time)
- Is loop-free by design (bounded execution, cannot stall the pipeline)
- Exposes RTR data as a first-class data source inside filter expressions
- Supports composable pipelines: `input | filter_bogons | filter_rpki_invalid | rib`

Our YAML DSL as of RV3 has:
- YAML-configured match-action rules (prefix_in, prefix_len_gt, community_has, etc.)
- Linear evaluation across filter rules per route event
- Actions: accept, reject, tag
- Configuration-reload wired into `RibManager.set_filter()`
- **No expressions**: only fixed predicates from a closed set
- **No composition**: cannot chain filters
- **No RTR integration inside filters**
- **No computed conditions**: cannot write `hop_count > 5 AND rpki_invalid`

The performance gap is real: YAML DSL evaluates via a Rust `match` + field access for every inbound route. Roto compiles to machine code. At 1500 msg/sec this is unmeasurable, but at 50K msg/sec (full internet table dump from 50 peers) it becomes relevant.

The expressivity gap is more important: operators need conditions like:
- `as_path_len > 20 AND NOT community_has("65535:666")`
- `rpki_validity == "invalid" AND prefix_len > 24`
- `origin_as IN [3356, 1299, 174]` — from whitelist of known Tier-1 ASes

### 2.2 RV5 approach: expression language on top of YAML DSL

**Not a full compiler — a predicate expression parser.** Use `pest` (PEG parser library) to parse filter conditions into AST closures. This gets to ~80% of Roto's practical value.

**New file**: `crates/rbmp-rib/src/filter_expr.rs`

#### Phase 1 — Expression grammar (add to existing YAML DSL)

Existing YAML filter:
```yaml
filters:
  - name: "reject-bogons"
    action: reject
    match:
      prefix_in: ["10.0.0.0/8", "172.16.0.0/12"]
```

New `expr` field (replaces `match` block for complex conditions):
```yaml
filters:
  - name: "rpki-invalid-too-specific"
    action: reject
    expr: "rpki == 'invalid' AND prefix_len > 24"

  - name: "tag-tier1-peers"  
    action: tag
    tag: "tier1"
    expr: "origin_as IN [701, 1239, 1299, 174, 3356, 6762, 7018]"

  - name: "reject-long-prepend"
    action: reject
    expr: "as_path_len > 6 AND has_prepend == true"

  - name: "alert-new-origin"
    action: alert
    alert_type: "origin_change"
    expr: "action == 'announce' AND peer_as IN [64512, 64513]"
```

#### Phase 2 — Filter expression AST

```rust
// crates/rbmp-rib/src/filter_expr.rs

use pest::Parser;
use pest_derive::Parser;
use std::collections::HashSet;

/// Route context available to filter expressions
pub struct RouteCtx<'a> {
    pub prefix:       &'a str,
    pub prefix_len:   u8,
    pub as_path_len:  usize,
    pub origin_asn:   u32,
    pub has_prepend:  bool,
    pub rpki:         &'a str,  // "valid" | "invalid" | "not-found" | "unknown"
    pub action:       &'a str,  // "announce" | "withdraw"
    pub peer_as:      u32,
    pub local_pref:   Option<u32>,
    pub med:          Option<u32>,
    pub community_set: HashSet<String>,
}

/// Compiled filter expression — evaluated per route event.
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    PrefixLenGt(u8),
    PrefixLenLt(u8),
    AsPathLenGt(usize),
    AsPathLenLt(usize),
    OriginAsIn(HashSet<u32>),
    PeerAsIn(HashSet<u32>),
    RpkiEq(String),       // "valid" | "invalid" | "not-found"
    ActionEq(String),     // "announce" | "withdraw"
    CommunityHas(String), // "64512:100"
    HasPrepend,
    True,
}

impl Expr {
    /// Evaluate the expression against a route context.
    pub fn eval(&self, ctx: &RouteCtx) -> bool {
        match self {
            Self::And(a, b)      => a.eval(ctx) && b.eval(ctx),
            Self::Or(a, b)       => a.eval(ctx) || b.eval(ctx),
            Self::Not(e)         => !e.eval(ctx),
            Self::PrefixLenGt(n) => ctx.prefix_len > *n,
            Self::PrefixLenLt(n) => ctx.prefix_len < *n,
            Self::AsPathLenGt(n) => ctx.as_path_len > *n,
            Self::AsPathLenLt(n) => ctx.as_path_len < *n,
            Self::OriginAsIn(s)  => s.contains(&ctx.origin_asn),
            Self::PeerAsIn(s)    => s.contains(&ctx.peer_as),
            Self::RpkiEq(v)      => ctx.rpki == v.as_str(),
            Self::ActionEq(v)    => ctx.action == v.as_str(),
            Self::CommunityHas(c) => ctx.community_set.contains(c),
            Self::HasPrepend     => ctx.has_prepend,
            Self::True           => true,
        }
    }
}
```

#### Phase 3 — PEG grammar (pest)

```pest
// crates/rbmp-rib/src/filter.pest

expr     = { or_expr }
or_expr  = { and_expr ~ ("OR" ~ and_expr)* }
and_expr = { not_expr ~ ("AND" ~ not_expr)* }
not_expr = { "NOT" ~ atom | atom }
atom     = { "(" ~ expr ~ ")" | comparison | membership | flag }

comparison = { field ~ op ~ value }
membership = { field ~ "IN" ~ "[" ~ int_list ~ "]" }
flag       = { "has_prepend" | "true" | "false" }

field  = { "prefix_len" | "as_path_len" | "origin_as" | "peer_as" | "rpki" | "action" | "community" | "local_pref" | "med" }
op     = { "==" | "!=" | ">" | "<" | ">=" | "<=" }
value  = { string | integer }
string = { "'" ~ (!"'" ~ ANY)* ~ "'" }
integer = { ASCII_DIGIT+ }
int_list = { integer ~ ("," ~ WHITESPACE* ~ integer)* }

WHITESPACE = _{ " " | "\t" }
```

Add `pest` to `rbmp-rib/Cargo.toml`:
```toml
pest       = "2"
pest_derive = "2"
```

#### Phase 4 — Hot-reload via inotify

**File**: `crates/rbmp-server/src/filter_watcher.rs`

Watch `config/filters.yaml` for changes using `notify` crate. On change:
1. Parse new filter YAML + compile `expr` fields
2. Validate all expressions (fail fast on parse error — don't apply broken config)
3. Call `rib_manager.set_filter(new_engine)` under write lock

```toml
notify = "6"
```

This gives us hot-reload without restart — closing the biggest remaining gap vs Roto.

#### Where we remain behind Roto (and that's OK)

| Roto capability | Our approach | Gap acceptable? |
|----------------|-------------|-----------------|
| Compiled to machine code | Interpreted AST (Expr enum) | ✅ At 1500 msg/sec, interpretation overhead is <1μs per message |
| Multi-RIB routing | Single RIB — filter decides store/drop only | ⚠️ Multi-RIB needs architectural change; deferred to RV6 |
| RTR data in filter scripts | RPKI validation done before filter (VrpCache) — result exposed as `rpki` field | ✅ Same practical effect |
| BGPsec inside filter | BGPsec not implemented at all | ✅ Deferred with BGPsec itself |
| Type safety at parse time | Runtime expression evaluation | ✅ Validated on load, not per-message |

---

## Part 3 — UI: Deep Analysis + SP/Enterprise Requirements

### 3.1 What the current UI has

The 5-page Svelte 5 app is aesthetically clean (dark gray palette, emerald accent, Tailwind system-ui). The skeleton is correct.

| Page | What's there | Assessment |
|------|-------------|-----------|
| Dashboard | 4 stat cards (peers up/down, speakers, RPKI %), live SSE event feed (last 50 entries) | ⚠️ Stat cards lack trend indicators; event feed is raw JSON, not operator-friendly |
| Peers | Table: IP, AS, RIB type, State, Prefixes, Hold; text search; manual refresh | ⚠️ No history, no capability display, no per-peer drill-down |
| Prefixes | Route table with live SSE toggle; prefix text search | ❌ Critically missing: no prefix history, no AS path viz, no per-prefix drill-down |
| Topology | D3 force-directed BGP-LS graph; zoom/pan; IGP metric labels on edges | ⚠️ No node click, no path highlight, no protocol filter working in UI |
| Alerts | SSE-backed in-memory alert list; severity colors; clear button | ❌ No persistence, no ack, no DuckDB-backed history |

### 3.2 What SP/Enterprise operators actually need (research-based)

Research sources: Kentik BGP Route Monitoring, ThousandEyes BGP Route Visualization, BGPalerter feature set, BGPStream, academic BGP route analysis literature.

The definitive operator needs in 2026:

**From Kentik (commercial benchmark)**: dynamic AS path visualization per prefix, RPKI validity overlay on all views, BGP path change timeline, announcement/withdrawal event timeline, traffic engineering change visualization (which paths changed + why), route leak + hijack detection alerts with enrichment.

**From ThousandEyes**: prefix timeline (30-day) showing path changes, grouping by monitor (equivalent to our: grouping by peer), path stability score, multi-vantage-point comparison (equivalent to: multi-peer comparison for same prefix).

**From BGPalerter**: ROA expiry alerts, new prefix visibility loss, unexpected upstream AS changes, RPKI TA malfunction detection.

**From BGPStream/academic**: BGP convergence time measurement, route flap damping status, AS relationship inference, MED oscillation detection.

### 3.3 Missing pages and features (prioritized)

#### Priority 1 — The Prefix Explorer (single most important missing page)

Kentik provides dynamic AS path visualizations, announcement and withdrawal events with a time series of event types by prefix — this is the single most critical feature missing from our UI.

**New route**: `/ui/src/routes/prefix/[prefix]/+page.svelte`

When an operator searches for or clicks on a prefix, they need to see:

```
Prefix: 203.0.113.0/24
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

┌─ Summary ────────────────────────────────────────┐
│ Origin AS:   AS64496 (Example Corp, US)          │
│ RPKI Status: ✅ Valid (ROA: AS64496 max-len /24) │
│ First seen:  2025-01-15 09:23 UTC                │
│ Last event:  2026-06-19 14:44 UTC (announce)     │
│ Stability:   High (3 events in 30 days)          │
└──────────────────────────────────────────────────┘

┌─ Announcement Timeline (last 7 days) ────────────┐
│  [Chart: y=event_count, x=time, color=action]    │
│  ████ announce  ░░░░ withdraw                    │
│  Convergence after last event: 340ms             │
└──────────────────────────────────────────────────┘

┌─ AS Path per Peer ───────────────────────────────┐
│  Peer 10.0.0.2 (AS65001): 64496 65001 64496     │
│  Peer 10.0.0.3 (AS65002): 64496 3356 64496      │
│  Path divergence: 1 hop different at position 2  │
└──────────────────────────────────────────────────┘

┌─ Event History ──────────────────────────────────┐
│  2026-06-19 14:44  announce  AS65001  64496 3356 │
│  2026-06-19 09:12  announce  AS65001  64496 7018 │  ← path changed
│  2026-06-17 03:33  withdraw  AS65001  —          │
└──────────────────────────────────────────────────┘

┌─ Enrichment (PeeringDB + RIPE STAT) ─────────────┐
│  Registrant: Example Corp                        │
│  RIR: ARIN  Country: US                          │
│  IRR route: ✅ route: 203.0.113.0/24  origin: AS64496 │
│  Known at RIPE RIS: 45 monitors see this prefix  │
└──────────────────────────────────────────────────┘
```

**API additions needed**:
```
GET /api/routes/prefix/{prefix}/timeline?since=7d
GET /api/routes/prefix/{prefix}/peers          (which peers see it + their AS_PATH)
GET /api/routes/prefix/{prefix}/convergence    (last N convergence times)
```

#### Priority 2 — AS Path Visualization

For the prefix explorer AND as a standalone view. Use D3 Sankey or chord diagram to show AS hop relationships:

```
[Customer AS] → [Provider 1] → [Transit A] → [Origin AS]
                            ↘ [Transit B] → [Origin AS]
```

Color code:
- Customer/Provider relationship (RFC 9234 OTC attribute)
- Prepended ASes (repeating)
- Private ASes (RFC 1918 equivalent)
- ASes with RPKI invalid routes

Kentik calls this "end-to-end, dynamic AS path visualization" — explore deep BGP visibility with end-to-end, dynamic AS path visualizations, track path changes, prefix reachability, and updates. It is the UI feature most referenced in operator surveys.

#### Priority 3 — RPKI Analysis Page

A dedicated RPKI page showing:
- Donut: valid/invalid/not-found breakdown (currently on dashboard as %)
- Table: all RPKI-invalid prefixes with origin AS, expected origin (from VRP), prefix length vs max-len
- "What if I enforce strict RPKI" analysis: how many prefixes would be dropped
- Per-peer RPKI invalid rate (some peers may advertise more invalid routes)
- ROA expiry calendar (from Routinator RTR data)
- Origin mismatch detail: prefix P is seen from AS A but ROA says AS B

#### Priority 4 — Policy Analysis View

BMP's unique value is pre-policy vs post-policy visibility. We need a UI for it:
```
Policy Analysis: AS65001 peer 10.0.0.2
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Pre-policy routes:  12,450
Post-policy routes: 11,892
Routes rejected by policy: 558

Top rejected prefixes:
  192.0.2.0/24   — rejected (bogon filter)
  10.0.0.0/8     — rejected (RFC1918 filter)
  198.18.0.0/15  — rejected (TEST-NET-2 filter)

Community modifications:
  + 65001:100 added to 3,201 routes (inbound tagging)
  - 64512:0 stripped from 445 routes (community scrub)

LOCAL_PREF changes:
  12,890 routes: 100 → 150 (transit preference policy)
```

This is possible from comparing pre-policy (Adj-RIB-In pre) vs post-policy (Adj-RIB-In post) RIB types, which BMP provides separately and we store in `route_events.rib_type`.

#### Priority 5 — Session Timeline / Peer Health History

Replace the static peer table with a timeline view showing session state over the past 7 days:

```
Peer 10.0.0.2 (AS65001)
Sessions: ━━━━━━━━━━━━━━━━━━━━▊▊━━━━━━━━━━━━━━━━━━━
           Mon      Tue      Wed      Thu      Fri
           ████ = up   ▊▊ = down   — = not configured
Flaps (7d): 2    Uptime: 5d 14h 22m    Prefixes: 12,450
Hold time: 90s   Capabilities: Add-Path IPv4, Flowspec, LLGR
```

This requires a new API endpoint:
```
GET /api/peers/{addr}/timeline?days=7   → [{ts, event_type, duration}]
```

#### Priority 6 — Device/Speaker Onboarding Wizard

Inspired by bonsai's `src/http_server/managed_devices.rs` pattern. A UI workflow for adding new BMP speakers:

**Step 1 — Register speaker**
```
Name:     xrd-pe1
IP:       10.0.0.1
Vendor:   Cisco IOS-XR  ▾
Site:     Singapore-DC1
ASN:      65000
```

**Step 2 — Generate BMP config snippet**
```
Click "Generate Config":

bmp server 1
 host 172.20.0.100 port 5000
 description rustybmp
 update-source Loopback0
 initial-delay 10
 stats-reporting-period 30
 initial-refresh delay 15 spread 2
!
router bgp 65000
 bmp-activate server 1
!

[Copy to clipboard] [Download .cfg]
```

**Step 3 — Test connection**
```
[Test BMP Connection]
Waiting for BMP Initiation message from 10.0.0.1...
✅ Connected — XRD 24.3.1 (Cisco IOS-XR)
   System name: xrd-pe1
   BMP session established 2.3 seconds ago
```

**Step 4 — Monitor onboarding progress**
```
Initial table dump in progress...
■■■■■■□□□□ 3,422 / ~7,000 routes received
EOR received for: IPv4 Unicast ✅
EOR pending:      IPv6 Unicast, VPNv4
```

#### Priority 7 — BMP Stats Viewer

We collect RFC 9972 stats (types 18-38) but expose zero UI for them. Operators need to see:

```
BMP Statistics — Peer 10.0.0.2 (AS65001)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Adj-RIB-In pre-policy:  12,450 routes    ████████████ 100%
Adj-RIB-In post-policy: 11,892 routes    ███████████░  95.5%
Routes rejected by policy:  558          ░░░░░░░░░░█    4.5%
  — Details: see Policy Analysis

RPKI invalids:          3,201 routes     ███░░░░░░░░░  25.7% ← alert threshold
RPKI not-found:         8,691 routes
RPKI valid:             0 routes         ← 0%! RPKI not enforced

GR stale routes:          0              ✅
LLGR stale routes:         0             ✅
Routes near limit:         0             ✅

[History chart: sparkline for each counter over 24h]
```

#### Priority 8 — SR Policy View

Since we decode SR Policy NLRI (SAFI 73) in RV3, operators need to see active policies:

```
SR Policies — Speaker 10.0.0.1
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Color  Endpoint      Preference  Segments                     Status
100    10.0.0.5      200         10.0.0.1 → 10.0.0.3 (SID A) ✅ Active
100    10.0.0.5      100         10.0.0.1 → 10.0.0.4 (SID B) ⚪ Backup
200    10.0.0.7      200         10.0.0.1 → 10.0.0.6 (SID C) ✅ Active
```

#### Priority 9 — ML Insights Page (NEW)

The analytics pipeline exists but has zero UI visibility. Operators need:

```
ML Anomaly Insights
━━━━━━━━━━━━━━━━━━

Model Status:
  Route Anomaly v1 (IsolationForest)
  Trained: 2026-06-15  |  Training rows: 45,230  |  Contamination: 5%
  [Retrain] [Download model]

Recent Anomaly Detections (last 24h):
┌──────────────────┬──────────┬──────────┬──────────────────────────────┐
│ Prefix           │ Score    │ Detected │ Reason                       │
├──────────────────┼──────────┼──────────┼──────────────────────────────┤
│ 203.0.113.0/24   │ -0.82    │ 14:23    │ hop_count spike (3→18 hops)  │
│ 198.51.100.0/24  │ -0.71    │ 09:11    │ origin_asn changed           │
│ 192.0.2.0/24     │ -0.65    │ 06:44    │ RPKI invalid + prepend       │
└──────────────────┴──────────┴──────────┴──────────────────────────────┘

[Export training data] [View all anomalies]
```

**API needed**: `GET /api/ml/anomalies?since=24h` — reads from a `ml_anomalies` DuckDB table populated by the Python DetectorPipeline.

#### Priority 10 — Operational Metrics Page

```
Operational Metrics — rustybmp Core
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

System:
  Uptime:          14d 3h 22m
  BMP msgs/sec:    847 (1hr avg)  / peak: 1,502
  DuckDB size:     2.1 GB (route_events: 1.4GB, stats_events: 0.7GB)
  Retention:       90 days (next sweep: 2h 44m)
  Kafka lag:       0 messages
  RTR sync:        OK (last: 4m ago, 412,301 VRPs)

Collectors:
  Direct:       1 core listener (port 5000)
  Distributed:  2 edge collectors (fra01, sin01)
  HA status:    🟢 Leader (lease expires in 7s)

Message rates (1hr sparklines):
  route_events:   [▁▂▃▂▃▄▃▂▃▄▃▂] 430/min avg
  peer_events:    [▁▁▁▁▁▁▁▁▁▁▁▁] 0.1/min avg
  stats_events:   [▂▂▂▂▂▂▂▂▂▂▂▂] 120/min avg
```

---

## Part 4 — ML Code Readiness Analysis

### 4.1 `rbmppy/parquet.py` — Solid Foundation, Two Issues

**Status**: Production-ready for feature export. One correctness issue.

**Issue 1: `origin_asn` extracts the last ASN from as_path**

```python
TRY_CAST(
    list_last(string_split(trim(COALESCE(as_path, '')), ' '))
    AS INTEGER
) AS origin_asn
```

This fails for:
- AS_SET routes: `64496 {64497 64498}` — `list_last()` returns `64498}` which `TRY_CAST` coerces to NULL
- Routes with no AS_PATH (iBGP routes where Loc-RIB is exported)
- Routes with AS_CONFED_SEQUENCE: `(65000 65001) 64496` — returns `64496` correctly, but confed segment handling differs by vendor

**Fix**: Add explicit handling:
```sql
TRY_CAST(
    regexp_extract(
        list_last(string_split(trim(COALESCE(as_path, '')), ' ')),
        '(\d+)', 1
    ) AS INTEGER
) AS origin_asn
```

**Issue 2: `origin_asn` is a categorical ID used as numeric feature in IsolationForest**

AS numbers like 64496 and 64497 are not numerically related — there is no semantic distance between them. Using raw ASN as a float feature teaches the model that "ASN distance" is meaningful. It isn't.

**Fix**: Hash the ASN to a stable bucket index, or use a binary flag per known-origin (for monitored prefixes). Simpler: drop `origin_asn` from `FEATURE_COLS` entirely and use `peer_as` instead (which IS meaningful — traffic patterns correlate with peer relationships).

### 4.2 `bmppy/ml/train_route_anomaly.py` — Wrong Granularity

**Status**: Trains, produces a model, but the feature matrix represents individual route events, not the right granularity for the anomaly model.

**The problem**: Each row is one route event (one announcement or withdrawal). The IsolationForest is learning to distinguish "normal announcements" from "abnormal announcements." But what operators need is "normal PREFIX BEHAVIOR" vs "abnormal prefix behavior." The correct granularity is:

**Per-prefix aggregated features over a time window**:
- `churn_rate_1h`: events per hour for this prefix
- `median_hop_count`: median AS_PATH length across all peers
- `hop_count_variance`: variance (high = inconsistent paths)
- `origin_asn_changes_24h`: how many times origin changed
- `rpki_invalid_fraction`: fraction of received paths that are RPKI invalid
- `peer_count_seeing`: number of peers currently announcing this prefix

This requires a DuckDB aggregation query, not a raw row export. Add `export_prefix_aggregates()` to `parquet.py`:

```python
def export_prefix_aggregates(
    db_path: str,
    output: str,
    window_hours: int = 1,
    days: int = 7,
) -> int:
    """
    Export per-prefix aggregated features — correct granularity for IsolationForest.
    Each row represents ONE PREFIX across a time window, not one route event.
    """
    since_ts = ...
    conn.execute(f"""
        COPY (
            WITH windows AS (
                SELECT
                    prefix,
                    time_bucket(INTERVAL '{window_hours} hours', occurred_at) AS window_start,
                    COUNT(*) AS event_count,
                    SUM(CASE WHEN action='announce' THEN 1 ELSE 0 END) AS announces,
                    SUM(CASE WHEN action='withdraw' THEN 1 ELSE 0 END) AS withdraws,
                    COUNT(DISTINCT peer_addr) AS peer_count,
                    AVG(as_path_len) AS avg_hop_count,
                    STDDEV(as_path_len) AS hop_count_stddev,
                    COUNT(DISTINCT
                        TRY_CAST(list_last(string_split(trim(COALESCE(as_path,'')), ' ')) AS INTEGER)
                    ) AS origin_asn_changes,
                    SUM(CASE WHEN rpki_validity='invalid' THEN 1 ELSE 0 END)::FLOAT
                        / COUNT(*) AS rpki_invalid_fraction,
                    AVG(COALESCE(local_pref, 100)) AS avg_local_pref
                FROM route_events
                WHERE occurred_at >= TIMESTAMPTZ '{since_ts}'
                GROUP BY prefix, window_start
            )
            SELECT * FROM windows
        ) TO '{output}' (FORMAT PARQUET, COMPRESSION 'zstd')
    """)
```

This is what bonsai's `train_anomaly.py` effectively does via Cypher `MATCH (e:DetectionEvent)` — it queries aggregated detection events, not raw telemetry rows.

### 4.3 `bmppy/ml/topology_snapshot.py` — Critical Stub Gap

**Status**: The module docstring and dataclass are well-designed. The implementation has one critical gap.

**Issue**: `SnapshotSequence.build()` and `BgpTopologySnapshot.to_pyg()` are declared but the diff shows they are stubs or incomplete. Specifically:

`to_pyg()` requires PyTorch Geometric `HeteroData` construction:
```python
def to_pyg(self):
    """
    Convert to PyTorch Geometric HeteroData for STGNN training.
    Requires: pip install torch torch-geometric
    """
    try:
        import torch
        from torch_geometric.data import HeteroData
    except ImportError:
        raise ImportError("torch and torch-geometric required")
    
    data = HeteroData()
    
    # Node features
    peer_features = self.nodes_df[NODE_FEATURE_COLS].fillna(0).values
    data['peer'].x = torch.tensor(peer_features, dtype=torch.float32)
    data['peer'].node_id = self.nodes_df['peer_addr'].tolist()
    
    # Edge indices (peer → peer via BGP session)
    if not self.edges_df.empty:
        src_idx = self.nodes_df['peer_addr'].reset_index(drop=True)
        src_idx = dict(zip(src_idx, range(len(src_idx))))
        
        valid = self.edges_df[
            self.edges_df['src'].isin(src_idx) &
            self.edges_df['dst'].isin(src_idx)
        ]
        src = torch.tensor([src_idx[s] for s in valid['src']], dtype=torch.long)
        dst = torch.tensor([src_idx[d] for d in valid['dst']], dtype=torch.long)
        data['peer', 'sessions_with', 'peer'].edge_index = torch.stack([src, dst])
        
        # Edge features (igp_metric, max_bandwidth)
        edge_feats = valid[['igp_metric', 'bandwidth']].fillna(0).values
        data['peer', 'sessions_with', 'peer'].edge_attr = torch.tensor(edge_feats, dtype=torch.float32)
    
    return data
```

This is the critical missing piece for the STGNN training pipeline. Without `to_pyg()`, `train_stgnn.py` (which doesn't exist yet) can't consume the snapshots.

### 4.4 Missing: `bmppy/ml/train_bgp_stgnn.py`

The STGNN training script is referenced in the backlog but was NOT implemented in RV4. The STGNN pipeline is:

```
SnapshotSequence (T=8 snapshots) → HeteroData list → GATv2-GRU → Anomaly score
```

This requires:
1. `topology_snapshot.py` `to_pyg()` complete (see §4.3)
2. Model definition: `class BgpStgnn(torch.nn.Module)` using `torch_geometric.nn.GATv2Conv` + GRU temporal layer
3. Training loop with NCT pre-training (similar to bonsai's `train_stgnn.py`)
4. Inference integration with `DetectorPipeline`

**RV5 action**: Complete `to_pyg()` and add `bmppy/ml/train_bgp_stgnn.py`.

### 4.5 `rbmppy/topology.py` — Two Performance Issues

**Issue 1**: `AsTopology._load()` queries `LIMIT 50000` route events to build the AS graph. For a network with 5M routes in DuckDB, this is a random 50K sample. The AS graph may miss important edges. Use a deterministic sampling or query the distinct AS pairs directly:

```sql
-- Better: query distinct AS pairs directly (much smaller result set)
SELECT DISTINCT
    TRIM(list_any_value(string_split(as_path, ' '))) AS asn1,  
    TRIM(list_last(string_split(as_path, ' '))) AS asn2,
    COUNT(*) AS edge_weight
FROM route_events
WHERE action = 'announce' AND as_path IS NOT NULL
  AND occurred_at >= NOW() - INTERVAL 24 HOUR
GROUP BY asn1, asn2
HAVING asn1 <> asn2
```

This returns O(N_edges) rows instead of O(N_routes) rows.

**Issue 2**: `BgpLsTopology` re-queries DuckDB on every instantiation. For the UI (which may call `GET /api/bgpls/graph` frequently), this creates unnecessary DuckDB scans. Add a TTL cache at the API level:

```rust
// In api/topology.rs — cache the graph for 60 seconds
use std::sync::OnceLock;
use tokio::sync::RwLock;

static GRAPH_CACHE: OnceLock<RwLock<(Instant, TopologyGraph)>> = OnceLock::new();

pub async fn bgpls_graph(...) -> Json<TopologyGraph> {
    let cache = GRAPH_CACHE.get_or_init(|| RwLock::new((Instant::now() - Duration::from_secs(61), Default::default())));
    let cached = cache.read().await;
    if cached.0.elapsed() < Duration::from_secs(60) {
        return Json(cached.1.clone());
    }
    drop(cached);
    let fresh = build_graph(&state, None);
    *cache.write().await = (Instant::now(), fresh.clone());
    Json(fresh)
}
```

---

## Part 5 — Device Onboarding (Bonsai-Inspired)

Bonsai's `src/http_server/managed_devices.rs` implemented a device lifecycle: registration → provisioning → monitoring → decommission. For rustybmp, the scope is narrower (BMP-only) but the pattern is the same.

### 5.1 API additions needed

```
POST   /api/speakers/register        — add speaker to config registry
GET    /api/speakers/{addr}/config   — generate BMP config snippet for vendor
POST   /api/speakers/{addr}/test     — test BMP connectivity (waits 15s for Initiation)
GET    /api/speakers/{addr}/onboard  — onboarding progress (EOR status per AFI/SAFI)
DELETE /api/speakers/{addr}          — remove from registry (does not disconnect existing session)
```

### 5.2 Config snippet generator

**File**: `crates/rbmp-server/src/api/onboard.rs`

```rust
pub enum Vendor {
    CiscoXR,
    CiscoXE,
    JunosOS,
    NokiaSRL,
    AristaEOS,
    FRRouting,
    OpenConfig,  // generic gNMI-style config output
}

pub fn generate_bmp_config(
    vendor: Vendor,
    collector_ip: &str,
    collector_port: u16,
    local_as: u32,
    update_source: Option<&str>,
) -> String {
    match vendor {
        Vendor::CiscoXR => format!(r#"
bmp server 1
 host {collector_ip} port {collector_port}
 description rustybmp
 {src}
 initial-delay 10
 stats-reporting-period 30
 initial-refresh delay 15 spread 2
!
router bgp {local_as}
 bmp-activate server 1
!
"#, src = update_source.map(|s| format!("update-source {s}")).unwrap_or_default()),
        Vendor::JunosOS => format!(r#"
set routing-options bmp station rustybmp
set routing-options bmp station rustybmp connection-mode active
set routing-options bmp station rustybmp address {collector_ip}
set routing-options bmp station rustybmp port {collector_port}
set routing-options bmp station rustybmp local-address {src}
set routing-options bmp station rustybmp statistics-timeout 60
set protocols bgp bmp station rustybmp monitor enable
"#, src = update_source.unwrap_or("0.0.0.0")),
        Vendor::FRRouting => format!(r#"
! FRRouting bmpd.conf
!
bmp server 1
 host {collector_ip} port {collector_port}
 description rustybmp
!
router bgp {local_as}
 bmp-activate server 1
!
"#),
        Vendor::NokiaSRL => format!(r#"
set / network-instance default protocols bgp-vpn bmp
set / network-instance default protocols bgp-vpn bmp station rustybmp
set / network-instance default protocols bgp-vpn bmp station rustybmp address {collector_ip}
set / network-instance default protocols bgp-vpn bmp station rustybmp port {collector_port}
set / network-instance default protocols bgp-vpn bmp station rustybmp report-rib adj-rib-in-pre pre-policy
set / network-instance default protocols bgp-vpn bmp station rustybmp admin-state enable
"#),
        _ => format!("# Not yet implemented for {:?}", vendor),
    }
}
```

### 5.3 BMP connection test endpoint

```rust
pub async fn test_bmp_connection(
    State(state): State<AppState>,
    Path(addr): Path<String>,
) -> Json<serde_json::Value> {
    // Check if speaker with this addr has connected in the last 30s
    let last_seen = {
        let rib = state.rib.read().await;
        rib.speaker_last_seen(&addr.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
    };
    
    match last_seen {
        Some(ts) if ts.elapsed() < Duration::from_secs(30) => {
            json!({ "connected": true, "last_seen_secs_ago": ts.elapsed().as_secs() })
        }
        _ => json!({ "connected": false, "hint": "Check BMP config and firewall on port 5000" }),
    }
}
```

### 5.4 EOR onboarding progress tracker

```rust
pub async fn onboard_progress(
    State(state): State<AppState>,
    Path(addr): Path<String>,
) -> Json<serde_json::Value> {
    let rib = state.rib.read().await;
    let speaker: IpAddr = addr.parse().map_err(|_| ...)?;
    
    // Get EOR status per peer per AFI/SAFI from RibManager
    let eor_status = rib.eor_status_for_speaker(&speaker);
    // eor_status: HashMap<(PeerAddr, AfiSafi), bool>
    
    json!({
        "speaker": addr,
        "total_routes": rib.total_routes_for_speaker(&speaker),
        "eor_received": eor_status.values().filter(|&&v| v).count(),
        "eor_pending": eor_status.values().filter(|&&v| !v).count(),
        "families": eor_status.iter().map(|((peer, afi_safi), eor)| json!({
            "peer": peer.to_string(),
            "afi_safi": format!("{}", afi_safi),
            "eor_received": eor,
        })).collect::<Vec<_>>(),
    })
}
```

---

## Part 6 — RV5 Epics Summary

### Epic RV5-1: Filter Expression Language

| Task | File | Details |
|------|------|---------|
| T1 | `crates/rbmp-rib/src/filter_expr.rs` | Expr AST + RouteCtx + eval() |
| T2 | `crates/rbmp-rib/src/filter.pest` | PEG grammar via pest |
| T3 | `crates/rbmp-rib/src/filter.rs` | Parse YAML `expr:` field → Expr AST |
| T4 | `crates/rbmp-server/src/filter_watcher.rs` | inotify hot-reload |
| T5 | Integration test | `filter_reject_invalid_and_too_specific` passes; hot-reload test |

### Epic RV5-2: Prefix Explorer UI

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/prefix/[prefix]/+page.svelte` | Summary + timeline chart + AS path per peer + event history + enrichment |
| T2 | `crates/rbmp-server/src/api/routes.rs` | `/api/routes/prefix/{prefix}/timeline`, `/peers`, `/convergence` |
| T3 | `ui/src/lib/api.ts` | Add `prefixTimeline()`, `prefixPeers()`, `prefixConvergence()` |
| T4 | Dashboard — make prefix names clickable | Link from routes table → prefix explorer |

### Epic RV5-3: AS Path Visualizer

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/aspath/+page.svelte` | D3 Sankey/DAG for AS path hops |
| T2 | `crates/rbmp-server/src/api/topology.rs` | `/api/as-path?prefix=P` → {hops, edges, peer_paths} |

### Epic RV5-4: RPKI Analysis Page

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/rpki/+page.svelte` | Donut + invalid table + impact analysis |
| T2 | `crates/rbmp-server/src/api/routes.rs` | `/api/rpki/analysis` with per-peer + per-prefix breakdown |

### Epic RV5-5: Policy Analysis View

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/policy/+page.svelte` | Pre vs post-policy diff per peer |
| T2 | `crates/rbmp-store/src/query.rs` | `policy_delta(peer_addr)` query comparing rib_type pre vs post |

### Epic RV5-6: Peer Health History Timeline

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/peers/[addr]/+page.svelte` | Session timeline + capability display + route delta |
| T2 | `crates/rbmp-server/src/api/peers.rs` | `/api/peers/{addr}/timeline?days=7` |

### Epic RV5-7: Speaker Onboarding + BMP Config Generator

| Task | File | Details |
|------|------|---------|
| T1 | `crates/rbmp-server/src/api/onboard.rs` | register, config_snippet, test, progress endpoints |
| T2 | `ui/src/routes/onboard/+page.svelte` | 4-step wizard: register → config → test → monitor |
| T3 | `crates/rbmp-rib/src/manager.rs` | `eor_status_for_speaker()`, `speaker_last_seen()` |

### Epic RV5-8: BMP Stats Dashboard

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/stats/+page.svelte` | RFC 9972 stat viewer per peer with sparklines |
| T2 | `crates/rbmp-server/src/api/stats.rs` | `/api/stats/{speaker}/{peer}?counter=N&hours=24` |

### Epic RV5-9: ML Completions

| Task | File | Details |
|------|------|---------|
| T1 | `bmppy/ml/topology_snapshot.py` | Complete `to_pyg()` with edge feature tensors |
| T2 | `bmppy/ml/train_bgp_stgnn.py` | New: GATv2-GRU model + NCT pre-training + supervised phase |
| T3 | `bmppy/rbmppy/parquet.py` | Add `export_prefix_aggregates()` (per-prefix windowed aggregation) |
| T4 | `bmppy/ml/train_route_anomaly.py` | Fix `origin_asn` encoding; switch to `export_prefix_aggregates` |
| T5 | `crates/rbmp-store/src/schema.rs` | Add `ml_anomalies` table for storing DetectorPipeline results |
| T6 | `crates/rbmp-server/src/api/` | `/api/ml/anomalies?since=24h` reads `ml_anomalies` |
| T7 | `ui/src/routes/ml/+page.svelte` | ML insights: model status + anomaly table |

### Epic RV5-10: SR Policy UI View

| Task | File | Details |
|------|------|---------|
| T1 | `ui/src/routes/srpolicy/+page.svelte` | Active SR policies: color/endpoint/preference/segments |
| T2 | `crates/rbmp-store/src/schema.rs` | `srpolicy_events` table (missing since RV3-1) |
| T3 | `crates/rbmp-store/src/writer.rs` | Write SR Policy events from PathAttributes.sr_policy |
| T4 | `crates/rbmp-server/src/api/topology.rs` | `/api/srpolicy` endpoint |

---

## Part 7 — Updated Project State Matrix (Post-RV4)

### Protocol coverage — complete after RV4

| Feature | Status |
|---------|--------|
| BMP RFC 7854 + 8671 + 9069 | ✅ |
| RFC 9972 stats types 18-38 | ✅ |
| EVPN types 1-11 | ✅ |
| BGP-LS NLRI + full attribute TLVs | ✅ |
| SR Policy SAFI 73 types A-K | ✅ |
| Route Target Constraint SAFI 132 | ✅ |
| SRv6 SID NLRI SAFI 72 | ✅ (RV4-5) |
| L2VPN VPLS SAFI 65 | ✅ (RV4-5) |
| Flowspec RFC 5575/8955 | ✅ |
| RPKI RTR (RFC 6810) | ✅ |
| MRT RFC 6396 | ✅ |
| Active BGP session | ❌ RV6 |
| BGPsec | ❌ RV6 |
| MCAST-VPN full decode | ❌ RV6 |

### UI coverage — gaps for RV5

| Feature | Status |
|---------|--------|
| Dashboard (basic) | ✅ skeleton |
| Peers table | ✅ skeleton |
| Prefixes live table | ✅ skeleton |
| BGP-LS topology graph | ✅ skeleton |
| Alerts (SSE) | ✅ skeleton |
| Prefix Explorer (timeline, history) | ❌ RV5-2 |
| AS Path Visualizer | ❌ RV5-3 |
| RPKI Analysis page | ❌ RV5-4 |
| Policy Analysis (pre vs post) | ❌ RV5-5 |
| Peer history timeline | ❌ RV5-6 |
| Device onboarding wizard | ❌ RV5-7 |
| BMP Stats (RFC 9972) viewer | ❌ RV5-8 |
| ML Insights page | ❌ RV5-9 |
| SR Policy view | ❌ RV5-10 |

### ML coverage — gaps for RV5

| Feature | Status |
|---------|--------|
| DuckDB → Parquet export | ✅ functional |
| IsolationForest training | ✅ functional, needs per-prefix aggregation fix |
| NetworkX topology graph | ✅ complete |
| STGNN topology snapshots | ⚠️ `to_pyg()` stub, no train script |
| ML anomaly persistence to DuckDB | ❌ RV5-9 |
| ML insights API + UI | ❌ RV5-9 |

---

## Part 8 — Notes for Next Session

Next diff should include at minimum RV5-2 (Prefix Explorer) and RV5-7 (Device Onboarding) as they are the highest operator-visible improvements. RV5-1 (filter expression language) is the highest technical value addition.

Upload: `rv5_all_changes.patch` + `RUSTYBMP_PROJECT_CONTEXT.md` (no zip needed).

---

*End of RUSTYBMP_BACKLOG_RV5.md — Sprint RV5*
