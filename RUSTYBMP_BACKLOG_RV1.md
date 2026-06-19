# RustyBMP — Sprint RV1 Backlog
## BGP Monitoring Protocol Core + Protocol Completeness

> **Version**: RV1  
> **Date**: 2026-06-18  
> **Basis**: Full code audit of `rustybmp-main` (4 crates) + `bonsai-main` (partial extraction) + RFC 7854 / RFC 8671 / RFC 9069 / RFC 9972 (May 2026) + RFC 7911 / RFC 7432 / RFC 7752  
> **Principle**: Everything in this document is grounded in actual existing code. All new tasks are additive and isolated to named files/crates. No existing interfaces are broken unless explicitly noted.

---

## Part 1 — Project Identity & Ground Rules

### What rustybmp is

RustyBMP is a **Rust-first, scalable BGP Monitoring Protocol (BMP) collector** focused exclusively on BMP/BGP. Unlike bonsai (which ingests telemetry broadly — gNMI, syslog, SNMP, BMP as one of many sources), rustybmp treats BMP as the primary operational plane. It collects, parses, stores, and analyses every BMP message at the deepest possible level: every path attribute, every community, every capability, every RIB type, every stats counter.

The output of rustybmp feeds a Python analytics layer (`rbmppy`) that exposes ML-based BGP anomaly detection, integration with internet intelligence (PeeringDB, RPKI, IRR, RIPE STAT), and a clean API for operators to build custom monitoring tooling.

**Target users**: network operators at service providers, large enterprises, IXPs, and anyone running BGP at scale. The tool must be trustable at hyperscaler volume.

### Non-goals for RV1

- gNMI / SNMP / syslog ingestion (that's bonsai's job)
- Full UI dashboard (deferred to RV3+)
- Multi-collector HA (deferred to RV2)

### Architecture axiom

Every crate in the workspace must compile independently and cleanly. If a change to rbmp-core breaks rbmp-rib, the interface contract between them was wrong. Keep crates small, keep their `pub` surfaces minimal, and document every `pub` function.

---

## Part 2 — Existing Codebase Analysis

### What already exists (do not re-implement)

**Crate: `rbmp-core`** (`crates/rbmp-core/`)

Complete, well-written BMP/BGP parsing library. No I/O.

- `bmp/types.rs`: All 7 BMP message types (RFC 7854), PeerHeader, PeerFlags, PeerType (0=Global, 1=RD, 2=Local/Adj-RIB-Out RFC 8671, 3=Loc-RIB RFC 9069), RibType enum (AdjRibInPrePolicy / PostPolicy / Out variants / LocRib / LocRibFiltered), BmpMessage envelope with UUID + timestamp.
- `bmp/parser.rs`: Frame-based parser (`parse_bmp_message`). Handles initiation TLVs, termination TLVs, peer up (parses both OPEN PDUs), peer down (all 6 reason codes including VrfDown RFC 9069), route monitoring, stats report (stat types 0–19), route mirroring.
- `bgp/types.rs`: AFI (IPv4/IPv6/L2Vpn/BgpLs), SAFI (Unicast/Multicast/LabeledUnicast/MplsVpn/Evpn/Flowspec/FlowspecVpn), AfiSafi, Prefix (V4/V6/Labeled/Vpn), RouteDistinguisher (type 0/1/2 display), AsPath with segment types (Set/Sequence/ConfedSequence/ConfedSet), StandardCommunity (well-known: NO_EXPORT, NO_ADVERTISE, BLACKHOLE, GRACEFUL_SHUTDOWN, ACCEPT_OWN), ExtendedCommunity, LargeCommunity, Origin, BgpCapability (all standard codes), PathAttributes (all standard attributes), BgpUpdate with EOR detection.
- `bgp/attributes.rs`: Full path attribute parser. Types 1-18 and 32 (Large Community). Unknown attributes preserved as RawAttribute.
- `bgp/capabilities.rs`: BGP capability parser. Codes 1,2,5,6,64,65,69,70,71,73.
- `bgp/nlri.rs`: NLRI decoders for IPv4/IPv6 unicast, labeled unicast (RFC 3107 / RFC 8277), VPN (RFC 4364). Next-hop decoder for IPv4/IPv6.
- `bgp/open.rs` / `bgp/update.rs`: BGP OPEN and UPDATE message parsers.
- `error.rs`: Typed error enum. Comprehensive.

**Crate: `rbmp-rib`** (`crates/rbmp-rib/`)

- `table.rs`: `RibTable` — per-RibType hash map of prefix string → RibEntry. Insert/remove/get/count/iter. Last-write-wins (no multi-path yet).
- `session.rs`: `PeerSession` (state machine: Unknown/Up/Down, flap counter, uptime, capabilities), `BmpSession` (speaker with peers HashMap).
- `manager.rs`: `RibManager` — owns all speaker sessions and RIB tables. Processes BmpMessage, emits `RibEvent` via Tokio broadcast channel. Handles EOR detection. Query surface: speakers(), speaker(addr), rib_for_peer(peer), total_routes(), total_peers_up().
- `event.rs`: `RibEvent` with `RibEventPayload` (SpeakerUp/Down, PeerUp/Down, RouteChange, Stats, EndOfRib), `RouteChange` (action + peer_header + rib_type + prefix + attributes).

**Crate: `rbmp-store`** (`crates/rbmp-store/`)

- `duck.rs`: DuckDB connection wrapper (`RouteStore`), open/in-memory, `checkpoint()`.
- `schema.rs`: 4 tables: `route_events` (prefix, action, all path attributes as strings), `peer_events`, `speaker_events`, `stats_events`. 7 indexes.
- `writer.rs`: Async task `run_store_writer` — consumes broadcast RibEvents, writes rows. All 6 RibEventPayload variants handled.
- `query.rs`: `QueryEngine` — current_rib (windowed latest-announce per prefix), prefix_history, route_changes, top_churning_prefixes, as_origin_counts, peer_history.

**Crate: `rbmp-server`** (`crates/rbmp-server/`)

- `receiver.rs`: BMP TCP listener with frame-length framing, `BytesMut` ring buffer, cancellation token, per-connection task with `handle_connection`, parse-errors skip frame without disconnecting.
- `config.rs`: TOML config: BmpConfig (listen_addr, max_frame_bytes, shed_stats_on_pressure, archive_path), HttpConfig (listen_addr, serve_ui, cors_origins), StoreConfig (db_path, in_memory, event_capacity, checkpoint_secs), LogConfig.
- `main.rs`: Wire-up: store → rib → receiver → store_writer → HTTP. Uses CancellationToken for graceful shutdown.
- `api/mod.rs`: Axum router. Endpoints: /health, /metrics, /api/speakers[/:addr], /api/peers[/:addr][/rib], /api/routes, /api/routes/prefix, /api/routes/changes, /api/analytics/churn, /api/analytics/origins, /api/events (SSE).

**Python: `bmppy/`**

Currently a stub — only `__init__.py` and an `analytics.py` that is missing (404 at runtime). Needs complete rebuild as rbmppy.

---

## Part 3 — Crate Architecture: Current + Future

```
rustybmp workspace
├── crates/
│   ├── rbmp-core/           ← EXISTING: pure parsing, zero I/O, no tokio
│   │   ├── src/bmp/         ← BMP RFC 7854/8671/9069 parser
│   │   └── src/bgp/         ← BGP UPDATE/OPEN/NLRI parser
│   │
│   ├── rbmp-rib/            ← EXISTING: in-memory state machine, tokio broadcast
│   │
│   ├── rbmp-store/          ← EXISTING: DuckDB persistence
│   │
│   ├── rbmp-server/         ← EXISTING: TCP receiver, HTTP API, main
│   │
│   ├── rbmp-enrichment/     ← NEW RV2: RPKI/ROA, PeeringDB, IRR lookups
│   │                            async, caches results, feeds RibManager annotations
│   │
│   └── rbmp-analytics/      ← NEW RV2: churn detection, hijack/leak scoring,
│                                convergence tracking, built on DuckDB queries
│
├── bmppy/                   ← REBUILD RV1: Python SDK (rbmppy package)
│   ├── rbmppy/
│   │   ├── client.py        ← async HTTP client wrapping rustybmp API
│   │   ├── stream.py        ← SSE event streaming
│   │   ├── models.py        ← typed Pydantic models
│   │   ├── analytics.py     ← ML feature extraction, anomaly detection
│   │   └── enrichment.py    ← PeeringDB/RPKI wrappers
│   └── pyproject.toml
│
├── lab/                     ← NEW RV1: ContainerLab topologies + XRD configs
│   ├── xrd-bmp.clab.yml
│   ├── configs/xrd/
│   └── README.md
│
└── config/
    └── rustybmp.toml.example
```

### Crate sizing rule

Every `src/` file in rbmp-core should be ≤ 500 lines. If a parser grows beyond that, split it. Example: `bgp/evpn.rs`, `bgp/bgpls.rs`, `bgp/flowspec.rs` are separate files, not added to `bgp/nlri.rs`.

---

## Part 4 — Dev Workflow (Mac ↔ GitHub ↔ Ubuntu ↔ XRD)

### Daily cycle

```
Mac (VSCode / rust-analyzer)
  │
  ├─ cargo build --workspace       # verify it builds
  ├─ cargo test --workspace        # all unit tests green
  ├─ git commit -m "feat: ..."
  └─ git push origin main
         │
         ▼
Ubuntu 24 (test machine)
  │
  ├─ git pull
  ├─ cargo build --release
  ├─ ./rustybmp config/rustybmp.toml &
  └─ containerlab deploy -t lab/xrd-bmp.clab.yml
         │
         ▼
XRD containers (BMP speakers)
  └─ bmp server 1 → Ubuntu:5000
```

### How to produce a diff file for the next Claude session

After a coding session on Mac:
```bash
#!/usr/bin/env bash
# scripts/dev/make_diff.sh
# Run from repo root after committing changes.
# Usage: ./scripts/dev/make_diff.sh <from_tag_or_commit> [to_commit]

FROM=${1:-$(git describe --tags --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)}
TO=${2:-HEAD}
OUT="diffs/diff_$(date +%Y%m%d_%H%M)_${FROM:0:8}_${TO:0:8}.patch"
mkdir -p diffs
git diff "$FROM" "$TO" -- 'crates/**/*.rs' 'bmppy/**/*.py' 'lab/**/*.yml' \
    'config/**' 'Cargo.toml' > "$OUT"
echo "Diff written to $OUT"
echo "Files changed:"
git diff --name-only "$FROM" "$TO"
```

The `.patch` file (or the listed changed files) is what gets uploaded to Claude in the next conversation. **Upload only the diff, not the full repo zip.**

### Branch strategy for this project

- `main` — always compilable, passing tests
- `feat/rv1-evpn` — feature branches per epic
- Merge via PR with `cargo test` gate

---

## Part 5 — Bonsai Extraction: What to Reuse

The following patterns from bonsai (`bonsai-main`) are directly applicable to rustybmp. Extract and adapt — do not copy wholesale.

