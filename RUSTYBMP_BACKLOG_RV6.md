# RustyBMP — Sprint RV6 Backlog
## UI Completeness · Roto-Level Filter Language · Protocol Completeness · SP/AI-DC Scale

> **Version**: RV6  
> **Date**: 2026-06-20  
> **Research basis**: Kentik BGP Route Monitoring (2026), ThousandEyes BGP Route Visualization view, NLNOG RING Looking Glass, Cloudflare Radar BGP Sankey (May 2025), DE-CIX GlobePEER, RouteViews LG (May 2025). Roto v0.11 documentation (cranelift JIT, January 2026). SRv6 uSID for AI infrastructure (Upperside WC Paris 2026). EVPN-VXLAN DC fabric guide (Juniper 2026). BMP RFC 9972, ASPA RFC 9319.  
> **Strategic framing**: RV5 scaffolded navigation (11 nav items) but left 6 pages empty. RV6 delivers the full UI, replaces our filter DSL with embedded Roto, and adds ASPA + BGPsec + MCAST-VPN to match the 2026 protocol landscape.

---

## Part 1 — RV5 Completion Audit

### What was delivered

RV5 was a small sprint (1,486 lines, 944 added). It wired the foundation for UI expansion without completing the pages.

| Item | Status | Notes |
|------|--------|-------|
| Filter `expr:` field | ✅ | `filter.rs` now carries `compiled_expr: Option<Expr>`; `filter_expr.rs` committed at sprint base (referenced but not in diff) |
| New nav items ×6 | ✅ skeleton | RPKI, Policy, AS Paths, Onboarding, ML Insights, BMP Stats nav links added to layout.svelte |
| Clickable prefixes | ✅ | Prefix table links to `/prefix/{encoded}` |
| Clickable peer IPs | ✅ | Peer table links to `/peers/{encoded}` |
| API type envelope fix | ✅ | `api.ts` now expects `{ routes: [...] }` not raw arrays |
| Prefix Explorer API | ✅ | `prefix_timeline`, `prefix_peers`, `prefix_convergence`, `rpki_analysis` in routes.rs |
| Peer timeline API | ✅ | `peer_timeline` in peers.rs |
| `ml_anomalies` schema | ✅ | Table added to schema.rs |
| `export_prefix_aggregates()` | ✅ | analytics.py — windowed per-prefix aggregation |
| `_prepare_features()` fix | ✅ | train_route_anomaly.py — handles missing columns gracefully |

### What is NOT in RV5 (confirmed missing pages)

| Page | Route | Status |
|------|-------|--------|
| Prefix Explorer | `/prefix/[prefix]/+page.svelte` | ❌ Not in diff |
| Peer Detail | `/peers/[addr]/+page.svelte` | ❌ Not in diff |
| RPKI Analysis | `/rpki/+page.svelte` | ❌ Not in diff |
| Policy Analysis | `/policy/+page.svelte` | ❌ Not in diff |
| AS Path Visualizer | `/aspath/+page.svelte` | ❌ Not in diff |
| Onboarding Wizard | `/onboard/+page.svelte` | ❌ Not in diff |
| ML Insights | `/ml/+page.svelte` | ❌ Not in diff |
| BMP Stats Viewer | `/stats/+page.svelte` | ❌ Not in diff |
| SR Policy View | `/srpolicy/+page.svelte` | ❌ Not in diff |

**RV6 delivers all 9 missing pages plus the filter language upgrade.**

---

## Part 2 — Filter Language: Embed Roto Directly

### 2.1 The strategic insight

Roto is **designed to be embedded in any Rust application**. Its features (cranelift JIT compilation, hot-reload, static typing, no-loops guarantee, Rust type registration with zero-cost) are precisely what our filter language needs. Building a competing implementation would be redundant and inferior.

**The correct answer is: embed Roto as our filter engine.** Replace our PEG-parsed expression DSL with actual Roto scripts.

Roto v0.11 (January 2026):
- JIT compiled to machine code via cranelift (same as Firefox's JS engine)
- Statically typed with full type inference  
- Hot-reloadable at runtime (call `recompile()`, new JIT code takes effect immediately)
- `filtermap` construct: function that `accept`s or `reject`s (plus optional attribute mutation)
- Named functions composable within scripts
- Modules and imports
- f-strings for logging
- All Rust types/functions registerable at negligible cost — no serialization

### 2.2 Implementation plan

#### RV6-FL T1 — Add Roto to rbmp-rib

**File**: `crates/rbmp-rib/Cargo.toml`
```toml
roto = "0.11"
```

#### RV6-FL T2 — Register BGP types with Roto runtime

**New file**: `crates/rbmp-rib/src/roto_ctx.rs`

```rust
// crates/rbmp-rib/src/roto_ctx.rs
//
// Register rustybmp's BGP types with the Roto runtime.
// Roto operates on a RouteCtx struct exposed as a registered Roto type.
// Zero serialization: Roto receives a pointer to the RouteCtx on the stack.

use roto::{Runtime, roto_method, roto_static_method};
use rbmp_core::bgp::types::PathAttributes;
use rbmp_core::bmp::types::RibType;
use std::net::IpAddr;
use serde::{Deserialize, Serialize};

/// Flat context presented to every Roto filtermap.
/// All fields are cheap to compute from PathAttributes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteCtx {
    // Prefix fields
    pub prefix:      String,    // "203.0.113.0/24"
    pub prefix_len:  u8,
    pub afi:         String,    // "ipv4" | "ipv6"
    // Session context
    pub peer_as:     u32,
    pub peer_addr:   String,
    pub rib_type:    String,    // "pre-policy" | "post-policy" | "loc-rib" | "adj-out-pre" …
    pub action:      String,    // "announce" | "withdraw"
    // BGP attributes
    pub as_path:     String,    // space-separated ASN list
    pub as_path_len: u32,
    pub origin_asn:  u32,
    pub has_prepend: bool,
    pub next_hop:    String,
    pub local_pref:  u32,       // 0 when absent (iBGP default 100 is router-specific)
    pub med:         u32,
    pub origin:      String,    // "igp" | "egp" | "incomplete"
    // Communities (as strings for Roto comparisons)
    pub communities: Vec<String>,  // ["64512:100", "no-export"]
    pub ext_communities: Vec<String>,
    pub large_communities: Vec<String>,
    // Security
    pub rpki:        String,    // "valid" | "invalid" | "not-found" | "unknown"
    pub otc_asn:     u32,       // 0 when OTC attribute absent
    // SR/EVPN context
    pub is_evpn:     bool,
    pub is_bgpls:    bool,
    pub is_srpolicy: bool,
    pub evpn_type:   u8,        // 0 when not EVPN
}

impl RouteCtx {
    pub fn from_bmp(
        prefix: &rbmp_core::bgp::types::Prefix,
        peer_as: u32,
        peer_addr: IpAddr,
        rib_type: RibType,
        action: &str,
        attrs: &PathAttributes,
        rpki: &str,
    ) -> Self {
        let as_path = attrs.as_path.as_ref().map(|p| p.to_string()).unwrap_or_default();
        let asns: Vec<u32> = as_path.split_whitespace()
            .filter_map(|s| s.parse().ok()).collect();
        let origin_asn = asns.last().copied().unwrap_or(0);
        let has_prepend = asns.windows(2).any(|w| w[0] == w[1]);

        Self {
            prefix:      prefix.to_string(),
            prefix_len:  prefix.prefix_len(),
            afi:         if prefix.is_v6() { "ipv6" } else { "ipv4" }.to_string(),
            peer_as,
            peer_addr:   peer_addr.to_string(),
            rib_type:    format!("{:?}", rib_type).to_lowercase(),
            action:      action.to_string(),
            as_path,
            as_path_len: asns.len() as u32,
            origin_asn,
            has_prepend,
            next_hop:    attrs.next_hop.map(|h| h.to_string()).unwrap_or_default(),
            local_pref:  attrs.local_pref.unwrap_or(0),
            med:         attrs.multi_exit_disc.unwrap_or(0),
            origin:      attrs.origin.as_ref().map(|o| format!("{}", o)).unwrap_or_default(),
            communities: attrs.communities.iter().map(|c| c.to_string()).collect(),
            ext_communities: attrs.extended_communities.iter().map(|c| c.to_string()).collect(),
            large_communities: attrs.large_communities.iter().map(|c| c.to_string()).collect(),
            rpki:        rpki.to_string(),
            otc_asn:     attrs.only_to_customer.unwrap_or(0),
            is_evpn:     attrs.evpn_reach.is_some(),
            is_bgpls:    attrs.bgpls_reach.is_some(),
            is_srpolicy: attrs.sr_policy.as_ref().map(|p| !p.is_empty()).unwrap_or(false),
            evpn_type:   attrs.evpn_reach.as_ref()
                .and_then(|e| e.routes.first())
                .map(|r| r.route_type_code())
                .unwrap_or(0),
        }
    }
}

/// Build a Roto Runtime with all BGP types and helper functions registered.
pub fn build_roto_runtime() -> roto::Runtime {
    let mut rt = Runtime::new();
    
    // Register RouteCtx fields as accessible properties
    rt.register_type::<RouteCtx>("RouteCtx")
        .field("prefix",      |r: &RouteCtx| r.prefix.clone())
        .field("prefix_len",  |r: &RouteCtx| r.prefix_len)
        .field("afi",         |r: &RouteCtx| r.afi.clone())
        .field("peer_as",     |r: &RouteCtx| r.peer_as)
        .field("peer_addr",   |r: &RouteCtx| r.peer_addr.clone())
        .field("rib_type",    |r: &RouteCtx| r.rib_type.clone())
        .field("action",      |r: &RouteCtx| r.action.clone())
        .field("as_path",     |r: &RouteCtx| r.as_path.clone())
        .field("as_path_len", |r: &RouteCtx| r.as_path_len)
        .field("origin_asn",  |r: &RouteCtx| r.origin_asn)
        .field("has_prepend", |r: &RouteCtx| r.has_prepend)
        .field("local_pref",  |r: &RouteCtx| r.local_pref)
        .field("med",         |r: &RouteCtx| r.med)
        .field("rpki",        |r: &RouteCtx| r.rpki.clone())
        .field("otc_asn",     |r: &RouteCtx| r.otc_asn)
        .field("is_evpn",     |r: &RouteCtx| r.is_evpn)
        .field("is_srpolicy", |r: &RouteCtx| r.is_srpolicy)
        .field("evpn_type",   |r: &RouteCtx| r.evpn_type);

    // Register helper functions callable from Roto
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
            use std::str::FromStr;
            if let (Ok(target), Ok(range)) = (
                cidr.parse::<ipnet::IpNet>(),
                ctx.prefix.parse::<ipnet::IpNet>(),
            ) {
                range.subnet_of(&target) || range == target
            } else {
                false
            }
        }
    );
    
    rt
}
```

#### RV6-FL T3 — Roto filter engine in filter.rs

**Replace** the current `FilterEngine` with a Roto-backed implementation:

```rust
// crates/rbmp-rib/src/filter.rs (Roto-backed engine)

use roto::{Compiler, Runtime, CompiledScript};
use crate::roto_ctx::{RouteCtx, build_roto_runtime};
use rbmp_core::bgp::types::PathAttributes;
use rbmp_core::bmp::types::RibType;
use std::net::IpAddr;
use tracing::{error, info, warn};

/// Verdict returned by a filtermap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterVerdict {
    Accept,
    Reject,
    /// Route accepted and tagged (tag name from script output)
    AcceptWithTag(u8),
}

/// Roto-backed filter engine. Hot-reloadable.
pub struct RotoFilterEngine {
    runtime:  Runtime,
    script:   Option<CompiledScript>,
    /// Path to the .roto file (for hot-reload)
    path:     String,
}

impl RotoFilterEngine {
    /// Load and compile a Roto script from `path`.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let rt     = build_roto_runtime();
        let source = std::fs::read_to_string(path)?;
        let script = Compiler::new(&rt).compile(&source)
            .map_err(|e| anyhow::anyhow!("Roto compile error: {e}"))?;
        info!(path, "Roto filter script loaded and JIT-compiled");
        Ok(Self { runtime: rt, script: Some(script), path: path.to_string() })
    }

    /// Load with fallback to no filtering (pass-all) on error.
    pub fn load_with_fallback(path: &str) -> Self {
        match Self::load(path) {
            Ok(e)  => e,
            Err(e) => {
                error!(%e, "Failed to load Roto filter — all routes accepted");
                Self { runtime: build_roto_runtime(), script: None, path: path.to_string() }
            }
        }
    }

    /// Hot-reload: re-read and recompile the script file.
    /// Returns Ok(true) if the new script compiled and replaced the old one.
    pub fn reload(&mut self) -> anyhow::Result<bool> {
        let source = std::fs::read_to_string(&self.path)?;
        match Compiler::new(&self.runtime).compile(&source) {
            Ok(new_script) => {
                self.script = Some(new_script);
                info!(path = %self.path, "Roto filter hot-reloaded");
                Ok(true)
            }
            Err(e) => {
                warn!(%e, "Roto reload failed — keeping existing filter");
                Ok(false)
            }
        }
    }

    /// Evaluate a route. Returns Accept or Reject.
    pub fn evaluate(
        &self,
        prefix:    &rbmp_core::bgp::types::Prefix,
        peer_as:   u32,
        peer_addr: IpAddr,
        rib_type:  RibType,
        action:    &str,
        attrs:     &PathAttributes,
        rpki:      &str,
    ) -> FilterVerdict {
        let script = match &self.script {
            Some(s) => s,
            None    => return FilterVerdict::Accept, // no script = accept all
        };
        let ctx = RouteCtx::from_bmp(prefix, peer_as, peer_addr, rib_type, action, attrs, rpki);
        match script.call::<RouteCtx, bool>("bgp_filter", &ctx) {
            Ok(true)  => FilterVerdict::Accept,
            Ok(false) => FilterVerdict::Reject,
            Err(e)    => {
                warn!(%e, "Roto evaluation error — accepting route");
                FilterVerdict::Accept
            }
        }
    }
}
```

#### RV6-FL T4 — Default filter script and documentation

**New file**: `config/filters.roto`

```roto
# rustybmp route filter — Roto scripting language
# Reference: https://roto.docs.nlnetlabs.nl/
#
# The filtermap bgp_filter is called for every route event.
# Return true to ACCEPT the route (store + forward).
# Return false to REJECT the route (discard silently).
#
# Available route context fields:
#   route.prefix          - "203.0.113.0/24"
#   route.prefix_len      - 24 (u8)
#   route.afi             - "ipv4" | "ipv6"
#   route.peer_as         - 65001 (u32)
#   route.as_path_len     - 3 (u32)
#   route.origin_asn      - 64496 (u32)
#   route.has_prepend     - true | false
#   route.local_pref      - 100 (u32)
#   route.med             - 0 (u32)
#   route.rpki            - "valid" | "invalid" | "not-found"
#   route.otc_asn         - 0 when absent, set to ASN when OTC attr present
#   route.is_evpn         - true | false
#   route.is_srpolicy     - true | false
#   route.evpn_type       - 0-11 (0 = not EVPN)
#
# Available helper functions:
#   community_has(route, "64512:100")        - check standard community
#   as_path_contains(route, 64496)           - check ASN in path
#   prefix_in_range(route, "10.0.0.0/8")    - check prefix coverage


# RFC 1918 and documentation prefix bogons
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

# Too-specific prefixes are signs of deaggregation attacks or misconfig
fn is_too_specific(route: RouteCtx) -> bool {
    route.afi == "ipv4" && route.prefix_len > 24
}

# RPKI-invalid routes from more-specific prefixes indicate likely hijacks
fn looks_like_hijack(route: RouteCtx) -> bool {
    route.rpki == "invalid" && route.prefix_len > 24
}

# Route leak: OTC attribute present means route should not have been
# forwarded further (violates RFC 9234 Only-to-Customer semantics)
fn is_route_leak(route: RouteCtx) -> bool {
    route.otc_asn > 0 && route.rib_type == "adj-out-pre"
}

# Main filter — called for every BMP route event.
# This function is HOT (called at line rate) — keep it fast.
fn bgp_filter(route: RouteCtx) -> bool {
    # Always pass non-announce events (withdrawals should always be stored)
    if route.action == "withdraw" {
        return true
    }

    # Reject bogons
    if is_bogon(route) {
        return false
    }

    # Reject obvious hijack signatures (RPKI invalid + too specific)
    if looks_like_hijack(route) {
        return false
    }

    # Reject route leaks (OTC violation)
    if is_route_leak(route) {
        return false
    }

    # Accept everything else
    true
}
```

**Advanced filter examples** (`config/filters-examples.roto`):

```roto
# Example: Operator-defined community-based routing policy
# Reject routes tagged with "do not store" community
fn bgp_filter(route: RouteCtx) -> bool {
    if community_has(route, "64512:0") {
        return false
    }
    # Tag Tier-1 routes for priority processing
    if as_path_contains(route, 174)
    || as_path_contains(route, 1299)
    || as_path_contains(route, 3356) {
        # Accept tier-1 routes (no filtering)
        return true
    }
    # RPKI strict enforcement example:
    if route.rpki == "invalid" {
        return false
    }
    true
}
```

```roto
# Example: AI datacenter fabric filter — accept only expected IP block
# and reject any route with private ASNs in the path
fn has_private_asn_in_path(route: RouteCtx) -> bool {
    # Check for RFC 6996 private ASN range
    # (simplified: check origin only, real implementation checks all ASNs)
    route.origin_asn >= 64512 && route.origin_asn <= 65534
}

fn bgp_filter(route: RouteCtx) -> bool {
    # Only accept routes within our DC address space (10.0.0.0/8)
    if !prefix_in_range(route, "10.0.0.0/8") {
        return false
    }
    # Reject if path has private/documentation ASNs  
    if has_private_asn_in_path(route) {
        return false
    }
    # Reject if AS_PATH is suspiciously long for a DC fabric (max 5 hops)
    if route.as_path_len > 5 {
        return false
    }
    true
}
```

#### RV6-FL T5 — Wire Roto engine into RibManager

**File**: `crates/rbmp-rib/src/manager.rs`

Replace `filter: Option<FilterEngine>` with `roto_filter: Option<RotoFilterEngine>`:

```rust
pub fn set_roto_filter(&mut self, engine: RotoFilterEngine) {
    self.roto_filter = Some(engine);
}

pub fn reload_roto_filter(&mut self) -> anyhow::Result<bool> {
    if let Some(ref mut f) = self.roto_filter {
        f.reload()
    } else {
        Ok(false)
    }
}
```

In the route processing path:
```rust
// Before inserting into RIB:
let verdict = self.roto_filter.as_ref()
    .map(|f| f.evaluate(&prefix, peer_as, peer_addr, rib_type, action, attrs, rpki_str))
    .unwrap_or(FilterVerdict::Accept);

if verdict == FilterVerdict::Reject {
    counter!("bgp_routes_filtered_total", "reason" => "roto").increment(1);
    continue;
}
```

#### RV6-FL T6 — Hot-reload via `notify` and filter admin API

**New endpoint**: `POST /api/filters/reload` — triggers `reload_roto_filter()` under write lock.

**Config**:
```toml
[filters]
enabled    = true
script     = "config/filters.roto"
hot_reload = true      # watch file changes via inotify
```

Wire `notify` watcher that calls `reload_roto_filter()` when `config/filters.roto` changes.

#### RV6-FL T7 — Filter test endpoint

**New endpoint**: `POST /api/filters/test` with body:
```json
{
  "prefix": "203.0.113.0/24",
  "peer_as": 65001,
  "as_path": "65001 64496",
  "rpki": "valid",
  "action": "announce"
}
```

Returns `{ "verdict": "accept" | "reject", "evaluation_ns": 42 }`.

Lets operators verify filter behavior without sending live traffic.

#### RV6-FL T8 — Filter stats

Expose per-filter verdict counters in Prometheus:
```
rustybmp_filter_verdict_total{verdict="accept"} 1234567
rustybmp_filter_verdict_total{verdict="reject"} 45678
rustybmp_filter_evaluation_ns_p99 180
```

### 2.3 What this achieves vs Roto/industry comparison

| Capability | Roto (Rotonda) | Our RV6 (embed Roto) |
|-----------|---------------|---------------------|
| JIT to machine code (cranelift) | ✅ | ✅ |
| Static typing | ✅ | ✅ |
| Hot-reload without restart | ✅ | ✅ |
| Loop-free guarantee | ✅ | ✅ |
| Named functions (composable) | ✅ | ✅ |
| BGP type access (zero serialization) | ✅ | ✅ |
| f-strings / logging inside filter | ✅ | ✅ |
| Multi-RIB routing | ✅ | ❌ RV7 |
| External data source access (RTR in filter) | Planned | ❌ RV7 |
| Filter configuration syntax = Roto | ✅ | ✅ |

**This is full parity on all hot-path capabilities.** Multi-RIB and external data source access in filters are future work.

---

## Part 3 — Protocol Completeness

### 3.1 ASPA (RFC 9319) — Critical 2026 addition

ASPA (Autonomous System Provider Authorization) is a RPKI extension that allows ASes to publish their legitimate upstream providers. BMP observers can use ASPA to detect route leaks more accurately than OTC alone.

**New crate**: `crates/rbmp-enrichment/src/aspa.rs`

```rust
/// RFC 9319 ASPA record: Customer ASN → set of Provider ASNs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspaRecord {
    pub customer_asn:  u32,
    pub provider_asns: Vec<u32>,  // providers sorted for binary search
}

/// ASPA validation result per AS hop in an AS_PATH
pub enum AspaVerdict {
    Valid,      // all upstream relationships verified
    Invalid,    // at least one hop violates ASPA (route leak)
    Unknown,    // no ASPA record for this customer
    NotApplicable, // only 1 ASN in path, no relationship to verify
}

/// Validate an AS_PATH against loaded ASPA records.
/// RFC 9319 §4: for each consecutive pair (left_AS, right_AS) in AS_PATH,
/// right_AS must appear in left_AS's ASPA provider set.
pub fn validate_as_path(as_path: &[u32], records: &HashMap<u32, AspaRecord>) -> AspaVerdict { ... }
```

**RTR protocol extension**: ASPA records are distributed via RTR (RFC 8210 + RFC 9319). Add to `RtrClient`:
```rust
// PDU type 11: ASPA PDU (RFC 9319 §3.4)
11 => {
    let flags       = data[0];
    let customer_as = u32::from_be_bytes([data[1],data[2],data[3],data[4]]);
    let providers   = parse_aspa_providers(&data[5..]);
    if flags & 1 == 1 {  // ANNOUNCE flag
        aspa_cache.insert(customer_as, providers);
    }
}
```

**Roto integration**: expose `aspa_valid(route)` as a registered helper function.

**New route context field**: `aspa: String` — "valid" | "invalid" | "unknown"

### 3.2 BGPsec (RFC 8205) — Path validation

BGPsec validates each AS_PATH hop using cryptographic signatures. Requires:
1. RPKI Router Certificates (separate from ROAs)
2. BGPsec_Path attribute (type 30, carries signature blocks)
3. Per-hop signature validation using each AS's router public key

**New crate**: `crates/rbmp-bgpsec/`
```toml
[dependencies]
ring = "0.17"          # crypto: ECDSA P-256 signature validation
x509-cert = "0.2"      # X.509 certificate parsing for router certs
```

**Scope for RV6**: Parse `BGPsec_Path` attribute (type 30), extract per-hop signatures, store raw in DuckDB `bgpsec_path` column. Full validation deferred to RV7 (requires router cert fetch from RPKI).

### 3.3 MCAST-VPN full RFC 6514 decode — `bgp/mvpn.rs`

The stub arm is in place. Full decode for types 1-7:

```rust
// crates/rbmp-core/src/bgp/mvpn.rs

pub enum MvpnNlri {
    Type1IntraAsIPmsi     { rd: [u8; 8], originating_router: IpAddr },
    Type2InterAsIPmsi     { rd: [u8; 8], originating_as: u32 },
    Type3SPmsiAD          { rd: [u8; 8], multicast_source: IpAddr, multicast_group: IpAddr, originating_router: IpAddr },
    Type4LeafAD           { rd: [u8; 8], route_key: Vec<u8>, path_id: u32 },
    Type5SourceActiveAD   { rd: [u8; 8], multicast_source: IpAddr, multicast_group: IpAddr },
    Type6SharedTreeJoin   { rd: [u8; 8], source_as: u32, multicast_group: IpAddr },
    Type7SourceTreeJoin   { rd: [u8; 8], multicast_source: IpAddr, multicast_group: IpAddr },
    Unknown               { nlri_type: u8, data: Vec<u8> },
}
```

### 3.4 BGP Unnumbered (RFC 5549) — IPv6 link-local next-hop

Critical for AI datacenter fabrics where BGP peers over point-to-point links use IPv6 link-local addresses. AI cluster fabrics (Meta, Google, NVIDIA public architectures) use this pattern.

RFC 5549 encodes IPv6 link-local next-hops in `MP_REACH_NLRI` for IPv4 NLRI. Currently rustybmp only handles IPv4 next-hops for IPv4 NLRI. Add:

```rust
// In bgp/nlri.rs decode_next_hop():
// When AFI=1 (IPv4), SAFI=1, and next_hop_len=16 or 32 → RFC 5549
// IPv6 next-hop for IPv4 NLRI (BGP unnumbered / RFC 5549)
```

New `RibEntry` field: `pub is_unnumbered: bool` — set when IPv6 link-local next-hop detected for IPv4 prefix.

### 3.5 SRv6 uSID decoding — critical for 2026 AI infrastructure

SRv6 uSID is transforming IP networking by streamlining operations. From improving traffic management in AI backend networks to enabling agile, future-proofed architectures for service providers. Multiple hyperscalers (Alibaba, Verizon, Nebius, Rakuten) presented SRv6 deployments at Upperside WC 2026. Our SRv6 attribute decode needs uSID support.

**In `bgp/srv6.rs`**: add `Srv6uSidStructure`:
```rust
/// SRv6 uSID — compressed SID encoding (draft-filsfils-spring-usid)
/// uSID block: 16/20/24/32 bits, uSID argument: 0/4/8/16 bits
pub struct Srv6uSid {
    pub block_len:     u8,
    pub node_len:      u8,
    pub function_len:  u8,
    pub argument_len:  u8,
    pub usid:          [u8; 16],  // full 128-bit IPv6 address
    pub usid_block:    [u8; 4],   // extracted block prefix
}
```

---

## Part 4 — UI: Complete Build

### 4.1 Design system and scale requirements

**Before writing pages**: define the design contract.

**Design principles (from research)**:
- For teams that need BGP visibility integrated with broader network intelligence, the platform should provide a more unified view — every page must link to related pages (prefix → topology, peer → prefix list, alert → prefix history)
- ThousandEyes represents information in a cohesive manner, pointing out relevant events on the timeline and in the ASN graph — timeline + graph must appear on the same page, synchronized
- From Cloudflare Radar: Sankey diagrams for AS paths with hover tooltips showing collector + timestamp

**Performance requirements at scale**:
- 200+ peers × 900K routes = 180M route_events rows in 90 days
- API responses must paginate (no `SELECT * LIMIT 5000` for production)
- UI tables must virtual-scroll (only render visible rows)
- SSE events at 1500/sec must not freeze the browser (batch DOM updates via `requestAnimationFrame`)
- Topology graph with 5K+ BGP-LS nodes needs level-of-detail rendering

**Component library additions needed** (`ui/src/lib/`):
```
ui/src/lib/
├── api.ts              ← exists (needs additions)
├── components/
│   ├── TimelineChart.svelte   ← D3 time-series, reusable
│   ├── AsnSankey.svelte       ← D3 Sankey for AS paths
│   ├── RpkiBadge.svelte       ← colored validity pill
│   ├── VirtualTable.svelte    ← virtual-scroll table
│   ├── PrefixSearch.svelte    ← search with autocomplete
│   ├── MetricCard.svelte      ← stat card with sparkline
│   ├── AlertBanner.svelte     ← dismissible alert header
│   └── CopyButton.svelte      ← copy-to-clipboard
```

### 4.2 Page 1: Prefix Explorer (`/prefix/[prefix]/+page.svelte`)

This is the most important missing page. In addition to the graphical route visualization, a routing data table is also available, providing a structured row-and-column representation of AS paths for each prefix.

**Layout**: 3-panel
```
┌─ Header: Prefix + enrichment summary ──────────────────────────────────────┐
│ 203.0.113.0/24  Origin: AS64496 (Example Corp)  RPKI: ✅ Valid              │
│ First seen: 2025-01-15  Last event: 5 min ago  Stability: High             │
└──────────────────────────────────────────────────────────────────────────────┘
┌─ Timeline (24h/7d/30d) ─────────────────────────────────────────────────────┐
│ [D3 bar chart: announces=emerald, withdraws=red, bucketed by hour]          │
│ Timeline metric selector: Path Changes | Reachability | Updates             │
└──────────────────────────────────────────────────────────────────────────────┘
┌─ AS Path per Peer ─────────────────┐  ┌─ Recent Events ───────────────────┐
│ Peer AS65001: 64496 3356 64496     │  │ 14:44 announce AS65001 64496 3356 │
│ Peer AS65002: 64496 7018 64496     │  │ 09:12 announce AS65001 64496 7018 │
│ Path divergence: position 2 differs│  │ 03:33 withdraw AS65001            │
└────────────────────────────────────┘  └───────────────────────────────────┘
┌─ RPKI Detail ──────────────┐  ┌─ Internet Enrichment ─────────────────────┐
│ Status: Valid              │  │ Registrant: Example Corp (ARIN)            │
│ ROA: AS64496 max-len /24   │  │ Country: US  PeeringDB: AS64496            │
│ VRP count: 1               │  │ IRR: route: 203.0.113.0/24 origin: AS64496 │
└────────────────────────────┘  └───────────────────────────────────────────┘
```

**Key Svelte 5 component spec**:
```svelte
<!-- ui/src/routes/prefix/[prefix]/+page.svelte -->
<script lang="ts">
  import { page } from '$app/stores';
  import TimelineChart from '$lib/components/TimelineChart.svelte';
  import AsnSankey from '$lib/components/AsnSankey.svelte';
  import RpkiBadge from '$lib/components/RpkiBadge.svelte';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';

  const prefix = $derived(decodeURIComponent($page.params.prefix));
  
  let timeline    = $state([]);
  let peers       = $state([]);
  let convergence = $state([]);
  let timeRange   = $state<'1d'|'7d'|'30d'>('7d');
  let timelineMetric = $state<'events'|'paths'|'peers'>('events');

  onMount(async () => {
    const days = timeRange === '1d' ? 1 : timeRange === '7d' ? 7 : 30;
    [timeline, peers, convergence] = await Promise.all([
      api.prefixTimeline(prefix, days).then(r => r.timeline),
      api.prefixPeers(prefix).then(r => r.peers),
      api.prefixConvergence(prefix, 10).then(r => r.events),
    ]);
  });
  
  // Derived: average convergence time
  const avgConvergenceMs = $derived(
    convergence.length
      ? convergence.reduce((s, e) => s + (e.convergence_ms ?? 0), 0) / convergence.length
      : null
  );
  
  // Derived: path diversity (are all peers seeing the same path?)
  const uniquePaths = $derived(new Set(peers.map(p => p.as_path)).size);
</script>
```

### 4.3 Page 2: AS Path Visualizer (`/aspath/+page.svelte`)

Cloudflare Radar uses a Sankey diagram. Sankey diagram illustrating the BGP routes for a given prefix. The visualization displays routes directed towards the Tier 1 networks. The interactive view allows panning and zooming, and hovering over the links provides tooltip information on which collector saw the route and when it was last updated.

**Layout**:
```
Search: [prefix or ASN input]  Time range: [1h | 24h | 7d]
Protocol: [IPv4 | IPv6]  RL: [Resolved | Raw ASNs]

┌─ Sankey: AS hop flow ─────────────────────────────────────────────────────┐
│                                                                            │
│   [Origin AS]  ──────────────→  [Transit A]  ──────→  [Target AS]        │
│                ──→  [Transit B] ─────────────────────→                    │
│                                                                            │
│   Node width = route count  Link width = announcement frequency           │
│   Hover → tooltip: peer_addr, last_seen, community tags                  │
│   Click node → drill into ASN detail                                      │
└────────────────────────────────────────────────────────────────────────────┘
┌─ AS Path Details table ────────────────────────────────────────────────────┐
│  Peer    │ AS Path                      │ Length │ Prepend │ RPKI  │ Count │
│  10.0.0.2│ 64496 3356 1299 13335        │   4    │  No     │ Valid │  847  │
│  10.0.0.3│ 64496 3356 3356 1299 13335   │   5    │  Yes    │ Valid │  213  │
└────────────────────────────────────────────────────────────────────────────┘
```

**`AsnSankey.svelte` component** (D3 Sankey):
```svelte
<!-- ui/src/lib/components/AsnSankey.svelte -->
<script lang="ts">
  import * as d3 from 'd3';
  import { sankey, sankeyLinkHorizontal } from 'd3-sankey';
  import { onMount } from 'svelte';

  let { paths = [], asnNames = {} } = $props<{
    paths: { hops: number[]; count: number }[];
    asnNames: Record<number, string>;
  }>();

  let container: SVGSVGElement;

  function draw() {
    // Build nodes (unique ASNs) and links (consecutive pairs)
    const nodeSet = new Map<number, { id: number; name: string }>();
    const linkMap = new Map<string, { source: number; target: number; value: number }>();

    for (const { hops, count } of paths) {
      for (const asn of hops) {
        if (!nodeSet.has(asn)) {
          nodeSet.set(asn, { id: asn, name: asnNames[asn] ?? `AS${asn}` });
        }
      }
      for (let i = 0; i < hops.length - 1; i++) {
        const key = `${hops[i]}-${hops[i+1]}`;
        const existing = linkMap.get(key);
        if (existing) { existing.value += count; }
        else { linkMap.set(key, { source: hops[i], target: hops[i+1], value: count }); }
      }
    }

    const nodes = Array.from(nodeSet.values());
    const links = Array.from(linkMap.values());

    const nodeIndex = new Map(nodes.map((n, i) => [n.id, i]));
    const sankeyLinks = links.map(l => ({
      source: nodeIndex.get(l.source)!,
      target: nodeIndex.get(l.target)!,
      value:  l.value,
    }));

    const W = container.clientWidth, H = 400;
    d3.select(container).selectAll('*').remove();

    const { nodes: laid, links: laidLinks } = sankey()
      .nodeWidth(20).nodePadding(10).size([W, H])
      ({ nodes: nodes.map((_, i) => ({ index: i })), links: sankeyLinks });

    const svg = d3.select(container);
    // Draw links
    svg.append('g').selectAll('path').data(laidLinks)
      .join('path')
      .attr('d', sankeyLinkHorizontal())
      .attr('stroke', '#4ade80').attr('stroke-opacity', 0.4)
      .attr('fill', 'none')
      .attr('stroke-width', d => Math.max(1, d.width ?? 1));

    // Draw nodes
    svg.append('g').selectAll('rect').data(laid)
      .join('rect')
      .attr('x', d => d.x0!).attr('y', d => d.y0!)
      .attr('width', d => d.x1! - d.x0!).attr('height', d => d.y1! - d.y0!)
      .attr('fill', '#10b981');

    // Labels
    svg.append('g').selectAll('text').data(laid)
      .join('text')
      .attr('x', d => d.x0! - 6).attr('y', d => (d.y0! + d.y1!) / 2)
      .attr('text-anchor', 'end').attr('fill', '#d1fae5').attr('font-size', 11)
      .text((d, i) => nodes[i]?.name ?? '');
  }

  onMount(draw);
  $effect(() => { if (paths.length) draw(); });
</script>

<svg bind:this={container} class="w-full" style="height:400px"></svg>
```

### 4.4 Page 3: RPKI Analysis (`/rpki/+page.svelte`)

RPKI allows BGP route prefixes announced by an AS to be cryptographically signed such that ISPs can validate the AS is authorized to announce said routes. You can use this page to identify sites on your network with traffic that would be dropped if strict RPKI validation was enforced on the routers.

**Layout**:
```
RPKI Analysis
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Global:     Valid: 78%  Invalid: 3%  Not-found: 19%
  [Donut chart]     [Trend sparkline: 7-day RPKI valid% per day]

  Impact Analysis: "What if I enforce strict RPKI?"
  ┌────────────────────────────────────────────────────────────┐
  │  Routes that would be DROPPED: 3,201 (3.1% of total RIB) │
  │  Top dropped origins:                                      │
  │    AS64497: 1,847 prefixes (origin mismatch)               │
  │    AS64498:   892 prefixes (max-length violation /27)      │
  │  Top dropped prefix blocks:                                │
  │    203.0.113.0/25 → more-specific of ROA /24               │
  │    198.51.100.0/25 → ROA says AS64496, seen from AS64497   │
  └────────────────────────────────────────────────────────────┘

  RPKI Invalid Prefixes (per peer breakdown):
  ┌──────────┬────────────┬────────────────────────┬──────────────────────────┐
  │ Prefix   │ Seen AS    │ ROA says               │ Violation type           │
  ├──────────┼────────────┼────────────────────────┼──────────────────────────┤
  │ /25      │ AS64497    │ AS64496, max-len /24    │ Origin mismatch + length │
  └──────────┴────────────┴────────────────────────┴──────────────────────────┘

  RPKI Valid % by Peer (sorted by invalid count):
  [Bar chart: peer → valid%, invalid%, not-found%]

  ROA Coverage:
  Your prefixes with ROAs: 45/50 (90%)
  [Table: prefix, ROA exists?, max-len, expiry, status]
```

### 4.5 Page 4: Policy Analysis (`/policy/+page.svelte`)

BMP's unique value vs all other tools — pre vs post-policy visibility. No commercial tool offers this depth.

```
Policy Analysis
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Select peer: [AS65001 (10.0.0.2) ▾]  Compare: [Pre-policy ↔ Post-policy]

┌─ Policy Impact Summary ──────────────────────────────────────────────────┐
│  Pre-policy:  12,450 routes  │  Post-policy: 11,892 routes               │
│  Accepted:    11,892 (95.5%) │  Rejected:       558 (4.5%)               │
│                                                                          │
│  [Donut: accepted vs rejected]  [Time trend: rejection rate over 24h]    │
└──────────────────────────────────────────────────────────────────────────┘

Rejected Routes Analysis:
┌──────────────────────────────────────────────────┬───────────────────────┐
│ Reason                          │ Count  │ % of rejected               │
├────────────────────────────────-┼────────┼─────────────────────────────┤
│ Bogon prefix (RFC 1918)         │   280  │  50.2%                      │
│ RPKI invalid                    │   201  │  36.0%                      │
│ Too specific (>/24)             │    55  │   9.9%                      │
│ Blackhole community             │    22  │   3.9%                      │
└─────────────────────────────────┴────────┴─────────────────────────────┘

Community Modifications (applied by inbound policy):
  + 65001:100 added to 8,240 routes (transit tagging)
  + 65001:200 added to 3,652 routes (peer tagging)

LOCAL_PREF Changes:
  3,891 routes: 100 → 150  [peer preference policy]
  8,001 routes: 100 → 100  [no change]

[View raw diff table] [Export CSV]
```

### 4.6 Page 5: Peer Detail (`/peers/[addr]/+page.svelte`)

```
Peer: 10.0.0.2  AS65001  ● Up  14d 3h 22m
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Session Timeline (last 7 days):
[Gantt: ███████████████████████▊▊████ with flap markers]
       Mon    Tue    Wed    Thu    Fri    Sat    Sun
Flaps: 2      Uptime: 5d 14h     Hold time: 90s

BGP Capabilities:
  ✅ 4-byte ASN (RFC 6793)
  ✅ Add-Path IPv4 (RFC 7911)
  ✅ Flowspec (RFC 5575)
  ✅ Long-Lived GR (RFC 9494) — stale_time: 3600s
  ✅ BGP-LS (RFC 7752)
  ✅ Route Refresh (RFC 2918)
  ✅ OTC / BGP Role: Customer (RFC 9234)

Route Counts per RIB:
  Adj-RIB-In pre-policy:   12,450  [sparkline 24h]
  Adj-RIB-In post-policy:  11,892  [sparkline 24h]
  Route delta (1h):         -3 routes

RPKI Status for this Peer:
  Valid: 11,200 (94%)  Invalid: 201 (1.7%)  Not-found: 491 (4.1%)

AS Path Statistics:
  Avg hop count: 3.2    Max: 18    Prepend ratio: 8.3%

Recent Events (from peer_events):
  14:44 → peer_up   hold=90s
  09:11 → peer_down  reason: holdtimer expired (code 4)
  09:08 → peer_up   hold=90s
  03:33 → peer_down  reason: notification (code 6 sub 4)

[View routes from this peer] [View policy analysis] [RPKI breakdown]
```

### 4.7 Page 6: Device Onboarding (`/onboard/+page.svelte`)

Inspired by bonsai's managed_devices workflow and the operational reality that after adding new prefixes with a network command a network administrator wants to be sure the new routes are visible on the upstream provider BGP routers:

```
Device Onboarding
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  [Tab: Registered Speakers] [Tab: Add New Speaker]

  ── Registered Speakers ──────────────────────────────────────────
  ┌─────────────────┬──────────┬──────────────┬────────┬──────────┐
  │ Hostname        │ IP       │ Vendor       │ Status │ Prefixes │
  ├─────────────────┼──────────┼──────────────┼────────┼──────────┤
  │ xrd-pe1         │ 10.0.0.1 │ Cisco IOS-XR │  ● Up  │ 12,450  │
  │ xrd-pe2         │ 10.0.0.2 │ Cisco IOS-XR │  ● Up  │  3,892  │
  │ frr-ce          │ 10.0.0.3 │ FRRouting    │  ● Up  │    100  │
  └─────────────────┴──────────┴──────────────┴────────┴──────────┘

  ── Add New Speaker ──────────────────────────────────────────────

  Step 1: Configure
    Name: [___________]  IP: [___________]  ASN: [_______]
    Vendor: [Cisco IOS-XR ▾]  Site: [___________]

  Step 2: BMP Config Snippet
    [Cisco IOS-XR ▾]
    ┌──────────────────────────────────────────────────────────┐
    │ bmp server 1                                             │
    │  host 172.20.0.100 port 5000                             │
    │  description rustybmp                                    │
    │  update-source Loopback0                                 │
    │  initial-delay 10                                        │
    │  stats-reporting-period 30                               │
    │  initial-refresh delay 15 spread 2                       │
    │ !                                                        │
    │ router bgp 65000                                         │
    │  bmp-activate server 1                                   │
    │ !                                                        │
    └──────────────────────────────────────────────────────────┘
    [📋 Copy]  [⬇ Download .cfg]

  Step 3: Test Connection
    [Test BMP Connection]  → ✅ Connected 2.3s ago — XRD 24.3.1

  Step 4: Onboarding Progress
    ████████░░ 7,234 / ~9,000 routes  [EOR: IPv4✅ IPv6⏳ VPNv4⏳]
```

### 4.8 Page 7: BMP Stats Viewer (`/stats/+page.svelte`)

RFC 9972 stats visualized. No other open-source tool has this.

```
BMP Statistics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Speaker: [10.0.0.1 ▾]  Peer: [10.0.0.2 AS65001 ▾]  Period: [24h]

Route Counts:
  ┌─────────────────────────────────────────────────────────────────────────┐
  │ Pre-policy Adj-RIB-In      12,450  ████████████████████████ 100%        │
  │ Post-policy Adj-RIB-In     11,892  ███████████████████████░  95.5%      │
  │ Rejected (policy)              558 ░░░░░░░░░░░░░░░░░░░████   4.5%      │
  │ Adj-RIB-Out pre-policy     10,901  ██████████████████████░   87.6%      │
  └─────────────────────────────────────────────────────────────────────────┘

Security Stats:
  ┌─────────────────────────────────────────────────────────────────────────┐
  │ RPKI invalidated (type 35)  3,201  ████████░░░░░░░░░░░░░░░░  25.7% ⚠  │
  │ RPKI not-valid (type 36)    3,410  ████████░░░░░░░░░░░░░░░░  27.4% ⚠  │
  │ Max AS_PATH rejected        0      ░░░░░░░░░░░░░░░░░░░░░░░░   0%   ✅  │
  └─────────────────────────────────────────────────────────────────────────┘

Stale Routes:
  GR stale (type 27):  0    ✅
  LLGR stale (type 28): 0   ✅

History (24h sparklines — one row per stat counter):
[sparkline grid: type-0 to type-38, each as mini time-series]
```

### 4.9 Page 8: SR Policy View (`/srpolicy/+page.svelte`)

```
SR Policies
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Speaker: [10.0.0.1 ▾]

Active SR Policies:
┌───────┬──────────────┬───────────────────────────────────┬────────────────┐
│ Color │ Endpoint     │ Best Candidate Path                │ Status         │
├───────┼──────────────┼───────────────────────────────────┼────────────────┤
│   100 │ 10.0.0.5     │ Pref:200 → SID:16001/16002/16003  │ ✅ Active      │
│   100 │ 10.0.0.5     │ Pref:100 → SID:16001/16005/16003  │ ⚪ Backup      │
│   200 │ 10.0.0.7     │ Pref:200 → SID:fcbb:1::/32        │ ✅ Active SRv6 │
└───────┴──────────────┴───────────────────────────────────┴────────────────┘

Segment Types:
  [A] MPLS Label: 16001     SID Node: 10.0.0.2
  [A] MPLS Label: 16002     SID Node: 10.0.0.3
  [B] SRv6 SID: fcbb:1::/32 Behavior: End.X (adj to 10.0.0.4)

Path Visualization:
[Topology map with SR path overlaid in emerald color]
```

### 4.10 Page 9: ML Insights (`/ml/+page.svelte`)

```
ML Anomaly Insights
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Model Status:
  Route Anomaly v1  IsolationForest  Trained: 2026-06-15  45,230 rows
  Contamination: 5%  Features: 8  [Retrain] [Download]

Recent Anomalies (last 24h):
  [Filter: All | Hijack | Churn | Flap | Leak]
┌──────────────────┬──────────┬─────────┬───────────────────────────────────┐
│ Prefix/Peer      │ Type     │ Score   │ Details                           │
├──────────────────┼──────────┼─────────┼───────────────────────────────────┤
│ 203.0.113.0/24   │ hijack   │ 0.92    │ Origin changed: AS64496→AS64498   │
│ 198.51.100.0/24  │ churn    │ -0.71   │ 47 events/hr vs baseline 2/hr     │
│ peer: 10.0.0.3   │ flap     │ 5 flaps │ 5 peer-down events in 10 min      │
└──────────────────┴──────────┴─────────┴───────────────────────────────────┘

Anomaly Rate (7-day trend):
[sparkline: anomaly count per day]

Model Training Data:
  Last export:  2026-06-19 02:00 UTC  (45,230 rows)
  Latest parquet: ml/data/route_anomaly/latest → 2026-06-19...parquet
  [Export Now] [View Schema]

Topology Snapshot Status (for STGNN):
  Buffer: T=8/8 snapshots  ✅ Ready for training
  Last snapshot: 5 min ago  All peers present: Yes
  [Train STGNN] [Download Snapshots]
```

### 4.11 Dashboard upgrades (`/+page.svelte`)

The current dashboard has 4 stat cards and a raw event feed. Upgrade to:

```
┌─ Health Bar ──────────────────────────────────────────────────────────────┐
│  rustybmp ● Up  14d 3h   │  BMP: 3 speakers ● ● ●  │  API: 847 req/min  │
└──────────────────────────────────────────────────────────────────────────┘

┌─ Stat Cards (with sparklines) ────────────────────────────────────────────┐
│  Peers Up      │  Total Routes   │  RPKI Valid %    │  Events/min        │
│     47          │   256,890       │    78.3%   ↑0.5  │    847 ↑          │
│  [sparkline]   │  [sparkline]    │  [donut mini]    │  [sparkline]       │
└──────────────────────────────────────────────────────────────────────────┘

┌─ Active Alerts ──────────────────────────────────────────────────────────┐
│  ⚠ CRITICAL  203.0.113.0/24  Origin changed (possible hijack)  14:23    │
│  ⚠ WARN      198.51.100.0/24  Churn spike Z=4.2                09:11    │
│  ℹ INFO      peer 10.0.0.3   LLGR stale routes: 12             06:44    │
└──────────────────────────────────────────────────────────────────────────┘

┌─ Top Churning Prefixes (24h) ──────┐  ┌─ Speaker Status Grid ───────────┐
│ 198.51.100.0/24    47 events  ████ │  │  xrd-pe1  ● Up  12,450 routes   │
│ 203.0.113.0/24     23 events  ██░  │  │  xrd-pe2  ● Up   3,892 routes   │
│ 192.0.2.0/24       11 events  █░░  │  │  frr-ce   ● Up     100 routes   │
└────────────────────────────────────┘  └─────────────────────────────────┘

┌─ Live Events (SSE) ──────────────────────────────────────────────────────┐
│ 14:44 [announce] 203.0.113.0/24 → AS65001 (AS64496 3356 64496)         │
│ 14:43 [withdraw] 198.51.100.0/24 → AS65001                             │
│ 14:43 [peer_up]  10.0.0.2 AS65001                                      │
│ [24h  7d  30d]                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## Part 5 — API Additions for Full UI-Backend Symphony

All 9 new UI pages require API endpoints. Consolidated list:

### New API endpoints

| Endpoint | Method | Handler | Epic |
|----------|--------|---------|------|
| `/api/routes/prefix/{p}/timeline` | GET | `prefix_timeline` | exists |
| `/api/routes/prefix/{p}/peers` | GET | `prefix_peers` | exists |
| `/api/routes/prefix/{p}/convergence` | GET | `prefix_convergence` | exists |
| `/api/routes/prefix/{p}/enrichment` | GET | `prefix_enrichment` (PeeringDB+RPKI) | RV6 |
| `/api/rpki/analysis` | GET | `rpki_analysis` | exists |
| `/api/rpki/impact` | GET | `rpki_enforcement_impact` | RV6 |
| `/api/rpki/coverage` | GET | `rpki_roa_coverage` | RV6 |
| `/api/policy?peer=X` | GET | `policy_delta` | exists |
| `/api/peers/{addr}/timeline` | GET | `peer_timeline` | exists |
| `/api/peers/{addr}/capabilities` | GET | `peer_capabilities` | RV6 |
| `/api/peers/{addr}/stats` | GET | `peer_stats_detail` | RV6 |
| `/api/aspath?prefix=X` | GET | `aspath_graph` | RV6 |
| `/api/aspath/peers?prefix=X` | GET | `aspath_peers` | RV6 |
| `/api/onboard/register` | POST | `register_speaker` | RV6 |
| `/api/onboard/config/{addr}?vendor=X` | GET | `bmp_config_snippet` | RV6 |
| `/api/onboard/test/{addr}` | POST | `test_bmp_connection` | RV6 |
| `/api/onboard/progress/{addr}` | GET | `onboard_progress` | RV6 |
| `/api/stats/{speaker}/{peer}` | GET | `bmp_stats_history` | RV6 |
| `/api/stats/{speaker}/{peer}/sparklines` | GET | `bmp_stats_sparklines` | RV6 |
| `/api/srpolicy` | GET | `srpolicy_list` | RV6 |
| `/api/ml/anomalies` | GET | `ml_anomalies` | RV6 |
| `/api/ml/model/status` | GET | `ml_model_status` | RV6 |
| `/api/ml/model/retrain` | POST | `trigger_retrain` | RV6 |
| `/api/filters/test` | POST | `filter_test` | RV6 |
| `/api/filters/reload` | POST | `filter_reload` | RV6 |
| `/api/filters/stats` | GET | `filter_stats` | RV6 |
| `/api/bgpls/graph` | GET | `bgpls_graph` | exists (with TTL cache) |
| `/api/bgpls/path?from=X&to=Y` | GET | `bgpls_path` (shortest path) | RV6 |

### New DuckDB queries needed

| Query | Table | Used by |
|-------|-------|---------|
| `rpki_enforcement_impact()` | route_events | RPKI page |
| `rpki_roa_coverage(owned_prefixes)` | route_events | RPKI page |
| `policy_delta(peer_addr)` | route_events (pre vs post rib_type) | Policy page |
| `aspath_graph(prefix)` | route_events (parse AS paths) | AS Path viz |
| `peer_stats_detail(peer_addr)` | stats_events | Peer detail + stats page |
| `bmp_stats_sparklines(peer_addr, hours)` | stats_events | Stats page |
| `ml_anomalies_recent(since)` | ml_anomalies | ML page |
| `srpolicy_current()` | srpolicy_events | SR Policy page |
| `bgpls_path(src_router_id, dst_router_id)` | bgpls_links | Topology page |

### Schema additions

```sql
-- SR Policy events table (was deferred since RV3-1)
CREATE TABLE IF NOT EXISTS srpolicy_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    action          VARCHAR     NOT NULL,
    discriminator   UINTEGER,
    color           UINTEGER,
    endpoint        VARCHAR,
    best_preference UINTEGER,
    segment_count   UINTEGER,
    segments_json   VARCHAR     -- JSON array of segment descriptions
);