### 5.1 Resource Governor (pressure shedding)

**From**: `src/resource_governor.rs`  
**What it does**: Tracks memory pressure and rate shedding. When `should_shed()` is true, low-value messages (StatsReport) are dropped before they hit the parser or RIB.  
**Adaptation for rustybmp**: Already partially implemented in `rbmp-server/src/config.rs` (`shed_stats_on_pressure`). Expand to a proper `GovernorHandle` in `rbmp-server/src/governor.rs` that monitors Tokio channel backpressure and process RSS, and passes a `should_shed()` check in `handle_connection` before dispatching stats messages.

### 5.2 Archive (JSONL raw PDU capture)

**From**: bonsai `src/streaming/bmp.rs` — `JsonLineArchive`  
**What it does**: Appends every BmpEvent as JSONL to a file for replay/debugging.  
**Adaptation**: `rbmp-server` already has `archive_path` in `BmpConfig`. The `receiver.rs` needs to wire this up. Create `crates/rbmp-server/src/archive.rs` that mirrors bonsai's `JsonLineArchive` pattern (async file, Mutex-guarded write, append-only).

### 5.3 ContainerLab + XRD config patterns

**From**: `lab/sp/configs/xrd/PE1.cfg` — the BMP section:
```
bmp server 1
 host <ubuntu-ip> port 5000
 description rustybmp
 update-source Loopback0
 initial-delay 30
 stats-reporting-period 30
 initial-refresh delay 30 spread 2
!
router bgp <ASN>
 bmp-activate server 1
!
```
This is the canonical XRD BMP config. The `initial-refresh delay 30 spread 2` is essential — without it, large RIB dumps can overwhelm the collector on reconnect.

**Also**: bonsai's XRD configs for VRF, VPN, EVPN, SR-MPLS are in `lab/sp/configs/xrd/`. All of these can be adapted for rustybmp test topologies.

### 5.4 Python BGP rule patterns

**From**: `python/bonsai_sdk/rules/bgp.py`  
**BgpSessionDown / BgpSessionFlap / BgpAllPeersDown / BgpNeverEstablished** — these detector classes are the right level of abstraction for rbmppy. Port them to use rustybmp's SSE event stream instead of bonsai's gRPC.

### 5.5 Chaos testing patterns

**From**: `chaos_plans/bgp_heavy.yaml` and `scripts/chaos_runner.py`  
**What it does**: Injects BGP flaps, link drops, VRF changes via ContainerLab exec.  
**Adaptation**: Create `lab/chaos/bgp_chaos.yaml` for rustybmp test scenarios (peer flap, mass withdrawal, policy change simulation).

### 5.6 DV4/EV1 backlog structure

The bonsai backlog format (numbered tasks, ✅ batch completion tags, code-level specificity) is exactly what we should follow. Each task in this backlog names the exact file and function to create/modify. Nothing is vague.

---

## Part 6 — RFC Reference Summary

### BMP RFCs (all must be implemented)

| RFC | Title | Status in v0.1 | RV1 Gap |
|-----|-------|----------------|---------|
| RFC 7854 | BMP core (messages 0-6, peer header, stats 0-17) | ✅ Complete | Stats types up to 19 only |
| RFC 8671 | Adj-RIB-Out (peer type 2, O flag) | ✅ Complete | RIB storage partial |
| RFC 9069 | Loc-RIB (peer type 3, F flag) | ✅ Complete | — |
| RFC 9972 | Advanced BMP stats (types 18-52+) | ❌ Missing | **Add RV1** |
| RFC 7911 | Add-Path | ✅ Capability parsed | ❌ RIB not Add-Path aware |

### BGP RFCs (parsing coverage)

| RFC | Feature | Status in v0.1 | RV1 Gap |
|-----|---------|----------------|---------|
| RFC 4271 | BGP-4 core | ✅ | — |
| RFC 4760 | MP-BGP (MP_REACH/UNREACH) | ✅ | — |
| RFC 4364 | BGP/MPLS IP VPN (L3VPN) | ✅ NLRI decoder | — |
| RFC 7432 | BGP EVPN | ❌ Missing | **Add RV1** |
| RFC 7752 | BGP-LS | ❌ Stub AFI only | **Add RV1 basics** |
| RFC 5575 | BGP Flowspec | ❌ Stub SAFI | **Add RV1** |
| RFC 8669 | BGP Prefix-SID attribute (type 40) | ❌ Goes to RawAttribute | **Add RV1** |
| RFC 9012 | BGP Tunnel Encapsulation attr (type 23) | ❌ Goes to RawAttribute | **Add RV1 decode** |
| RFC 9234 | BGP OTC + BGP Role attr (types 35, 36 capability) | ❌ Missing | **Add RV1** |
| RFC 4724 | Graceful Restart capability | ✅ | EOR tracking per AFI-SAFI |
| RFC 9494 | Long-Lived Graceful Restart (LLGR) | ✅ Capability | LLGR stale tracking RV2 |
| RFC 6793 | 4-byte ASN (AS4_PATH, AS4_AGGREGATOR) | ✅ | — |
| RFC 8092 | Large Communities | ✅ | — |
| RFC 1997 | Standard Communities | ✅ | — |
| RFC 4360 | Extended Communities | ✅ | Route Target decode RV1 |
| RFC 2439 | Route Flap Damping | ❌ Not applicable to BMP parser | Stats type 26 decode |
| RFC 8950 | Extended Next-Hop Encoding | ✅ Capability | — |

### RFC 9972 — New Stats (May 2026, must implement)

These are brand-new stats types published May 2026. The current `stat_name()` function in `rbmp-core/src/bmp/types.rs` only handles types 0-19. The decoder in `bmp/parser.rs::parse_stats_report` handles 4-byte, 8-byte, and 7-byte (per-AFI/SAFI) lengths but doesn't name the new types.

New types to name and decode:

| Type | Description | Length | Scope |
|------|-------------|--------|-------|
| 18 | Pre-policy Adj-RIB-In routes (global) | 8 (Gauge) | Adj-RIB-In |
| 19 | Pre-policy Adj-RIB-In routes (per-AFI/SAFI) | 11 | Adj-RIB-In |
| 20 | Post-policy Adj-RIB-In routes (global) | 8 | Adj-RIB-In |
| 21 | Post-policy Adj-RIB-In routes (per-AFI/SAFI) | 11 | Adj-RIB-In |
| 22 | Per-AFI/SAFI pre-policy routes rejected by inbound policy | 11 | Adj-RIB-In |
| 23 | Per-AFI/SAFI post-policy routes accepted by inbound policy | 11 | Adj-RIB-In |
| 24 | Adj-RIB-Out pre-policy routes (global) | 8 | Adj-RIB-Out |
| 25 | Adj-RIB-Out pre-policy routes (per-AFI/SAFI) | 11 | Adj-RIB-Out |
| 26 | Per-AFI/SAFI routes suppressed by route-damping | 11 | Adj-RIB-In/Loc-RIB |
| 27 | Per-AFI/SAFI routes stale by Graceful Restart | 11 | Adj-RIB-In/Loc-RIB |
| 28 | Per-AFI/SAFI routes stale by LLGR (RFC 9494) | 11 | Adj-RIB-In/Loc-RIB |
| 29 | Routes left before received-route threshold (global) | 8 | Adj-RIB-In |
| 30 | Routes left before received-route threshold (per-AFI/SAFI) | 11 | Adj-RIB-In |
| 31 | Routes left before license threshold (global) | 8 | Adj-RIB-In/Loc-RIB |
| 32 | Routes left before license threshold (per-AFI/SAFI) | 11 | Adj-RIB-In/Loc-RIB |
| 33 | Pre-policy routes rejected due to max AS_PATH length (global) | 8 | Adj-RIB-In |
| 34 | Pre-policy routes rejected due to max AS_PATH length (per-AFI/SAFI) | 11 | Adj-RIB-In |
| 35 | Per-AFI/SAFI post-policy routes invalidated by RPKI/ROA | 11 | Adj-RIB-In |
| 36 | Per-AFI/SAFI routes not valid by RPKI/ROA (not-found + invalid) | 11 | Adj-RIB-In |
| 37 | Adj-RIB-Out post-policy routes (global) | 8 | Adj-RIB-Out |
| 38 | Adj-RIB-Out post-policy routes (per-AFI/SAFI) | 11 | Adj-RIB-Out |

**Key change**: Per-AFI/SAFI stats (odd types 19,21,22,23,25,26,27,28,30,32,34,35,36,38) use 11-byte value: 2-byte AFI + 1-byte SAFI + 8-byte Gauge. The current 7-byte handler in the parser was for RFC 7854's older per-AFI/SAFI format (types 9,10,13,14). The new 11-byte format must be handled separately.

---

## Part 7 — Sprint RV1 Epics

### Epic RV1-1: RFC 9972 Stats Decoder ✅ COMPLETE

**Scope**: `crates/rbmp-core/`  
**Files modified**: `src/bmp/types.rs`, `src/bmp/parser.rs`

#### RV1-1 T1 — Extend `stat_name()` with RFC 9972 types

**File**: `crates/rbmp-core/src/bmp/types.rs`

Add to the `stat_name()` match arm (after case 19):
```rust
18 => "pre-policy-adj-rib-in-routes",
19 => "per-afi-safi-pre-policy-adj-rib-in-routes",
20 => "post-policy-adj-rib-in-routes",
21 => "per-afi-safi-post-policy-adj-rib-in-routes",
22 => "per-afi-safi-pre-policy-routes-rejected-inbound",
23 => "per-afi-safi-post-policy-routes-accepted-inbound",
24 => "adj-rib-out-pre-policy-routes",
25 => "per-afi-safi-adj-rib-out-pre-policy-routes",
26 => "per-afi-safi-damped-routes",
27 => "per-afi-safi-gr-stale-routes",
28 => "per-afi-safi-llgr-stale-routes",
29 => "pre-route-limit-adj-rib-in",
30 => "per-afi-safi-pre-route-limit-adj-rib-in",
31 => "pre-license-limit-routes",
32 => "per-afi-safi-pre-license-limit-routes",
33 => "max-aspath-length-rejected",
34 => "per-afi-safi-max-aspath-length-rejected",
35 => "per-afi-safi-rpki-invalidated-routes",
36 => "per-afi-safi-rpki-not-valid-routes",
37 => "adj-rib-out-post-policy-routes",
38 => "per-afi-safi-adj-rib-out-post-policy-routes",
```

#### RV1-1 T2 — Add `StatGranularity` enum and decode 11-byte per-AFI/SAFI stats

**File**: `crates/rbmp-core/src/bmp/types.rs`

The old `StatEntry` only holds a `u64 value`. RFC 9972 per-AFI/SAFI stats (11 bytes) embed AFI + SAFI + count. Add a richer type:

```rust
/// Identifies which stat types use per-AFI/SAFI 11-byte encoding (RFC 9972)
pub fn stat_is_per_afi_safi_11byte(t: u16) -> bool {
    matches!(t, 19 | 21 | 22 | 23 | 25 | 26 | 27 | 28 | 30 | 32 | 34 | 35 | 36 | 38)
}

/// Which stat types use the older per-AFI/SAFI 7-byte format (RFC 7854 §4.8 types 9/10/13/14)
pub fn stat_is_per_afi_safi_7byte(t: u16) -> bool {
    matches!(t, 9 | 10 | 13 | 14)
}

/// Richer stat entry that tracks AFI/SAFI for per-address-family stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatEntry {
    pub stat_type:  u16,
    pub name:       &'static str,  // changed from String to &'static str — no allocation
    pub value:      u64,
    /// Present for per-AFI/SAFI stats (types with 7 or 11-byte value)
    pub afi_safi:   Option<AfiSafi>,
}
```

#### RV1-1 T3 — Update `parse_stats_report` to handle 11-byte stats

**File**: `crates/rbmp-core/src/bmp/parser.rs`

In `parse_stats_report`, the per-stat decode block currently handles:
- `4 => u32 as u64` 
- `8 => u64`
- `7 => per-AFI/SAFI (AFI:2 + SAFI:1 + count:4)`

Add:
```rust
// RFC 9972 per-AFI/SAFI 11-byte: AFI(2) + SAFI(1) + 64-bit Gauge(8)
11 if stat_is_per_afi_safi_11byte(stat_type) => {
    let afi  = u16::from_be_bytes([rest[pos], rest[pos+1]]);
    let safi = rest[pos+2];
    let val  = u64::from_be_bytes(rest[pos+3..pos+11].try_into().unwrap());
    stats.push(StatEntry {
        stat_type,
        name: stat_name(stat_type),
        value: val,
        afi_safi: Some(AfiSafi::new(afi, safi)),
    });
    pos += stat_len;
    continue;
}
```

Also update the 7-byte handler to populate `afi_safi`:
```rust
7 => {
    let afi  = u16::from_be_bytes([rest[pos], rest[pos+1]]);
    let safi = rest[pos+2];
    let val  = u32::from_be_bytes(rest[pos+3..pos+7].try_into().unwrap()) as u64;
    StatEntry {
        stat_type,
        name: stat_name(stat_type),
        value: val,
        afi_safi: Some(AfiSafi::new(afi, safi)),
    }
}
```

#### RV1-1 T4 — Update DuckDB stats schema for per-AFI/SAFI

**File**: `crates/rbmp-store/src/schema.rs`

Add columns to `stats_events`:
```sql
CREATE TABLE IF NOT EXISTS stats_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    counter_name    VARCHAR     NOT NULL,
    counter_value   UBIGINT     NOT NULL,
    stat_type       USMALLINT,          -- raw type code
    afi             USMALLINT,          -- NULL for global stats
    safi            UTINYINT            -- NULL for global stats
);
```

Update `persist()` in `crates/rbmp-store/src/writer.rs` to write `afi_safi` fields.

**Tests**: Add a unit test in `rbmp-core` that builds a StatsReport with type 35 (RPKI-invalidated, 11-byte) and verifies correct decode including AFI/SAFI.

---

### Epic RV1-2: EVPN NLRI Parser ✅ COMPLETE

**Scope**: `crates/rbmp-core/`  
**New file**: `crates/rbmp-core/src/bgp/evpn.rs`

EVPN is critical for DC fabric monitoring. The AFI=25 (L2VPN), SAFI=70 (EVPN) combination arrives in MP_REACH_NLRI/MP_UNREACH_NLRI. The current `dispatch_nlri_decode()` falls through to `decode_nlri()` which doesn't understand EVPN's variable-length route types.

#### RV1-2 T1 — Create `bgp/evpn.rs` with EVPN route type structs

RFC 7432 defines 5 route types:

```rust
// crates/rbmp-core/src/bgp/evpn.rs

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use serde::{Deserialize, Serialize};
use crate::{Error, Result};

/// RFC 7432 §7 — EVPN route types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvpnRoute {
    /// Type 1: Ethernet Auto-Discovery (A-D) route
    EthernetAutoDiscovery {
        rd:           [u8; 8],
        esi:          [u8; 10],  // 10-byte Ethernet Segment Identifier
        ethernet_tag: u32,
        mpls_label:   u32,
    },
    /// Type 2: MAC/IP Advertisement route
    MacIpAdvertisement {
        rd:           [u8; 8],
        esi:          [u8; 10],
        ethernet_tag: u32,
        mac:          [u8; 6],
        ip:           Option<IpAddr>,  // 0, 32, or 128-bit IP
        mpls_label1:  u32,
        mpls_label2:  Option<u32>,
    },
    /// Type 3: Inclusive Multicast Ethernet Tag route
    InclusiveMulticastEthernetTag {
        rd:           [u8; 8],
        ethernet_tag: u32,
        originating_router_ip: IpAddr,
    },
    /// Type 4: Ethernet Segment route
    EthernetSegment {
        rd:           [u8; 8],
        esi:          [u8; 10],
        originating_router_ip: IpAddr,
    },
    /// Type 5: IP Prefix route (RFC 9136)
    IpPrefix {
        rd:           [u8; 8],
        esi:          [u8; 10],
        ethernet_tag: u32,
        prefix:       IpAddr,
        prefix_len:   u8,
        gw_ip:        Option<IpAddr>,
        mpls_label:   u32,
    },
    /// Unknown type — preserved for future RFCs
    Unknown { route_type: u8, data: Vec<u8> },
}

impl EvpnRoute {
    pub fn route_type_name(&self) -> &'static str {
        match self {
            Self::EthernetAutoDiscovery { .. }    => "ethernet-auto-discovery",
            Self::MacIpAdvertisement { .. }        => "mac-ip-advertisement",
            Self::InclusiveMulticastEthernetTag { .. } => "inclusive-multicast-ethernet-tag",
            Self::EthernetSegment { .. }           => "ethernet-segment",
            Self::IpPrefix { .. }                  => "ip-prefix",
            Self::Unknown { route_type, .. }       => "unknown",
        }
    }
}

/// Decode EVPN NLRI from MP_REACH or MP_UNREACH attribute body.
/// Each EVPN route: Route-Type(1) + Length(1) + Value(Length)
pub fn decode_evpn_nlri(mut buf: &[u8]) -> Result<Vec<EvpnRoute>> {
    let mut routes = Vec::new();
    while buf.len() >= 2 {
        let route_type = buf[0];
        let length     = buf[1] as usize;
        buf = &buf[2..];
        if buf.len() < length {
            return Err(Error::UnexpectedEof { needed: length, have: buf.len() });
        }
        let value = &buf[..length];
        buf = &buf[length..];
        let route = parse_evpn_route(route_type, value)?;
        routes.push(route);
    }
    Ok(routes)
}

fn parse_evpn_route(route_type: u8, v: &[u8]) -> Result<EvpnRoute> {
    match route_type {
        1 => {
            // Type 1: RD(8) + ESI(10) + ETag(4) + Label(3) = 25 bytes
            if v.len() < 25 { return Err(Error::UnexpectedEof { needed: 25, have: v.len() }); }
            let mut rd = [0u8; 8]; rd.copy_from_slice(&v[0..8]);
            let mut esi = [0u8; 10]; esi.copy_from_slice(&v[8..18]);
            let ethernet_tag = u32::from_be_bytes([v[18], v[19], v[20], v[21]]);
            let mpls_label   = decode_mpls_label(&v[22..25]);
            Ok(EvpnRoute::EthernetAutoDiscovery { rd, esi, ethernet_tag, mpls_label })
        }
        2 => {
            // Type 2: RD(8) + ESI(10) + ETag(4) + MAClen(1) + MAC(6) + IPlen(1) + IP(0/4/16) + Label1(3) + Label2(3)?
            if v.len() < 33 { return Err(Error::UnexpectedEof { needed: 33, have: v.len() }); }
            let mut rd = [0u8; 8]; rd.copy_from_slice(&v[0..8]);
            let mut esi = [0u8; 10]; esi.copy_from_slice(&v[8..18]);
            let ethernet_tag = u32::from_be_bytes([v[18], v[19], v[20], v[21]]);
            // MAC length must be 48
            let mac_len = v[22];
            if mac_len != 48 { return Err(Error::BgpParse(format!("EVPN type2 mac_len={mac_len}, expected 48"))); }
            let mut mac = [0u8; 6]; mac.copy_from_slice(&v[23..29]);
            let ip_len  = v[29];
            let mut pos = 30;
            let ip = match ip_len {
                0  => None,
                32 => {
                    if v.len() < pos + 4 { return Err(Error::UnexpectedEof { needed: pos+4, have: v.len() }); }
                    let a = IpAddr::V4(Ipv4Addr::from([v[pos], v[pos+1], v[pos+2], v[pos+3]]));
                    pos += 4;
                    Some(a)
                }
                128 => {
                    if v.len() < pos + 16 { return Err(Error::UnexpectedEof { needed: pos+16, have: v.len() }); }
                    let mut b = [0u8; 16]; b.copy_from_slice(&v[pos..pos+16]);
                    pos += 16;
                    Some(IpAddr::V6(Ipv6Addr::from(b)))
                }
                _ => return Err(Error::BgpParse(format!("EVPN type2 ip_len={ip_len}"))),
            };
            if v.len() < pos + 3 { return Err(Error::UnexpectedEof { needed: pos+3, have: v.len() }); }
            let mpls_label1 = decode_mpls_label(&v[pos..pos+3]);
            let mpls_label2 = if v.len() >= pos + 6 { Some(decode_mpls_label(&v[pos+3..pos+6])) } else { None };
            Ok(EvpnRoute::MacIpAdvertisement { rd, esi, ethernet_tag, mac, ip, mpls_label1, mpls_label2 })
        }
        3 => {
            // Type 3: RD(8) + ETag(4) + IPlen(1) + IP(4 or 16)
            if v.len() < 13 { return Err(Error::UnexpectedEof { needed: 13, have: v.len() }); }
            let mut rd = [0u8; 8]; rd.copy_from_slice(&v[0..8]);
            let ethernet_tag = u32::from_be_bytes([v[8], v[9], v[10], v[11]]);
            let ip_len = v[12];
            let originating_router_ip = decode_evpn_ip(ip_len, &v[13..])?;
            Ok(EvpnRoute::InclusiveMulticastEthernetTag { rd, ethernet_tag, originating_router_ip })
        }
        4 => {
            // Type 4: RD(8) + ESI(10) + IPlen(1) + IP(4 or 16)
            if v.len() < 19 { return Err(Error::UnexpectedEof { needed: 19, have: v.len() }); }
            let mut rd = [0u8; 8]; rd.copy_from_slice(&v[0..8]);
            let mut esi = [0u8; 10]; esi.copy_from_slice(&v[8..18]);
            let ip_len = v[18];
            let originating_router_ip = decode_evpn_ip(ip_len, &v[19..])?;
            Ok(EvpnRoute::EthernetSegment { rd, esi, originating_router_ip })
        }
        5 => {
            // Type 5: RD(8)+ESI(10)+ETag(4)+IPlen(1)+IP(4/16)+GW_IP(4/16)+Label(3)
            if v.len() < 34 { return Err(Error::UnexpectedEof { needed: 34, have: v.len() }); }
            let mut rd = [0u8; 8]; rd.copy_from_slice(&v[0..8]);
            let mut esi = [0u8; 10]; esi.copy_from_slice(&v[8..18]);
            let ethernet_tag = u32::from_be_bytes([v[18], v[19], v[20], v[21]]);
            let prefix_len = v[22];
            let ip_octets = match prefix_len {
                0..=32  => 4,
                33..=128 => 16,
                _ => 4,
            };
            if v.len() < 23 + ip_octets + ip_octets + 3 {
                return Err(Error::UnexpectedEof { needed: 23 + ip_octets*2 + 3, have: v.len() });
            }
            let prefix = if ip_octets == 4 {
                IpAddr::V4(Ipv4Addr::from([v[23], v[24], v[25], v[26]]))
            } else {
                let mut b = [0u8; 16]; b.copy_from_slice(&v[23..39]);
                IpAddr::V6(Ipv6Addr::from(b))
            };
            let gw_off = 23 + ip_octets;
            let gw_ip = if v[gw_off..gw_off+ip_octets].iter().all(|&b| b == 0) {
                None
            } else if ip_octets == 4 {
                Some(IpAddr::V4(Ipv4Addr::from([v[gw_off], v[gw_off+1], v[gw_off+2], v[gw_off+3]])))
            } else {
                let mut b = [0u8; 16]; b.copy_from_slice(&v[gw_off..gw_off+16]);
                Some(IpAddr::V6(Ipv6Addr::from(b)))
            };
            let label_off = gw_off + ip_octets;
            let mpls_label = decode_mpls_label(&v[label_off..label_off+3]);
            Ok(EvpnRoute::IpPrefix { rd, esi, ethernet_tag, prefix, prefix_len, gw_ip, mpls_label })
        }
        _ => Ok(EvpnRoute::Unknown { route_type, data: v.to_vec() }),
    }
}

fn decode_mpls_label(b: &[u8]) -> u32 {
    // 24-bit field: top 20 bits = label, bit 0 = bottom-of-stack
    let raw = u32::from_be_bytes([0, b[0], b[1], b[2]]);
    raw >> 4
}

fn decode_evpn_ip(ip_len: u8, buf: &[u8]) -> Result<IpAddr> {
    match ip_len {
        32 => {
            if buf.len() < 4 { return Err(Error::UnexpectedEof { needed: 4, have: buf.len() }); }
            Ok(IpAddr::V4(Ipv4Addr::from([buf[0], buf[1], buf[2], buf[3]])))
        }
        128 => {
            if buf.len() < 16 { return Err(Error::UnexpectedEof { needed: 16, have: buf.len() }); }
            let mut b = [0u8; 16]; b.copy_from_slice(&buf[..16]);
            Ok(IpAddr::V6(Ipv6Addr::from(b)))
        }
        _ => Err(Error::BgpParse(format!("EVPN IP length {ip_len} not 32 or 128"))),
    }
}
```