-- ASPA validation results (RV6 protocol addition)
CREATE TABLE IF NOT EXISTS aspa_validations (
    occurred_at     TIMESTAMPTZ NOT NULL,
    prefix          VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    as_path         VARCHAR,
    aspa_verdict    VARCHAR     -- 'valid' | 'invalid' | 'unknown'
);
```

---

## Part 6 — UI Scale Performance

### The problem at SP scale

At a large SP with 50 BGP speakers, 500 peers, and 900K routes each:
- `route_events` table: ~50M rows/day × 90-day retention = 4.5B rows
- Direct queries without careful indexing: unacceptable
- D3 force simulation with 5K+ BGP-LS nodes: browser freezes above ~500 nodes

### Solutions

#### Virtual table component

```svelte
<!-- ui/src/lib/components/VirtualTable.svelte -->
<!-- Renders only visible rows, handles 100K+ row tables smoothly -->
<script lang="ts">
  let { rows = [], columns = [], rowHeight = 40 } = $props();
  let container: HTMLDivElement;
  let scrollTop = $state(0);
  let visibleStart = $derived(Math.floor(scrollTop / rowHeight));
  let visibleEnd = $derived(visibleStart + Math.ceil((container?.clientHeight ?? 600) / rowHeight) + 5);
  let visibleRows = $derived(rows.slice(visibleStart, visibleEnd));
  let totalHeight = $derived(rows.length * rowHeight);
  let paddingTop = $derived(visibleStart * rowHeight);