#### RV1-2 T2 — Add `EvpnNlri` to `bgp/types.rs` and wire into `PathAttributes`

**File**: `crates/rbmp-core/src/bgp/types.rs`

Add alongside `MpReachNlri`:
```rust
/// EVPN NLRI carried in MP_REACH (AFI=25, SAFI=70)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvpnReachNlri {
    pub next_hops: Vec<IpAddr>,
    pub routes:    Vec<EvpnRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvpnUnreachNlri {
    pub routes: Vec<EvpnRoute>,
}
```

Add to `PathAttributes`:
```rust
pub evpn_reach:   Option<EvpnReachNlri>,
pub evpn_unreach: Option<EvpnUnreachNlri>,
```

#### RV1-2 T3 — Wire into `bgp/attributes.rs` dispatch

**File**: `crates/rbmp-core/src/bgp/attributes.rs`

In `dispatch_nlri_decode`, add the EVPN arm:
```rust
Safi::Evpn => {
    // Return Prefix::V4/V6 wrappers for now; EVPN routes also stored in evpn_reach
    // full decode happens in parse_mp_reach_evpn
    Vec::new() // prefixes field stays empty; evpn_reach captures the routes
}
```

In `parse_mp_reach`, after building `MpReachNlri`, check if `afi_safi == (L2Vpn, Evpn)`:
```rust
if afi_safi == AfiSafi::evpn() {
    let evpn_routes = decode_evpn_nlri(cur.remaining_slice())?;
    attrs.evpn_reach = Some(EvpnReachNlri { next_hops: ..., routes: evpn_routes });
}
```

#### RV1-2 T4 — Add EVPN to DuckDB schema

**File**: `crates/rbmp-store/src/schema.rs`

```sql
CREATE TABLE IF NOT EXISTS evpn_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    peer_as         UINTEGER    NOT NULL,
    action          VARCHAR     NOT NULL,   -- 'announce' | 'withdraw'
    route_type      UTINYINT    NOT NULL,   -- 1-5
    route_type_name VARCHAR     NOT NULL,
    rd              VARCHAR,               -- route distinguisher
    ethernet_tag    UINTEGER,
    mac             VARCHAR,               -- for type 2
    ip              VARCHAR,               -- for type 2/3/4/5
    prefix_len      UTINYINT,              -- for type 5
    mpls_label      UINTEGER,
    esi_hex         VARCHAR                -- 10-byte ESI as hex
);
```

**Tests**: Unit tests for each EVPN route type (1-5) in `crates/rbmp-core/src/bgp/evpn.rs`. Build byte arrays matching RFC 7432 examples and verify field extraction.

---

### Epic RV1-3: BGP Flowspec NLRI ✅ COMPLETE

**Scope**: `crates/rbmp-core/`  
**New file**: `crates/rbmp-core/src/bgp/flowspec.rs`

Flowspec (RFC 5575 for IPv4, RFC 8955 for IPv6) carries traffic filter rules as NLRI in AFI=1/2, SAFI=133/134. Each NLRI entry is a set of type-value component pairs.

#### RV1-3 T1 — Create `bgp/flowspec.rs`

```rust
// crates/rbmp-core/src/bgp/flowspec.rs

use serde::{Deserialize, Serialize};
use crate::{Error, Result};

/// A single Flowspec component (type + value)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowspecComponent {
    /// Type 1: Destination prefix
    DestPrefix  { prefix: String, prefix_len: u8 },
    /// Type 2: Source prefix
    SrcPrefix   { prefix: String, prefix_len: u8 },
    /// Type 3: IP Protocol (numeric operator + values)
    IpProtocol  { ops: Vec<NumericOp> },
    /// Type 4: Port
    Port        { ops: Vec<NumericOp> },
    /// Type 5: Destination port
    DstPort     { ops: Vec<NumericOp> },
    /// Type 6: Source port
    SrcPort     { ops: Vec<NumericOp> },
    /// Type 7: ICMP type
    IcmpType    { ops: Vec<NumericOp> },
    /// Type 8: ICMP code
    IcmpCode    { ops: Vec<NumericOp> },
    /// Type 9: TCP flags (bitmask operator)
    TcpFlags    { ops: Vec<BitmaskOp> },
    /// Type 10: Packet length
    PktLen      { ops: Vec<NumericOp> },
    /// Type 11: DSCP
    Dscp        { ops: Vec<NumericOp> },
    /// Type 12: Fragment flags
    Fragment    { ops: Vec<BitmaskOp> },
    Unknown     { component_type: u8, data: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumericOp {
    pub lt: bool, pub gt: bool, pub eq: bool,
    pub and_bit: bool,   // 0=OR, 1=AND between ops
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitmaskOp {
    pub not: bool, pub match_bit: bool, pub and_bit: bool,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowspecNlri {
    pub components: Vec<FlowspecComponent>,
    /// Human-readable summary: e.g. "dst=192.0.2.0/24 proto=6 dstport=80"
    pub summary:    String,
}

/// Decode a Flowspec NLRI from a byte buffer.
/// Each NLRI: length-encoded (1 or 2 bytes) + type-value components.
pub fn decode_flowspec_nlri(mut buf: &[u8], afi_is_ipv6: bool) -> Result<Vec<FlowspecNlri>> {
    let mut result = Vec::new();
    while !buf.is_empty() {
        // Length field: if first byte < 0xF0, length = first byte; else 2-byte length
        let (length, header_size) = if buf[0] < 0xF0 {
            (buf[0] as usize, 1)
        } else if buf.len() >= 2 {
            let len = ((buf[0] as usize & 0x0F) << 8) | buf[1] as usize;
            (len, 2)
        } else {
            break;
        };
        buf = &buf[header_size..];
        if buf.len() < length { break; }
        let nlri_bytes = &buf[..length];
        buf = &buf[length..];
        let nlri = parse_flowspec_nlri(nlri_bytes, afi_is_ipv6)?;
        result.push(nlri);
    }
    Ok(result)
}

fn parse_flowspec_nlri(mut buf: &[u8], afi_is_ipv6: bool) -> Result<FlowspecNlri> {
    let mut components = Vec::new();
    let mut summary_parts = Vec::new();
    while !buf.is_empty() {
        let comp_type = buf[0];
        buf = &buf[1..];
        let (comp, summary, consumed) = parse_flowspec_component(comp_type, buf, afi_is_ipv6)?;
        buf = &buf[consumed..];
        summary_parts.push(summary);
        components.push(comp);
    }
    Ok(FlowspecNlri { components, summary: summary_parts.join(" ") })
}

fn parse_flowspec_component(t: u8, buf: &[u8], ipv6: bool) -> Result<(FlowspecComponent, String, usize)> {
    match t {
        1 | 2 => {
            // Destination or Source prefix: prefix_len(1) + prefix bytes
            if buf.is_empty() { return Err(Error::UnexpectedEof { needed: 1, have: 0 }); }
            let prefix_len = buf[0];
            let octets = (prefix_len as usize + 7) / 8;
            if buf.len() < 1 + octets { return Err(Error::UnexpectedEof { needed: 1+octets, have: buf.len() }); }
            let prefix = if ipv6 {
                let mut a = [0u8; 16]; a[..octets].copy_from_slice(&buf[1..1+octets]);
                std::net::Ipv6Addr::from(a).to_string()
            } else {
                let mut a = [0u8; 4]; a[..octets.min(4)].copy_from_slice(&buf[1..1+octets.min(4)]);
                std::net::Ipv4Addr::from(a).to_string()
            };
            let label = if t == 1 { "dst" } else { "src" };
            let summary = format!("{label}={prefix}/{prefix_len}");
            let comp = if t == 1 {
                FlowspecComponent::DestPrefix { prefix, prefix_len }
            } else {
                FlowspecComponent::SrcPrefix { prefix, prefix_len }
            };
            Ok((comp, summary, 1 + octets))
        }
        3..=8 | 10 | 11 => {
            // Numeric operator-value pairs
            let (ops, consumed) = parse_numeric_ops(buf)?;
            let label = match t {
                3 => "proto", 4 => "port", 5 => "dstport", 6 => "srcport",
                7 => "icmptype", 8 => "icmpcode", 10 => "pktlen", 11 => "dscp", _ => "?",
            };
            let summary = format!("{}={}", label, ops.iter().map(|o| o.value.to_string()).collect::<Vec<_>>().join(","));
            let comp = match t {
                3  => FlowspecComponent::IpProtocol { ops },
                4  => FlowspecComponent::Port { ops },
                5  => FlowspecComponent::DstPort { ops },
                6  => FlowspecComponent::SrcPort { ops },
                7  => FlowspecComponent::IcmpType { ops },
                8  => FlowspecComponent::IcmpCode { ops },
                10 => FlowspecComponent::PktLen { ops },
                _  => FlowspecComponent::Dscp { ops },
            };
            Ok((comp, summary, consumed))
        }
        9 | 12 => {
            // Bitmask operator-value pairs
            let (ops, consumed) = parse_bitmask_ops(buf)?;
            let summary = format!("{}=0x{:x}", if t==9 { "tcpflags" } else { "fragment" },
                ops.first().map(|o| o.value).unwrap_or(0));
            let comp = if t == 9 {
                FlowspecComponent::TcpFlags { ops }
            } else {
                FlowspecComponent::Fragment { ops }
            };
            Ok((comp, summary, consumed))
        }
        _ => {
            // Unknown: consume until end-of-list (eol bit) or end of buffer
            Ok((FlowspecComponent::Unknown { component_type: t, data: buf.to_vec() },
                format!("unknown-type-{t}"), buf.len()))
        }
    }
}

fn parse_numeric_ops(buf: &[u8]) -> Result<(Vec<NumericOp>, usize)> {
    let mut ops = Vec::new();
    let mut pos = 0;
    loop {
        if pos >= buf.len() { break; }
        let op_byte = buf[pos]; pos += 1;
        let eol      = op_byte & 0x80 != 0;
        let and_bit  = op_byte & 0x40 != 0;
        let len_code = (op_byte >> 4) & 0x03;
        let lt       = op_byte & 0x04 != 0;
        let gt       = op_byte & 0x02 != 0;
        let eq       = op_byte & 0x01 != 0;
        let vlen = 1usize << len_code;  // 1, 2, 4, or 8 bytes
        if pos + vlen > buf.len() { break; }
        let value = match vlen {
            1 => buf[pos] as u64,
            2 => u16::from_be_bytes([buf[pos], buf[pos+1]]) as u64,
            4 => u32::from_be_bytes(buf[pos..pos+4].try_into().unwrap()) as u64,
            8 => u64::from_be_bytes(buf[pos..pos+8].try_into().unwrap()),
            _ => 0,
        };
        pos += vlen;
        ops.push(NumericOp { lt, gt, eq, and_bit, value });
        if eol { break; }
    }
    Ok((ops, pos))
}

fn parse_bitmask_ops(buf: &[u8]) -> Result<(Vec<BitmaskOp>, usize)> {
    let mut ops = Vec::new();
    let mut pos = 0;
    loop {
        if pos >= buf.len() { break; }
        let op_byte = buf[pos]; pos += 1;
        let eol       = op_byte & 0x80 != 0;
        let and_bit   = op_byte & 0x40 != 0;
        let len_code  = (op_byte >> 4) & 0x03;
        let not_bit   = op_byte & 0x02 != 0;
        let match_bit = op_byte & 0x01 != 0;
        let vlen = 1usize << len_code;
        if pos + vlen > buf.len() { break; }
        let value = match vlen {
            1 => buf[pos] as u64,
            2 => u16::from_be_bytes([buf[pos], buf[pos+1]]) as u64,
            _ => 0,
        };
        pos += vlen;
        ops.push(BitmaskOp { not: not_bit, match_bit, and_bit, value });
        if eol { break; }
    }
    Ok((ops, pos))
}
```

#### RV1-3 T2 — Wire Flowspec into PathAttributes and dispatch

**File**: `crates/rbmp-core/src/bgp/types.rs`  
Add:
```rust
pub flowspec_reach:   Option<Vec<FlowspecNlri>>,
pub flowspec_unreach: Option<Vec<FlowspecNlri>>,
```

**File**: `crates/rbmp-core/src/bgp/attributes.rs`  
In `dispatch_nlri_decode`, add:
```rust
Safi::Flowspec | Safi::FlowspecVpn => {
    let is_ipv6 = matches!(afi_safi.afi, Afi::Ipv6);
    decode_flowspec_nlri(cur.remaining_slice(), is_ipv6)
        .map(|_| Vec::new()) // Flowspec routes stored in flowspec_reach, not in prefixes
}
```

**Tests**: Unit test for Flowspec type-1 (prefix), type-3 (protocol), type-4 (port), and type-9 (TCP flags) component decode.

---

### Epic RV1-4: Advanced Path Attributes ✅ COMPLETE

**Scope**: `crates/rbmp-core/src/bgp/attributes.rs`, new `bgp/srv6.rs`

#### RV1-4 T1 — BGP Prefix-SID attribute (RFC 8669, type 40)

Currently goes to `RawAttribute`. Add decode.

**File**: `crates/rbmp-core/src/bgp/attributes.rs`

```rust
// In parse_path_attributes, case 40:
40 => {
    // RFC 8669 Prefix-SID: TLV-encoded
    // TLV type 1 = Label Index, type 4 = SRv6 L3 Service
    attrs.prefix_sid = Some(parse_prefix_sid(attr_buf)?);
}
```

Add to `bgp/types.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefixSid {
    pub label_index: Option<u32>,       // TLV type 1 (RFC 8669 §3.1)
    pub originator_srgb: Option<Vec<(u16, u32)>>, // TLV type 3: flags + SRGB base + range
    pub srv6_l3_service: Option<Srv6L3Service>,   // TLV type 5
    pub raw_tlvs: Vec<(u8, Vec<u8>)>,             // unrecognized TLVs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Srv6L3Service {
    pub sub_sub_tlvs: Vec<Srv6SubSubTlv>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Srv6SubSubTlv {
    pub sid:           [u8; 16],  // 128-bit SRv6 SID
    pub sid_flags:     u8,
    pub endpoint_behavior: u16,
}
```

**Parse function** in `bgp/srv6.rs`:
```rust
pub fn parse_prefix_sid(buf: &[u8]) -> Result<PrefixSid> { ... }
pub fn parse_srv6_l3_service(buf: &[u8]) -> Result<Srv6L3Service> { ... }
```

#### RV1-4 T2 — OTC attribute (RFC 9234, type 35)

RFC 9234 defines the "Only to Customer" (OTC) attribute and the BGP role capability. OTC prevents route leaks.

**File**: `crates/rbmp-core/src/bgp/types.rs`  
Add to `PathAttributes`:
```rust
pub only_to_customer: Option<u32>,  // RFC 9234 §4.1: the AS that set OTC
```

Add to `BgpCapability`:
```rust
BgpRole(u8),   // RFC 9234: 0=Provider, 1=RS, 2=RS-Client, 3=Customer, 4=Peer
```

**File**: `crates/rbmp-core/src/bgp/attributes.rs`  
```rust
35 if attr_len == 4 => {
    attrs.only_to_customer = Some(u32::from_be_bytes(attr_buf[..4].try_into().unwrap()));
}
```

**File**: `crates/rbmp-core/src/bgp/capabilities.rs`  
```rust
// Capability code 9: BGP Role (RFC 9234)
9 if !data.is_empty() => Ok(BgpCapability::BgpRole(data[0])),
```

#### RV1-4 T3 — Tunnel Encapsulation attribute (RFC 9012, type 23)

Type 23 carries tunnel endpoint info. For BMP observers the main interest is reading the tunnel type.

**File**: `crates/rbmp-core/src/bgp/types.rs`  
Add:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelEncapEntry {
    pub tunnel_type: u16,   // e.g. 7=VXLAN, 8=NVGRE, 10=MPLS-in-GRE, 23=SRv6
    pub tunnel_type_name: &'static str,
    pub endpoint: Option<std::net::IpAddr>,
    pub color: Option<u32>,
}
pub fn tunnel_type_name(t: u16) -> &'static str {
    match t {
        1  => "l2tpv3-over-ip",
        2  => "gre",
        3  => "transmit-tunnel-endpoint",
        4  => "ipsec-in-tunnel-mode",
        5  => "ip-in-ip-with-ipsec",
        6  => "mpls-in-ip-with-ipsec",
        7  => "ip-in-ip",
        8  => "vxlan",
        9  => "nvgre",
        10 => "mpls",
        11 => "mpls-in-gre",
        12 => "vxlan-gpe",
        13 => "mpls-in-udp",
        14 => "ipv6-tunnel",
        15 => "sr-mpls",
        16 => "geneve",
        17 => "endpoint",
        23 => "srv6",
        _  => "unknown",
    }
}
```

#### RV1-4 T4 — Extended Community Route Target decode improvements

Currently `ExtendedCommunity::fmt()` shows `rt:X:Y` for type 0x00/0x02. Extend to handle:
- VPN Route Origin (SOO): type `0x03`
- VXLAN VNI (type `0x81` sub-type `0x0C`)
- Color extended community (type `0x03` sub-type `0x0B`) for SR-TE policy

---

### Epic RV1-5: Add-Path Aware RIB ✅ COMPLETE (NLRI path_id parsing deferred to RV2)

**Scope**: `crates/rbmp-rib/`

RFC 7911 Add-Path sends multiple paths for the same prefix with a per-peer path-id. The current `RibTable` uses `prefix.to_string()` as key — this means only the last-received path per prefix is kept. Add-Path requires a compound key of `(prefix, path_id)`.

#### RV1-5 T1 — Detect Add-Path peers in session

**File**: `crates/rbmp-rib/src/session.rs`

Add to `PeerSession`:
```rust
/// AFI/SAFIs for which Add-Path is active (send/receive/both)
/// Populated from Add-Path capability in PeerUp OPEN.
pub add_path_families: Vec<(AfiSafi, u8)>,
```

In `on_up()`, extract Add-Path capability:
```rust
for cap in &caps {
    if let BgpCapability::AddPath(entries) = cap {
        self.add_path_families = entries.clone();
    }
}
```

Add helper:
```rust
pub fn add_path_active_for(&self, afi_safi: AfiSafi) -> bool {
    self.add_path_families.iter().any(|(a, _)| *a == afi_safi)
}
```

#### RV1-5 T2 — Add `path_id` to `RibEntry`

**File**: `crates/rbmp-rib/src/table.rs`

```rust
pub struct RibEntry {
    pub prefix:      Prefix,
    pub path_id:     Option<u32>,  // RFC 7911: present when Add-Path active for this AFI-SAFI
    pub attributes:  PathAttributes,
    pub received_at: DateTime<Utc>,
    pub peer_addr:   IpAddr,
    pub peer_as:     u32,
    // Is this the best-path selection result?
    pub is_best:     bool,
}
```

Update `RibTable` key to include path_id:
```rust
fn entry_key(prefix: &Prefix, path_id: Option<u32>) -> String {
    match path_id {
        Some(id) => format!("{}@{}", prefix, id),
        None     => prefix.to_string(),
    }
}
```

#### RV1-5 T3 — Best-path selection stub

When multiple paths exist for a prefix, mark `is_best` based on BGP decision process. For RV1, implement the first 3 tie-breakers only:
1. Highest LOCAL_PREF wins
2. Shortest AS_PATH wins (hop count)
3. Lowest MED wins

```rust
// crates/rbmp-rib/src/table.rs
pub fn recompute_best_path(&mut self, rib: RibType, prefix: &Prefix) {
    let key_prefix = prefix.to_string();
    let table = match self.tables.get_mut(&rib) { Some(t) => t, None => return };
    // Collect all paths for this prefix
    let matching_keys: Vec<String> = table.keys()
        .filter(|k| k.starts_with(&key_prefix))
        .cloned()
        .collect();
    if matching_keys.len() <= 1 { return; }
    // Find best path
    let best_key = matching_keys.iter().min_by_key(|k| {
        let e = &table[*k];
        let lp  = e.attributes.local_pref.map(|v| u32::MAX - v).unwrap_or(0);
        let hop = e.attributes.as_path.as_ref().map(|p| p.hop_count()).unwrap_or(0);
        let med = e.attributes.multi_exit_disc.unwrap_or(u32::MAX);
        (lp, hop, med)
    }).cloned();
    // Mark best
    for k in &matching_keys {
        if let Some(e) = table.get_mut(k) {
            e.is_best = best_key.as_deref() == Some(k);
        }
    }
}
```

---

### Epic RV1-6: Server Hardening ✅ COMPLETE

**Scope**: `crates/rbmp-server/`

#### RV1-6 T1 — Archive writer for raw PDUs

**New file**: `crates/rbmp-server/src/archive.rs`

```rust
// Append-only JSONL archive of every BmpMessage received.
// This is the replay/debug/audit trail.
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use anyhow::Result;
use rbmp_core::bmp::types::BmpMessage;