</script>
<div bind:this={container} class="overflow-auto" 
     style="height:600px"
     onscroll={(e) => scrollTop = e.currentTarget.scrollTop}>
  <div style="height:{totalHeight}px; position:relative">
    <table style="position:absolute; top:{paddingTop}px; width:100%">
      <tbody>
        {#each visibleRows as row}
          <tr>
            {#each columns as col}
              <td>{row[col.key]}</td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
```

#### BGP-LS topology: level-of-detail rendering

For large topologies (>500 nodes), switch from force-directed to hierarchical:

```javascript
// In topology/+page.svelte: adaptive rendering
function chooseLayout(nodeCount) {
  if (nodeCount <= 100) return 'force';       // D3 force-directed
  if (nodeCount <= 1000) return 'hierarchical'; // D3 tree layout
  return 'clustered';                          // cluster by AS, collapse to AS-level
}
```

#### SSE event batching to prevent browser freezes

```typescript
// In api.ts: batch rapid SSE events
let eventBuffer: [string, unknown][] = [];
let rafPending = false;

export function openEventStream(callback: (type: string, data: unknown) => void): EventSource {
  const es = new EventSource('/api/events');
  es.onmessage = (e) => {
    const { kind, ...rest } = JSON.parse(e.data);
    eventBuffer.push([kind, rest]);
    if (!rafPending) {
      rafPending = true;
      requestAnimationFrame(() => {
        const batch = eventBuffer.splice(0);
        for (const [type, data] of batch) callback(type, data);
        rafPending = false;
      });
    }
  };
  return es;
}
```

#### DuckDB query optimization for large tables

Add to `query.rs` — all analytical queries must use LIMIT + time-bounded WHERE:
```rust
// Never: SELECT * FROM route_events (can be 4.5B rows)
// Always: 
let sql = "SELECT ... FROM route_events WHERE occurred_at >= NOW() - INTERVAL '7 days' LIMIT 1000";
```

Add composite index:
```sql
-- For prefix timeline queries
CREATE INDEX IF NOT EXISTS idx_route_events_prefix_time
ON route_events (prefix, occurred_at DESC);

-- For stats viewer
CREATE INDEX IF NOT EXISTS idx_stats_peer_type_time
ON stats_events (peer_addr, counter_name, occurred_at DESC);
```

---

## Part 7 — RV6 Epic Index

### Epic RV6-1: Roto Filter Language

| Task | File |
|------|------|
| T1: Add roto crate | `crates/rbmp-rib/Cargo.toml` |
| T2: Register BGP types | `crates/rbmp-rib/src/roto_ctx.rs` (new) |
| T3: RotoFilterEngine struct | `crates/rbmp-rib/src/filter.rs` (rewrite) |
| T4: Default filter script | `config/filters.roto` (new) |
| T5: Wire into RibManager | `crates/rbmp-rib/src/manager.rs` |
| T6: Hot-reload via inotify | `crates/rbmp-server/src/filter_watcher.rs` (new) |
| T7: Filter test endpoint | `crates/rbmp-server/src/api/mod.rs` + handler |
| T8: Filter stats Prometheus | `crates/rbmp-server/src/api/health.rs` |

### Epic RV6-2: Protocol (ASPA, BGPsec-parse, MCAST-VPN, uSID)

| Task | File |
|------|------|
| T1: ASPA RTR client | `crates/rbmp-enrichment/src/aspa.rs` (new) |
| T2: ASPA route context | `crates/rbmp-rib/src/roto_ctx.rs` |
| T3: BGPsec_Path attribute parse (type 30) | `crates/rbmp-core/src/bgp/attributes.rs` |
| T4: MCAST-VPN full RFC 6514 | `crates/rbmp-core/src/bgp/mvpn.rs` (new) |
| T5: SRv6 uSID | `crates/rbmp-core/src/bgp/srv6.rs` |
| T6: BGP unnumbered (RFC 5549) | `crates/rbmp-core/src/bgp/nlri.rs` |
| T7: ASPA DuckDB table + Prometheus | `schema.rs`, `health.rs` |

### Epic RV6-3: UI — Component Library

| Task | File |
|------|------|
| T1: TimelineChart.svelte | `ui/src/lib/components/TimelineChart.svelte` |
| T2: AsnSankey.svelte | `ui/src/lib/components/AsnSankey.svelte` |
| T3: RpkiBadge.svelte | `ui/src/lib/components/RpkiBadge.svelte` |
| T4: VirtualTable.svelte | `ui/src/lib/components/VirtualTable.svelte` |
| T5: MetricCard.svelte | `ui/src/lib/components/MetricCard.svelte` |
| T6: BatchedEventSource.ts | `ui/src/lib/sse.ts` (RAF batching) |
| T7: SSE event batching | `ui/src/lib/api.ts` |

### Epic RV6-4: UI — 9 Missing Pages

| Task | Route |
|------|-------|
| T1: Prefix Explorer | `/prefix/[prefix]/+page.svelte` |
| T2: Peer Detail | `/peers/[addr]/+page.svelte` |
| T3: AS Path Visualizer | `/aspath/+page.svelte` |
| T4: RPKI Analysis | `/rpki/+page.svelte` |
| T5: Policy Analysis | `/policy/+page.svelte` |
| T6: Onboarding Wizard | `/onboard/+page.svelte` |
| T7: BMP Stats Viewer | `/stats/+page.svelte` |
| T8: SR Policy View | `/srpolicy/+page.svelte` |
| T9: ML Insights | `/ml/+page.svelte` |
| T10: Dashboard upgrade | `/+page.svelte` |

### Epic RV6-5: API Completions

| Task | File |
|------|------|
| T1: `prefix_enrichment` | `api/routes.rs` |
| T2: `rpki_impact` + `rpki_coverage` | `api/routes.rs` |
| T3: `peer_capabilities` | `api/peers.rs` |
| T4: `aspath_graph` + `aspath_peers` | `api/topology.rs` |
| T5: Onboarding APIs | `api/onboard.rs` (new) |
| T6: `bmp_stats_history` + sparklines | `api/stats.rs` |
| T7: `srpolicy_list` | `api/topology.rs` |
| T8: `ml_anomalies` + model status | `api/ml.rs` (new) |
| T9: `filter_test` + `filter_reload` | `api/filters.rs` (new) |
| T10: `bgpls_path` shortest path | `api/topology.rs` |

### Epic RV6-6: DuckDB + Schema

| Task | File |
|------|------|
| T1: `srpolicy_events` table | `schema.rs` |
| T2: `aspa_validations` table | `schema.rs` |
| T3: Composite indexes | `schema.rs` |
| T4: `policy_delta()` query | `query.rs` |
| T5: `rpki_enforcement_impact()` | `query.rs` |
| T6: `aspath_graph()` | `query.rs` |
| T7: `srpolicy_current()` + `ml_anomalies_recent()` | `query.rs` |
| T8: Writer: SR Policy events | `writer.rs` |

### Epic RV6-7: Scale Performance

| Task | File |
|------|------|
| T1: VirtualTable component | `ui/src/lib/components/VirtualTable.svelte` |
| T2: Topology level-of-detail | `ui/src/routes/topology/+page.svelte` |
| T3: SSE RAF batching | `ui/src/lib/api.ts` |
| T4: DuckDB time-bounded queries | `query.rs` (audit all queries) |
| T5: Topology graph cache | `api/topology.rs` (60s TTL) |

---

## Part 8 — Feature Completion Matrix (Post-RV6)

| Feature | Kentik | ThousandEyes | NLNOG LG | gobmp | rustybmp RV6 |
|---------|--------|-------------|----------|-------|--------------|
| Prefix timeline chart | ✅ | ✅ | ❌ | ❌ | ✅ |
| AS path Sankey | ✅ | ✅ | ✅ | ❌ | ✅ |
| Multi-peer path comparison | ✅ | ✅ | ✅ | ❌ | ✅ |
| RPKI analysis page | ✅ | ✅ | ✅ | ❌ | ✅ |
| Policy analysis (pre/post BMP) | ❌ | ❌ | ❌ | ❌ | ✅ (unique!) |
| Peer session timeline | ✅ | ✅ | ❌ | ❌ | ✅ |
| Device onboarding wizard | ❌ | ❌ | ❌ | ❌ | ✅ |
| BMP Stats viewer (RFC 9972) | ❌ | ❌ | ❌ | ❌ | ✅ (unique!) |
| SR Policy view | ❌ | ❌ | ❌ | ❌ | ✅ (unique!) |
| ML anomaly detection | ✅ AI | ✅ AI | ❌ | ❌ | ✅ |
| Programmable filter language | ❌ | ❌ | ❌ | ❌ | ✅ Roto JIT |
| ASPA validation | ❌ | Partial | ✅ | ❌ | ✅ |
| Virtual scroll (scale) | ✅ | ✅ | ❌ | ❌ | ✅ |
| BGP unnumbered (AI DC fabric) | ✅ | ✅ | ❌ | ❌ | ✅ |

**Unique advantages after RV6**: Policy Analysis (pre vs post-policy BMP visibility), BMP Stats viewer (RFC 9972), SR Policy view, Roto JIT filter language. These capabilities exist nowhere else in open-source BGP monitoring.

---

## Part 9 — Notes for Next Session

Next upload: `rv6_all_changes.patch`. The diff should include all 7 epics. The largest individual piece is RV6-4 (9 UI pages). If the developer splits the sprint, suggest delivering RV6-1 (Roto filter) and RV6-4 T1-T5 (first 5 pages) first, then RV6-4 T6-T10 + all other epics in a second commit.

**Important**: Roto v0.11 API may require specific registration patterns. Check `docs.rs/roto/0.11` for the correct `Runtime` and `Compiler` API. The architecture in §2.2 is based on Roto v0.11 documentation but specific method names may differ slightly — follow the actual crate API.

---

*End of RUSTYBMP_BACKLOG_RV6.md — Sprint RV6*