pub struct BmpArchive {
    file: Option<Arc<Mutex<tokio::fs::File>>>,
}

impl BmpArchive {
    pub async fn open(path: Option<&str>) -> Result<Self> { ... }
    pub async fn append(&self, msg: &BmpMessage) -> Result<()> { ... }
}
```

Wire into `receiver.rs` — after every successful parse, call `archive.append(&msg).await`.

#### RV1-6 T2 — Back-pressure governor

**New file**: `crates/rbmp-server/src/governor.rs`

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Lightweight shedding signal.
/// Set to true when Tokio channel backpressure exceeds threshold.
#[derive(Clone)]
pub struct ShedSignal(Arc<AtomicBool>);

impl ShedSignal {
    pub fn new() -> Self { Self(Arc::new(AtomicBool::new(false))) }
    pub fn set(&self, val: bool) { self.0.store(val, Ordering::Relaxed); }
    pub fn should_shed(&self) -> bool { self.0.load(Ordering::Relaxed) }
}

/// Spawn a background task that monitors the mpsc channel capacity.
/// When remaining capacity < 20%, sets shed=true; when > 60%, clears it.
pub fn spawn_pressure_monitor(
    msg_tx: tokio::sync::mpsc::Sender<rbmp_core::bmp::types::BmpMessage>,
    signal: ShedSignal,
) {
    tokio::spawn(async move {
        loop {
            let cap   = msg_tx.max_capacity();
            let avail = msg_tx.capacity();
            let used_pct = 100 - (avail * 100 / cap);
            if used_pct > 80 { signal.set(true); }
            else if used_pct < 40 { signal.set(false); }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    });
}
```

Wire into `receiver.rs` — before sending stats messages, check `shed_signal.should_shed()`.

#### RV1-6 T3 — Wire event broadcast properly in `main.rs`

The current `main.rs` has a hack where it creates a second broadcast channel for `event_tx` instead of using the RibManager's channel. Fix this:

```rust
// Correct wiring in main.rs:
let (rib_mgr, rib_rx_for_store) = RibManager::new(cfg.store.event_capacity);
let event_tx = rib_mgr.event_sender(); // expose a Sender clone from RibManager
let rib = Arc::new(RwLock::new(rib_mgr));
```

Add to `RibManager`:
```rust
pub fn event_sender(&self) -> broadcast::Sender<RibEvent> {
    self.event_tx.clone()
}
```

#### RV1-6 T4 — DuckDB checkpoint task

Wire the `checkpoint_secs` config:
```rust
// In main.rs, spawn a periodic checkpoint task:
if cfg.store.checkpoint_secs > 0 {
    let store3 = Arc::clone(&store);
    let secs   = cfg.store.checkpoint_secs;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(secs)
        );
        loop {
            interval.tick().await;
            if let Ok(s) = store3.lock() {
                if let Err(e) = s.checkpoint() {
                    tracing::warn!(error = %e, "DuckDB checkpoint failed");
                }
            }
        }
    });
}
```

---

### Epic RV1-7: rbmppy Python SDK ✅ COMPLETE

**Scope**: `bmppy/`  
**Rebuild as**: `bmppy/rbmppy/` package (importable as `import rbmppy`)

#### RV1-7 T1 — Project structure

```
bmppy/
├── rbmppy/
│   ├── __init__.py          # exports: RustyBmpClient, EventStream, BmpEvent, RouteEvent
│   ├── client.py            # async HTTP client
│   ├── stream.py            # SSE event streaming
│   ├── models.py            # Pydantic/dataclass models matching Rust JSON
│   ├── analytics.py         # BGP analytics helpers
│   └── peering.py           # PeeringDB + RPKI wrappers (stubs for RV1)
├── pyproject.toml
└── README.md
```

#### RV1-7 T2 — `models.py`: typed Python models

Mirror the Rust JSON output:
```python
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Optional, List, Dict, Any
from datetime import datetime

@dataclass
class Speaker:
    addr: str
    sys_name: Optional[str]
    sys_descr: Optional[str]
    connected_at: str
    peer_count: int
    peers_up: int
    total_routes: int

@dataclass
class Peer:
    speaker: str
    addr: str
    asn: int
    bgp_id: str
    state: str          # "Up" | "Down" | "Unknown"
    up_at: Optional[str]
    uptime_secs: Optional[int]
    hold_time: int
    flaps: int
    route_counts: Dict[str, int]
    capabilities: List[str] = field(default_factory=list)

@dataclass
class Route:
    speaker: str
    peer: str
    peer_as: int
    prefix: str
    next_hop: Optional[str]
    as_path: Optional[str]
    local_pref: Optional[int]
    med: Optional[int]
    communities: List[str]
    received_at: str

@dataclass
class RouteChange:
    occurred_at: str
    speaker_addr: str
    peer_addr: str
    peer_as: int
    rib_type: str
    action: str        # "announce" | "withdraw"
    prefix: str
    afi: str
    origin: Optional[str]
    as_path: Optional[str]
    as_path_len: Optional[int]
    next_hop: Optional[str]
    local_pref: Optional[int]
    med: Optional[int]
    communities: Optional[str]

@dataclass
class RibEvent:
    """A single event from the /api/events SSE stream."""
    kind: str          # "route_change" | "peer_up" | "peer_down" | "speaker_up" | etc.
    occurred_at: str
    speaker: str
    data: Dict[str, Any]
```

#### RV1-7 T3 — `client.py`: async HTTP client

```python
import httpx
import asyncio
from typing import Optional, List, AsyncIterator
from .models import Speaker, Peer, Route, RouteChange

class RustyBmpClient:
    """
    Async HTTP client for the rustybmp API.
    
    Usage:
        async with RustyBmpClient("http://localhost:7878") as c:
            speakers = await c.speakers()
            routes   = await c.routes(limit=100)
    """
    
    def __init__(self, base_url: str = "http://localhost:7878", timeout: float = 30.0):
        self.base_url = base_url.rstrip("/")
        self._client: Optional[httpx.AsyncClient] = None
        self._timeout = timeout

    async def __aenter__(self):
        self._client = httpx.AsyncClient(base_url=self.base_url, timeout=self._timeout)
        return self

    async def __aexit__(self, *_):
        if self._client:
            await self._client.aclose()

    async def health(self) -> dict:
        r = await self._client.get("/health")
        r.raise_for_status()
        return r.json()

    async def speakers(self) -> List[Speaker]:
        r = await self._client.get("/api/speakers")
        r.raise_for_status()
        return [Speaker(**s) for s in r.json()["speakers"]]

    async def peers(self) -> List[Peer]:
        r = await self._client.get("/api/peers")
        r.raise_for_status()
        return [Peer(**p) for p in r.json()["peers"]]

    async def routes(self, limit: int = 500, rib_type: Optional[str] = None) -> List[Route]:
        params = {"limit": limit}
        if rib_type:
            params["rib_type"] = rib_type
        r = await self._client.get("/api/routes", params=params)
        r.raise_for_status()
        return [Route(**row) for row in r.json()["routes"]]

    async def prefix_history(self, prefix: str, limit: int = 200) -> List[RouteChange]:
        r = await self._client.get("/api/routes/prefix", params={"prefix": prefix, "limit": limit})
        r.raise_for_status()
        return [RouteChange(**row) for row in r.json()["history"]]

    async def top_churn(self, limit: int = 20) -> List[tuple]:
        r = await self._client.get("/api/analytics/churn", params={"limit": limit})
        r.raise_for_status()
        return r.json().get("churn", [])
```

#### RV1-7 T4 — `stream.py`: SSE event streaming

```python
import asyncio
import json
import httpx
from typing import AsyncIterator
from .models import RibEvent

async def event_stream(base_url: str) -> AsyncIterator[RibEvent]:
    """
    Async generator yielding RibEvents from the rustybmp SSE stream.
    
    Usage:
        async for event in event_stream("http://localhost:7878"):
            print(event.kind, event.data)
    """
    async with httpx.AsyncClient(timeout=None) as client:
        async with client.stream("GET", f"{base_url}/api/events") as response:
            response.raise_for_status()
            async for line in response.aiter_lines():
                if line.startswith("data: "):
                    raw = line[6:]
                    try:
                        obj = json.loads(raw)
                        yield RibEvent(
                            kind=obj.get("kind", "unknown"),
                            occurred_at=obj.get("occurred_at", ""),
                            speaker=obj.get("speaker", ""),
                            data=obj,
                        )
                    except json.JSONDecodeError:
                        pass
```

#### RV1-7 T5 — `analytics.py`: BGP analytics helpers

```python
from __future__ import annotations
from typing import List, Optional, Tuple, Dict
from collections import defaultdict, deque
from datetime import datetime, timedelta, timezone
from .models import RouteChange, Peer, Route

class PrefixMonitor:
    """
    Track prefix stability over a sliding window.
    
    Usage:
        monitor = PrefixMonitor(window_seconds=300)
        monitor.record(route_change)
        if monitor.churn_rate("192.0.2.0/24") > 5:
            print("prefix is unstable")
    """
    
    def __init__(self, window_seconds: int = 300):
        self.window = window_seconds
        # prefix -> deque of (timestamp, action) tuples
        self._events: Dict[str, deque] = defaultdict(lambda: deque())

    def record(self, change: RouteChange) -> None:
        now = datetime.now(timezone.utc)
        cutoff = now - timedelta(seconds=self.window)
        q = self._events[change.prefix]
        # Trim old events
        while q and q[0][0] < cutoff:
            q.popleft()
        q.append((now, change.action))

    def churn_rate(self, prefix: str) -> int:
        """Number of announce+withdraw events in the window."""
        return len(self._events.get(prefix, []))

    def top_churning(self, n: int = 10) -> List[Tuple[str, int]]:
        """Return the n most active prefixes by event count."""
        counts = [(p, len(q)) for p, q in self._events.items()]
        return sorted(counts, key=lambda x: -x[1])[:n]


class SessionFlap:
    """
    Detect BGP session instability.
    Fires when a peer goes down more than `threshold` times in `window_seconds`.
    """
    
    def __init__(self, threshold: int = 3, window_seconds: int = 300):
        self.threshold = threshold
        self.window    = window_seconds
        self._flaps: Dict[str, deque] = defaultdict(lambda: deque())

    def record_down(self, peer_addr: str) -> bool:
        """
        Record a peer-down event. Returns True if the flap threshold is exceeded.
        """
        now     = datetime.now(timezone.utc)
        cutoff  = now - timedelta(seconds=self.window)
        q       = self._flaps[peer_addr]
        while q and q[0] < cutoff:
            q.popleft()
        q.append(now)
        return len(q) >= self.threshold

    def flap_count(self, peer_addr: str) -> int:
        return len(self._flaps.get(peer_addr, []))


def extract_as_path_features(as_path: Optional[str]) -> dict:
    """
    Extract ML-ready features from a raw AS_PATH string.
    Returns a dict of: hop_count, origin_asn, has_prepend, has_private_asn,
                       first_asn, unique_asns, path_diversity_ratio
    """
    if not as_path:
        return {"hop_count": 0, "origin_asn": None, "has_prepend": False,
                "has_private_asn": False, "first_asn": None, "unique_asns": 0,
                "path_diversity_ratio": 1.0}
    asns = [int(a) for a in as_path.split() if a.isdigit()]
    if not asns:
        return extract_as_path_features(None)
    # Private ASN ranges: 64512-65534, 4200000000-4294967294
    def is_private(asn: int) -> bool:
        return 64512 <= asn <= 65534 or 4200000000 <= asn <= 4294967294
    unique = set(asns)
    prev = None
    has_prepend = False
    for a in asns:
        if a == prev:
            has_prepend = True
            break
        prev = a
    return {
        "hop_count":            len(asns),
        "origin_asn":           asns[-1] if asns else None,
        "has_prepend":          has_prepend,
        "has_private_asn":      any(is_private(a) for a in asns),
        "first_asn":            asns[0],
        "unique_asns":          len(unique),
        "path_diversity_ratio": len(unique) / len(asns) if asns else 1.0,
    }


def community_summary(communities: Optional[str]) -> dict:
    """
    Parse a comma-separated community string into a structured summary.
    Returns: {standard: [...], well_known: [...], count: int}
    """
    if not communities:
        return {"standard": [], "well_known": [], "count": 0}
    WELL_KNOWN = {
        "4294967041": "NO_EXPORT",
        "4294967042": "NO_ADVERTISE",
        "4294967043": "NO_EXPORT_SUBCONFED",
        "4294902426": "BLACKHOLE",
    }
    parts = [c.strip() for c in communities.split(",") if c.strip()]
    standard, well_known = [], []
    for c in parts:
        # Community can be "ASN:VALUE" or a raw u32 string
        if c in WELL_KNOWN:
            well_known.append(WELL_KNOWN[c])
        else:
            standard.append(c)
    return {"standard": standard, "well_known": well_known, "count": len(parts)}
```

#### RV1-7 T6 — `pyproject.toml`

```toml
[project]
name = "rbmppy"
version = "0.1.0"
description = "Python SDK for rustybmp BGP monitoring"
requires-python = ">=3.11"
dependencies = [
    "httpx>=0.27",
    "anyio>=4",
]

[project.optional-dependencies]
ml = ["pandas>=2", "scikit-learn>=1.4", "polars>=0.20"]
dev = ["pytest>=8", "pytest-asyncio>=0.23", "respx>=0.21"]

[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.backends.legacy:build"

[tool.pytest.ini_options]
asyncio_mode = "auto"
```

---

### Epic RV1-8: ContainerLab + XRD Test Infrastructure ✅ COMPLETE

**New directory**: `lab/`

#### RV1-8 T1 — XRD BMP config template

**File**: `lab/configs/xrd/bmp-speaker.cfg`

```
!! XRD BMP speaker config for rustybmp testing
!! Substitute <UBUNTU_IP>, <LOCAL_LOOPBACK>, <BGP_ASN>, <PEER_IP>, <PEER_ASN>

hostname xrd-bmp-1

!!─── Loopback ─────────────────────────────────────────────────────────────────
interface Loopback0
 ipv4 address <LOCAL_LOOPBACK> 255.255.255.255
 ipv6 address fc00:0:1::1/128
!

!!─── BMP Configuration ────────────────────────────────────────────────────────
!! BMP server pointing to rustybmp on Ubuntu
bmp server 1
 host <UBUNTU_IP> port 5000
 description rustybmp-collector
 update-source Loopback0
 initial-delay 10
 stats-reporting-period 30
 initial-refresh delay 15 spread 2
!
!! Enable BMP for all BGP sessions
router bgp <BGP_ASN>
 bmp-activate server 1
 bmp server 1
  initial-refresh delay 15 spread 2
 !
!

!!─── Full BMP with per-peer granularity ───────────────────────────────────────
!! Uncomment to enable Adj-RIB-Out monitoring (RFC 8671)
! router bgp <BGP_ASN>
!  neighbor <PEER_IP>
!   bmp-activate server 1
!   address-family ipv4 unicast
!    bmp-activate server 1
!   !
!  !
! !
```

#### RV1-8 T2 — ContainerLab topology: simple BMP lab

**File**: `lab/xrd-bmp.clab.yml`

```yaml
name: rustybmp-test

topology:
  nodes:
    # XRD router as BMP speaker
    xrd-pe1:
      kind: cisco_xrd
      image: ios-xr/xrd-control-plane:24.x
      startup-config: lab/configs/xrd/pe1.cfg
      cpu: 1
      memory: 2048

    # Second XRD for iBGP peer
    xrd-pe2:
      kind: cisco_xrd
      image: ios-xr/xrd-control-plane:24.x
      startup-config: lab/configs/xrd/pe2.cfg
      cpu: 1
      memory: 2048

    # FRRouting for eBGP CE simulation
    frr-ce:
      kind: linux
      image: frrouting/frr:9.1
      binds:
        - lab/configs/frr/frr.conf:/etc/frr/frr.conf
        - lab/configs/frr/daemons:/etc/frr/daemons

  links:
    # PE1 <-> PE2 (iBGP over loopback)
    - endpoints: ["xrd-pe1:Gi0/0/0/0", "xrd-pe2:Gi0/0/0/0"]
    # PE1 <-> FRR (eBGP)
    - endpoints: ["xrd-pe1:Gi0/0/0/1", "frr-ce:eth1"]
```

#### RV1-8 T3 — XRD PE1 full test config

**File**: `lab/configs/xrd/pe1.cfg`

```
!! PE1 — XRD BMP test speaker
!! Loopback0: 10.0.0.1/32
!! Gi0/0/0/0: 10.0.12.0/31 (to PE2)
!! Gi0/0/0/1: 10.0.13.0/31 (to FRR-CE)
!! iBGP AS 65000, PE1-PE2 peers
!! eBGP to FRR-CE AS 65001

hostname xrd-pe1

interface Loopback0
 ipv4 address 10.0.0.1 255.255.255.255
!
interface GigabitEthernet0/0/0/0
 description to-pe2
 ipv4 address 10.0.12.0 255.255.255.254
 no shutdown
!
interface GigabitEthernet0/0/0/1
 description to-frr-ce
 ipv4 address 10.0.13.0 255.255.255.254
 no shutdown
!

!!─── BMP ─────────────────────────────────────────────────────────────────────
bmp server 1
 host 172.20.0.100 port 5000
 description rustybmp
 update-source Loopback0
 initial-delay 10
 stats-reporting-period 30
 initial-refresh delay 15 spread 2
!

!!─── BGP ─────────────────────────────────────────────────────────────────────
router bgp 65000
 bgp router-id 10.0.0.1
 bmp-activate server 1
 !
 address-family ipv4 unicast
  network 10.0.0.1/32
 !
 !
 !! iBGP to PE2
 neighbor 10.0.0.2
  remote-as 65000
  update-source Loopback0
  address-family ipv4 unicast
  !
 !
 !! eBGP to FRR-CE
 neighbor 10.0.13.1
  remote-as 65001
  address-family ipv4 unicast
   route-policy PASS in
   route-policy PASS out
  !
 !
!

route-policy PASS
  pass
end-policy
!

lldp
!
```

#### RV1-8 T4 — FRR config for route injection

**File**: `lab/configs/frr/frr.conf`

```
! FRR CE router — injects test prefixes into PE1

frr version 9.1
frr defaults traditional
hostname frr-ce
log syslog informational
no ipv6 forwarding

interface eth1
 ip address 10.0.13.1/31
!

router bgp 65001
 bgp router-id 10.0.13.1
 !
 address-family ipv4 unicast
  network 203.0.113.0/24
  network 198.51.100.0/24
  network 192.0.2.0/24
 !
 neighbor 10.0.13.0
  remote-as 65000
  address-family ipv4 unicast
  !
 !
!
line vty
!
```

#### RV1-8 T5 — Test scenario scripts

**File**: `lab/scenarios/flap_peer.sh`

```bash
#!/usr/bin/env bash
# Simulate BGP peer flap on XRD PE1 toward FRR-CE
# Requires containerlab to be running: clab deploy -t lab/xrd-bmp.clab.yml

ROUTER=${1:-clab-rustybmp-test-xrd-pe1}
PEER_IP="10.0.13.1"

echo "== Flapping BGP peer $PEER_IP on $ROUTER =="
for i in 1 2 3; do
    echo "Flap $i: bringing neighbor down..."
    docker exec "$ROUTER" xrctl run "router bgp 65000; neighbor $PEER_IP shutdown"
    sleep 5
    echo "Flap $i: restoring neighbor..."
    docker exec "$ROUTER" xrctl run "router bgp 65000; no neighbor $PEER_IP shutdown"
    sleep 10
done
echo "Flap test complete. Check rustybmp events: curl http://localhost:7878/api/peers"
```

**File**: `lab/scenarios/mass_withdrawal.sh`

```bash
#!/usr/bin/env bash
# Inject 100 routes then withdraw them all — tests bulk withdrawal handling
FRROUTER=${1:-clab-rustybmp-test-frr-ce}

echo "== Injecting 100 test prefixes =="
for i in $(seq 1 100); do
    SUBNET="10.$((i/256)).$((i%256)).0/24"
    docker exec "$FRROUTER" vtysh -c "conf t" -c "router bgp 65001" \
        -c "address-family ipv4 unicast" -c "network $SUBNET"
done

sleep 15
echo "== Withdrawing all 100 prefixes =="
for i in $(seq 1 100); do
    SUBNET="10.$((i/256)).$((i%256)).0/24"
    docker exec "$FRROUTER" vtysh -c "conf t" -c "router bgp 65001" \
        -c "address-family ipv4 unicast" -c "no network $SUBNET"
done
echo "Withdrawal test complete."
```

---

## Part 8 — Future Sprints Outline (RV2 and beyond)

These are outlined here so the architecture can accommodate them. Detailed tasks will be in separate backlog files (RUSTYBMP_BACKLOG_RV2.md, etc.) produced as diffs are uploaded.

### Sprint RV2 — Internet Intelligence Integration

**New crate**: `crates/rbmp-enrichment/`

1. **RPKI/ROA validation** — RTR protocol (RFC 8210) to receive VRP (Validated ROA Payload) cache from rpki-client/Routinator. Validate every route announce against ROA state (valid/invalid/not-found). Store in DuckDB `rpki_status` column.

2. **PeeringDB integration** — REST API client (`https://peeringdb.com/api/`). Enrich ASN with: org name, network type (ISP/Content/Enterprise), peering policy (open/selective/no), IX presences, NOC contact. Cache per-ASN for 24h.

3. **RIPE STAT API** — `https://stat.ripe.net/data/` for:
   - `announced-prefixes`: all prefixes originated by an ASN
   - `prefix-overview`: owner, abuse contact, visibility
   - `bgp-state`: current BGP table state from RIS
   - `routing-history`: historical routing changes

4. **IRR / WHOIS** — Query RADB/RIPE/ARIN/APNIC for route objects and aut-num objects. Verify that announced prefixes have corresponding IRR route objects.

5. **BGP hijack detection** — Rules:
   - Origin ASN changed for a prefix (sudden origin change)
   - AS_PATH dramatically shorter than historical norm
   - New origin for a prefix with no matching IRR route object
   - Prefix more-specific than registered route object

6. **Route leak detection** (RFC 9234 + OTC attribute):
   - OTC attribute present but should not be
   - Valley-free violation: customer routes should not appear in provider's RIB

### Sprint RV3 — Advanced Analytics

**New crate**: `crates/rbmp-analytics/`

1. **Convergence timing** — Measure time between first withdraw and stable new state for each prefix. Store as `convergence_ms` in DuckDB.

2. **ECMP imbalance detection** — When Add-Path is active and multiple paths exist, compare path attributes. Flag when one path has significantly better attributes than others (suggesting misconfiguration).

3. **Policy change impact** — Compare pre-policy vs post-policy RIB for a peer. Compute: rejected count, accepted count, attribute modifications (communities added/removed by policy, LOCAL_PREF changes).

4. **Route oscillation** — Prefix that announces/withdraws more than N times per hour. Store oscillation events with full timeline.

5. **Churn baseline** — Compute rolling 24h average churn per prefix. Alert when current churn > 3σ above baseline.

6. **ML anomaly detection** (Python, rbmppy) — IsolationForest on per-peer feature vectors:
   - Features: route_count, churn_rate, avg_as_path_len, prepend_ratio, communities_count, peer_flap_count, rpki_invalid_ratio
   - Train on 7 days of normal data, flag outliers

### Sprint RV4 — Operational UI

1. **Real-time dashboard** — Svelte app served by Axum. Panels: active speakers/peers summary, route count timeline, top churning prefixes, RPKI status breakdown.

2. **Prefix explorer** — Search by prefix, see full history, RPKI status, RIPE STAT data, current AS_PATH, community tags.

3. **AS path visualizer** — D3.js graph of AS hops for a selected prefix across all peers. Highlights prepending, private ASNs, route divergence between peers.

4. **Peer comparison** — Side-by-side RIB diff between two peers for the same AFI-SAFI. Shows routes accepted by one peer but not another.

### Sprint RV5 — High Availability + Scale

1. **Multi-collector** — Multiple rustybmp instances collecting from the same set of routers. Consensus on RIB state, de-duplicate events.

2. **Write-ahead log** — Before writing to DuckDB, journal events to an append-only WAL file. On crash, replay WAL to recover last N seconds.

3. **Distributed RPKI cache** — Share RPKI VRP table across multiple collectors via shared Redis/Valkey cache.

---

## Part 9 — Quality Gates

Every PR for RV1 epics must pass:

```bash
# Run before every PR:
cargo fmt --all              # formatting
cargo clippy --workspace -- -D warnings   # no warnings
cargo test --workspace       # all tests green
cargo build --workspace --release        # release build succeeds
```

### Required test coverage per epic

- RV1-1 (Stats): Unit test for stat types 18, 19 (11-byte), 35 (RPKI), 37
- RV1-2 (EVPN): Unit test for route types 1, 2 (MAC/IP with IPv4), 3, 4, 5
- RV1-3 (Flowspec): Unit test for type-1 prefix, type-3 protocol, type-4 port, type-9 TCP flags
- RV1-4 (Path Attrs): Unit test for Prefix-SID label index, OTC attribute
- RV1-5 (Add-Path): Unit test that two paths for same prefix are stored separately; best-path selection chooses correct one
- RV1-7 (rbmppy): Unit test for `extract_as_path_features` and `community_summary`; integration test against mock HTTP server

---

## Part 10 — Key Config Reference

### `rustybmp.toml` (current defaults)

```toml
[bmp]
listen_addr     = "0.0.0.0:5000"
max_frame_bytes = 65535
shed_stats_on_pressure = true
# archive_path = "runtime/bmp-archive.jsonl"  # uncomment to enable

[http]
listen_addr = "0.0.0.0:7878"
serve_ui    = true
cors_origins = []

[store]
db_path          = "runtime/routes.duckdb"
in_memory        = false
event_capacity   = 16384
checkpoint_secs  = 60

[log]
level  = "info"    # trace|debug|info|warn|error
format = "pretty"  # pretty|json
```

### XRD BMP statistics-reporting-period tuning

The `stats-reporting-period N` (in seconds) on XRD controls how often StatsReport messages are sent. With RFC 9972 now providing 20+ new gauge types, each StatsReport may be significantly larger than before. Recommended values:
- Lab: `30` seconds  
- Production: `300` seconds (5 min)  
- With `shed_stats_on_pressure = true` in rustybmp, stats are automatically dropped when the receiver is under load.

---

## Part 11 — File Change Index (what this sprint creates/modifies)

### Modified files

| File | Change | Epic |
|------|--------|-------|
| `crates/rbmp-core/src/bmp/types.rs` | Add RFC 9972 stat names (types 18-38), `StatEntry` afi_safi field | RV1-1 |
| `crates/rbmp-core/src/bmp/parser.rs` | Handle 11-byte per-AFI/SAFI stats | RV1-1 |
| `crates/rbmp-core/src/bgp/types.rs` | Add `EvpnReachNlri`, `EvpnUnreachNlri`, `FlowspecNlri`, `PrefixSid`, `TunnelEncapEntry`, `only_to_customer` to `PathAttributes` | RV1-2/3/4 |
| `crates/rbmp-core/src/bgp/attributes.rs` | Wire EVPN + Flowspec dispatch, add attr type 35 (OTC), 40 (Prefix-SID), 23 (Tunnel Encap) | RV1-2/3/4 |
| `crates/rbmp-core/src/bgp/capabilities.rs` | Add BgpRole capability (code 9) | RV1-4 |
| `crates/rbmp-rib/src/session.rs` | Add `add_path_families` | RV1-5 |
| `crates/rbmp-rib/src/table.rs` | Add `path_id` to `RibEntry`, compound key, `recompute_best_path` | RV1-5 |
| `crates/rbmp-rib/src/manager.rs` | Expose `event_sender()`, use path_id from update | RV1-5/6 |
| `crates/rbmp-store/src/schema.rs` | Add `evpn_events` table, add `afi`/`safi`/`stat_type` to `stats_events` | RV1-1/2 |
| `crates/rbmp-store/src/writer.rs` | Write EVPN events, write stat afi/safi fields | RV1-1/2 |
| `crates/rbmp-server/src/main.rs` | Fix event_tx wiring, add checkpoint task | RV1-6 |

### New files

| File | Purpose | Epic |
|------|---------|-------|
| `crates/rbmp-core/src/bgp/evpn.rs` | RFC 7432 EVPN NLRI decoder | RV1-2 |
| `crates/rbmp-core/src/bgp/flowspec.rs` | RFC 5575/8955 Flowspec NLRI decoder | RV1-3 |
| `crates/rbmp-core/src/bgp/srv6.rs` | SRv6 Prefix-SID parser | RV1-4 |
| `crates/rbmp-server/src/archive.rs` | Async JSONL archive writer | RV1-6 |
| `crates/rbmp-server/src/governor.rs` | Back-pressure shedding signal | RV1-6 |
| `bmppy/rbmppy/__init__.py` | Package exports | RV1-7 |
| `bmppy/rbmppy/client.py` | Async HTTP client | RV1-7 |
| `bmppy/rbmppy/stream.py` | SSE event streaming | RV1-7 |
| `bmppy/rbmppy/models.py` | Pydantic/dataclass models | RV1-7 |
| `bmppy/rbmppy/analytics.py` | BGP analytics helpers | RV1-7 |
| `bmppy/pyproject.toml` | Python packaging | RV1-7 |
| `lab/xrd-bmp.clab.yml` | ContainerLab topology | RV1-8 |
| `lab/configs/xrd/pe1.cfg` | XRD PE1 full config | RV1-8 |
| `lab/configs/xrd/pe2.cfg` | XRD PE2 full config | RV1-8 |
| `lab/configs/frr/frr.conf` | FRR CE config | RV1-8 |
| `lab/configs/frr/daemons` | FRR daemon config | RV1-8 |
| `lab/scenarios/flap_peer.sh` | Peer flap test | RV1-8 |
| `lab/scenarios/mass_withdrawal.sh` | Mass withdrawal test | RV1-8 |
| `scripts/dev/make_diff.sh` | Diff generation for next session | Workflow |

---

## Part 12 — Notes for the Next Claude Session

The next conversation should start with an upload of the diff patch file (output of `scripts/dev/make_diff.sh`). That diff will show which RV1 epics are complete and which have compilation or test failures. The next backlog (RUSTYBMP_BACKLOG_RV2.md) will:

1. Mark completed RV1 epics as ✅
2. Document any code changes required by test failures found on Ubuntu/XRD
3. Add RV2 epics in full detail (RPKI, PeeringDB, advanced analytics)

**Context to preserve**: This document is the complete project context. In the next session, upload this file + the diff only. The full repo zip is not needed again.

---

*End of RUSTYBMP_BACKLOG_RV1.md — Sprint RV1*
