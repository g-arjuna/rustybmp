# RustyBMP — Sprint RV2 Backlog
## Analytics Engine · Internet Intelligence · Collector Architecture · Scale

> **Version**: RV2  
> **Date**: 2026-06-19  
> **Basis**: Full diff analysis of `rv1_all_changes.patch` + research paper (IMACSI 2025, Hiremath — "Real-Time BGP Monitoring with BMP and Streaming Telemetry") + bonsai architecture gap analysis + RV1 `results_and_decisions.md`  
> **Principle**: Every task names an exact file, function, and expected behaviour. Nothing is vague.

---

## Part 1 — RV1 Completion Audit

### ✅ Fully Complete

| Epic | Status | Notes |
|------|--------|-------|
| RV1-1 RFC 9972 Stats | ✅ | Types 18-38 named; 11-byte per-AFI/SAFI parser; `StatEntry.afi_safi` field; DuckDB schema updated with `stat_type`, `afi`, `safi` columns |
| RV1-2 EVPN NLRI | ✅ | All 5 route types; `evpn_events` table; `writer.rs` correctly writes EVPN rows on announce |
| RV1-3 Flowspec NLRI | ✅ | Types 1-12; numeric + bitmask ops; `flowspec_reach`/`flowspec_unreach` in `PathAttributes` |
| RV1-4 Advanced Path Attrs | ✅ | OTC type 35, Prefix-SID type 40 (`bgp/srv6.rs`), Tunnel Encap type 23, BgpRole cap 9; `TunnelEncapEntry.tunnel_type_name: String` (lifetime fix applied) |
| RV1-5 Add-Path RIB | ✅ (structure) | `path_id: Option<u32>` + `is_best: bool` in `RibEntry`; compound key `prefix@path_id`; `recompute_best_path()` stub; **path_id always None — NLRI parsing deferred** |
| RV1-6 Server Hardening | ✅ | `archive.rs`, `governor.rs`, fixed `event_tx` wiring (D7 critical bug — SSE was dead), checkpoint task, receiver updated |
| RV1-7 rbmppy SDK | ✅ (partial) | `client.py`, `stream.py`, `models.py` (pydantic), `peering.py` (live PeeringDB + Cloudflare RPKI); `analytics.py` only bug-fixed not rebuilt |
| RV1-8 ContainerLab | ✅ | `xrd-bmp.clab.yml`, XRD pe1/pe2 configs, FRR CE, flap/withdrawal scenarios |

### ⚠️ Partially Complete — Carry Forward to RV2

| Gap | Details | RV2 Epic |
|-----|---------|----------|
| `analytics.py` not rebuilt | Only `current_rib()` SQL bug fixed; `PrefixMonitor`/`SessionFlap`/Z-score entirely absent | RV2-5 |
| Add-Path NLRI path_id | Struct ready, but path_id is always `None`; NLRI decoder unchanged | RV2-1 |
| BGP-LS | Stub AFI `BgpLs` exists, no NLRI decoder | RV2-2 |
| LLGR state machine | Capability parsed; stats type 28 named; no stale-route tracking | RV2-9 |
| RPKI caching | `peering.py` makes live HTTP per call; no cache, no bulk, no RTR | RV2-3 |
| Receiver supervision | No reconnect/restart on speaker TCP drop | RV2-6 |
| DuckDB write batching | Every event = immediate lock + insert | RV2-7 |
| Route Target full decode | Extended communities show `rt:X:Y` for 0x00/0x02 only; opaque for others | RV2-1 |
| `evpn_unreach` in writer | `writer.rs` writes on announce; no handling for EVPN withdraw | RV2-1 |

---

## Part 2 — Research Integration: Hiremath 2026 Paper

The uploaded paper (IMACSI 2025) validates the entire rustybmp approach and directly informs the RV2 analytics engine. Key insights:

**Validated architecture**: BMP Routers → BMP Collector → Telemetry Converter → Kafka message broker → InfluxDB → Grafana + Anomaly Detector. RustyBMP replaces Kafka+InfluxDB with DuckDB (same semantics, simpler deployment). Grafana can still be used against the Prometheus endpoint.

**Validated performance targets** (from Table 1):
- Average event-to-dashboard latency: **680ms** (target for our SSE stream)
- Peak event handling: **1500 messages/sec** (throughput benchmark for our receiver)
- Anomaly detection accuracy: **97.2%** (target for our Z-score model)
- False positive rate: **2.1%** (target)
- Alert dispatch: **<300ms** from detection to rbmppy event emission
- Prefix hijack detection: **100% (43/43 cases)**
- Route leak detection: **95%**

**Statistical model (equations from §3.3, directly implementable)**:

```
For each prefix Pi, over sliding window of N time-slots Tj:

mean:   μPi = (1/N) × Σ f(Pi, Tj)          # eq. 2
stddev: σPi = sqrt((1/N) × Σ (f(Pi,Tj) - μPi)²)   # eq. 3
z-score: Zi = (f(Pi, Tnew) - μPi) / σPi    # eq. 4

Anomaly when: |Zi| > θ   (θ = 3.0 default)
Hijack when: origin_AS changed without prior withdraw
```

**BGP event tuple** (equation 1 from paper):
```
ei = (Pi, ASi, NHi, Ti, Typei)
where Pi=prefix, ASi=AS_PATH, NHi=next-hop, Ti=timestamp, Typei=announce|withdraw
```

This exact tuple is already in our `RouteChange` struct. The Python analytics engine can directly consume it from the SSE stream or DuckDB.

**Two alert types** (from Algorithm 2):
1. `Zi > θ`: "Abnormal announcement frequency for prefix P"
2. `origin_AS changed unexpectedly`: "Origin AS change — possible hijack"

---

## Part 3 — Bonsai Architecture Gaps Analysis

These are patterns from bonsai that rustybmp has NOT yet incorporated. Categorised by urgency.

### Critical gaps (affect correctness/reliability at scale)

**3.1 Receiver Supervisor (`src/receiver_supervisor.rs`)**

Bonsai wraps every collector receiver in a supervisor that:
- Monitors the TCP connection health
- On drop: exponential backoff reconnect (1s, 2s, 4s, 8s, max 60s)
- Emits `SpeakerDown` event when speaker drops (so RIB can clear routes)
- Tracks reconnect attempts in Prometheus

Currently rustybmp: when a BMP speaker (XRD router) drops TCP, `handle_connection` exits and the routes remain in memory. There is no reconnect attempt.

**3.2 Write Coordinator (`src/write_coordinator.rs`)**

Bonsai batches writes with a coordinator:
- Buffers events for up to N ms (default 50ms) or until batch size X (default 100 events)
- Flushes in a single transaction
- Critical for scale: 1500 msg/sec × individual DuckDB inserts = lock contention on Mutex<RouteStore>

Currently rustybmp: every `RibEvent` goes directly to a `store.lock().unwrap()` + single INSERT. At 1500 msg/sec this serializes all writes behind a single Mutex.

**3.3 Event Bus (`src/event_bus.rs`)**

Bonsai's in-process event bus uses path-based routing (publish to `"streaming/bmp/route-monitoring"` etc.). Multiple independent subscribers. No one slow subscriber can block another.

Currently rustybmp: Tokio `broadcast::channel` — a lagged subscriber causes events to be dropped. The SSE handler, store writer, and analytics subscriber all share one channel. A slow WebSocket client causes route events to drop.

### Important gaps (affect operational completeness)

**3.4 Registry (`src/registry.rs`)**

Maps IP addresses to rich metadata: hostname, vendor, role, site. In bonsai this enriches every event. In rustybmp, events only have `speaker_addr: IpAddr`. Operators need to know "that's xrd-pe1 at Site-A (Cisco IOS-XR)".

**3.5 Correlation Buffer (`src/correlation_buffer.rs`)**

Groups events within a time window. Key use case for BGP: detecting when multiple peers go down simultaneously (= upstream failure vs single peer misconfiguration).

**3.6 Distributed Collector/Core Architecture (`docs/collector_core_protocol.md`)**

The user's original brief specifically mentioned "scalable core + collector architecture." Bonsai separates:
- **Collector**: runs at network edge, receives BMP, forwards parsed messages
- **Core**: receives from collectors, owns RIB + store + API

Currently rustybmp is a monolith. For multi-site BGP collection (5+ XRD routers at different PoPs), we need this.

### Future gaps (architecture completeness)

**3.7 HA Coordinator** — Two instances, leader election, de-duplicated writes  
**3.8 Graph engine** — AS topology from AS_PATH data, EVPN topology from type-2/3 routes  
**3.9 TLS for BMP** — BMP is plaintext TCP by default; bonsai adds TLS

---

## Part 4 — RV2 Epics

### Epic RV2-1: Protocol Completeness Fixes

**Scope**: `crates/rbmp-core/` and `crates/rbmp-store/`

These are clean-up items from RV1's deferred list.

#### RV2-1 T1 — Add-Path NLRI path_id extraction

**File**: `crates/rbmp-core/src/bgp/nlri.rs`

When Add-Path is active (negotiated via capability code 69), the NLRI format changes: each entry is prefixed with a 4-byte path_id before the prefix_len byte. The current `decode_nlri()` assumes the old format. Add a new function:

```rust
/// Decode length-prefixed NLRI with 4-byte Add-Path path IDs (RFC 7911).
/// Format per entry: path_id(4) + prefix_len(1) + prefix_bytes
pub fn decode_nlri_add_path(buf: &mut impl Buf, afi: Afi) -> Result<Vec<(u32, Prefix)>> {
    let mut results = Vec::new();
    while buf.remaining() > 0 {
        if buf.remaining() < 5 {
            return Err(Error::UnexpectedEof { needed: 5, have: buf.remaining() });
        }
        let path_id    = buf.get_u32();
        let prefix_len = buf.get_u8();
        let octets     = (prefix_len as usize + 7) / 8;
        let max_bits   = match afi { Afi::Ipv6 => 128, _ => 32 };
        if prefix_len > max_bits {
            return Err(Error::InvalidPrefixLen { prefix_len, afi: afi.as_u16() });
        }
        if buf.remaining() < octets {
            return Err(Error::UnexpectedEof { needed: octets, have: buf.remaining() });
        }
        let prefix = decode_single_prefix(buf, afi, prefix_len, octets)?;
        results.push((path_id, prefix));
    }
    Ok(results)
}
```

**File**: `crates/rbmp-core/src/bgp/update.rs`

In `parse_bgp_update`, after checking for Add-Path capability (which requires session-level state), dispatch to `decode_nlri_add_path`. Since parsing is stateless, pass a bool `add_path_active: bool` into the parser. Update `parse_bgp_update` signature:

```rust
pub fn parse_bgp_update(buf: &[u8]) -> Result<BgpUpdate>
// becomes:
pub fn parse_bgp_update_with_caps(buf: &[u8], add_path_active: bool) -> Result<BgpUpdate>
```

The `BgpUpdate` already has a `path_ids` field? No — add it:

**File**: `crates/rbmp-core/src/bgp/types.rs`

```rust
pub struct BgpUpdate {
    pub withdrawn:  Vec<Prefix>,
    pub attributes: PathAttributes,
    pub announced:  Vec<Prefix>,
    /// Add-Path path IDs, parallel to `announced`. Empty when Add-Path not active.
    /// Index i in `announced` has path_id `path_ids[i]` (or None if empty).
    pub announced_path_ids: Vec<Option<u32>>,
}
```

**File**: `crates/rbmp-rib/src/manager.rs`

When processing `RouteMonitoring`, check if the peer has Add-Path active for the prefix's AFI-SAFI, then extract `path_id` from `update.announced_path_ids[i]`:

```rust
let path_id = update.announced_path_ids.get(idx).copied().flatten();
let entry = RibEntry { prefix, path_id, ..., is_best: path_id.is_none() };
// After insert, if path_id is Some, trigger recompute_best_path
if path_id.is_some() {
    rib.recompute_best_path(rib_type, &prefix_clone);
}
```

#### RV2-1 T2 — EVPN withdraw path in writer.rs

**File**: `crates/rbmp-store/src/writer.rs`

Currently only announce writes to `evpn_events`. Add withdraw:

```rust
// In RouteChange handling, after the route_events INSERT:
// Write EVPN withdraw rows
if action == "withdraw" {
    if let Some(evpn) = attrs.as_ref().and_then(|a| a.evpn_unreach.as_ref()) {
        for route in &evpn.routes {
            let (rd_s, etag, mac_s, ip_s, pfxlen, label, esi_s) = evpn_route_fields(route);
            conn.execute(
                "INSERT INTO evpn_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    id, ts, spk,
                    rc.peer_header.peer_address.to_string(),
                    rc.peer_header.peer_as,
                    "withdraw",
                    route.route_type_code(), route.route_type_name(),
                    rd_s, etag, mac_s, ip_s, pfxlen, label, esi_s,
                ],
            )?;
        }
    }
}
```

#### RV2-1 T3 — Extended Community full Route Target decode

**File**: `crates/rbmp-core/src/bgp/types.rs`

Update `ExtendedCommunity::fmt()` to handle all common sub-types:

```rust
impl fmt::Display for ExtendedCommunity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_high = self.type_high & 0x3F;
        match (type_high, self.type_low) {
            (0x00, 0x02) | (0x02, 0x02) => {
                // Route Target — Two-Octet AS Specific
                let admin    = u16::from_be_bytes([self.value[0], self.value[1]]);
                let assigned = u32::from_be_bytes([self.value[2], self.value[3], self.value[4], self.value[5]]);
                write!(f, "rt:{}:{}", admin, assigned)
            }
            (0x01, 0x02) => {
                // Route Target — IPv4 Address Specific
                let ip = std::net::Ipv4Addr::from([self.value[0], self.value[1], self.value[2], self.value[3]]);
                let assigned = u16::from_be_bytes([self.value[4], self.value[5]]);
                write!(f, "rt:{}:{}", ip, assigned)
            }
            (0x00, 0x03) | (0x02, 0x03) => {
                // Route Origin (SoO) — Two-Octet AS
                let admin    = u16::from_be_bytes([self.value[0], self.value[1]]);
                let assigned = u32::from_be_bytes([self.value[2], self.value[3], self.value[4], self.value[5]]);
                write!(f, "soo:{}:{}", admin, assigned)
            }
            (0x03, 0x0C) => {
                // Color extended community (SR-TE policy)
                let color = u32::from_be_bytes([self.value[2], self.value[3], self.value[4], self.value[5]]);
                write!(f, "color:{}", color)
            }
            (0x80, 0x0A) => {
                // Encapsulation sub-type (Tunnel Type for BGP Encapsulation)
                let tunnel_type = u16::from_be_bytes([self.value[4], self.value[5]]);
                write!(f, "encap:{}", crate::bgp::types::tunnel_type_name(tunnel_type))
            }
            _ => write!(f, "ext:0x{:02x}{:02x}:{}", self.type_high, self.type_low,
                self.value.iter().map(|b| format!("{:02x}", b)).collect::<String>())
        }
    }
}
```

**Tests**: Unit tests for RT, SoO, color, encap community display.

---

### Epic RV2-2: BGP-LS NLRI Decoder

**Scope**: new `crates/rbmp-core/src/bgp/bgpls.rs`

RFC 7752 defines BGP Link-State as AFI=16388 (BGP-LS), SAFI=71. It carries topology information (IS-IS/OSPF nodes, links, prefixes) as NLRI.

XRD already has BGP-LS configured (bonsai lab `xrd/PE1.cfg` — `bgp-ls source` section). This data is arriving via BMP right now and being silently dropped.

#### RV2-2 T1 — BGP-LS NLRI structs and decoder

**New file**: `crates/rbmp-core/src/bgp/bgpls.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::{Error, Result};

/// RFC 7752 §3.2 — BGP-LS NLRI types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BgpLsNlri {
    /// Type 1: Node NLRI
    Node(NodeNlri),
    /// Type 2: Link NLRI
    Link(LinkNlri),
    /// Type 3: IPv4 Topology Prefix NLRI
    Ipv4Prefix(PrefixNlri),
    /// Type 4: IPv6 Topology Prefix NLRI
    Ipv6Prefix(PrefixNlri),
    /// Type 6: SRv6 SID NLRI (RFC 9514)
    Srv6Sid(Srv6SidNlri),
    Unknown { nlri_type: u16, data: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeNlri {
    pub protocol_id:     u8,    // 1=IS-IS L1, 2=IS-IS L2, 3=OSPFv2, 5=OSPFv3
    pub identifier:      u64,
    pub local_node:      NodeDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkNlri {
    pub protocol_id:     u8,
    pub identifier:      u64,
    pub local_node:      NodeDescriptor,
    pub remote_node:     NodeDescriptor,
    pub link_descriptor: Vec<LinkDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixNlri {
    pub protocol_id:      u8,
    pub identifier:       u64,
    pub local_node:       NodeDescriptor,
    pub prefix_descriptor: Vec<PrefixDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Srv6SidNlri {
    pub protocol_id: u8,
    pub identifier:  u64,
    pub local_node:  NodeDescriptor,
    pub sid:         [u8; 16],
}

/// Node Descriptor sub-TLVs (RFC 7752 §3.2.1.4)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeDescriptor {
    IsIs { iso_node_id: [u8; 7] },          // sub-TLV 515 (0x0203)
    Ospf { router_id: std::net::Ipv4Addr },  // sub-TLV 516 (0x0204)
    Autonomous { asn: u32 },                 // sub-TLV 512
    BgpRouterId { router_id: std::net::Ipv4Addr },  // sub-TLV 513
    Unknown { code: u16, data: Vec<u8> },
}

/// Link Descriptor sub-TLVs (RFC 7752 §3.2.2)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkDescriptor {
    LinkLocalRemoteId { local: u32, remote: u32 },  // TLV 258
    IPv4InterfaceAddr(std::net::Ipv4Addr),           // TLV 259
    IPv4NeighborAddr(std::net::Ipv4Addr),            // TLV 260
    IPv6InterfaceAddr(std::net::Ipv6Addr),           // TLV 261
    Unknown { code: u16, data: Vec<u8> },
}

/// Prefix Descriptor sub-TLVs (RFC 7752 §3.2.3)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrefixDescriptor {
    OspfRouteType(u8),                         // TLV 264
    IpReachabilityInfo { prefix: String, prefix_len: u8 }, // TLV 265
    Unknown { code: u16, data: Vec<u8> },
}

/// Decode BGP-LS NLRIs from MP_REACH/MP_UNREACH attribute body.
/// Each NLRI: type(2) + length(2) + value
pub fn decode_bgpls_nlri(mut buf: &[u8]) -> Result<Vec<BgpLsNlri>> {
    let mut result = Vec::new();
    while buf.len() >= 4 {
        let nlri_type = u16::from_be_bytes([buf[0], buf[1]]);
        let length    = u16::from_be_bytes([buf[2], buf[3]]) as usize;
        buf = &buf[4..];
        if buf.len() < length { break; }
        let value = &buf[..length];
        buf = &buf[length..];
        let nlri = parse_bgpls_nlri(nlri_type, value)?;
        result.push(nlri);
    }
    Ok(result)
}

fn parse_bgpls_nlri(nlri_type: u16, buf: &[u8]) -> Result<BgpLsNlri> {
    if buf.len() < 9 {
        return Err(Error::UnexpectedEof { needed: 9, have: buf.len() });
    }
    let protocol_id  = buf[0];
    let identifier   = u64::from_be_bytes(buf[1..9].try_into().unwrap());
    let descriptors  = &buf[9..];

    match nlri_type {
        1 => {
            let local_node = parse_node_descriptors(descriptors)?;
            Ok(BgpLsNlri::Node(NodeNlri { protocol_id, identifier, local_node }))
        }
        2 => {
            // Link: local node descriptor + remote node descriptor + link descriptor
            let (local, rest)   = parse_node_descriptor_group(descriptors)?;
            let (remote, rest2) = parse_node_descriptor_group(rest)?;
            let link_desc       = parse_link_descriptors(rest2)?;
            Ok(BgpLsNlri::Link(LinkNlri {
                protocol_id, identifier,
                local_node: local, remote_node: remote, link_descriptor: link_desc
            }))
        }
        3 | 4 => {
            let (local, rest)     = parse_node_descriptor_group(descriptors)?;
            let prefix_descriptor = parse_prefix_descriptors(rest)?;
            Ok(BgpLsNlri::Ipv4Prefix(PrefixNlri { protocol_id, identifier, local_node: local, prefix_descriptor }))
        }
        _ => Ok(BgpLsNlri::Unknown { nlri_type, data: buf.to_vec() }),
    }
}

fn parse_node_descriptors(buf: &[u8]) -> Result<NodeDescriptor> {
    let (nd, _) = parse_node_descriptor_group(buf)?;
    Ok(nd)
}

fn parse_node_descriptor_group(buf: &[u8]) -> Result<(NodeDescriptor, &[u8])> {
    if buf.len() < 4 { return Ok((NodeDescriptor::Unknown { code: 0, data: vec![] }, buf)); }
    // Sub-TLV group: type(2) + length(2) + value
    let group_type = u16::from_be_bytes([buf[0], buf[1]]);
    let group_len  = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let group_data = if buf.len() >= 4 + group_len { &buf[4..4+group_len] } else { &[] };
    let rest       = if buf.len() >= 4 + group_len { &buf[4+group_len..] } else { &[] };

    let nd = parse_node_descriptor_sub_tlvs(group_data);
    Ok((nd, rest))
}

fn parse_node_descriptor_sub_tlvs(buf: &[u8]) -> NodeDescriptor {
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let sub_type = u16::from_be_bytes([buf[pos], buf[pos+1]]);
        let sub_len  = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) as usize;
        pos += 4;
        if pos + sub_len > buf.len() { break; }
        let sub_data = &buf[pos..pos+sub_len];
        pos += sub_len;
        match sub_type {
            512 if sub_len == 4 => {
                let asn = u32::from_be_bytes([sub_data[0], sub_data[1], sub_data[2], sub_data[3]]);
                return NodeDescriptor::Autonomous { asn };
            }
            513 if sub_len == 4 => {
                return NodeDescriptor::BgpRouterId {
                    router_id: std::net::Ipv4Addr::from([sub_data[0], sub_data[1], sub_data[2], sub_data[3]])
                };
            }
            515 if sub_len == 7 => {
                let mut id = [0u8; 7];
                id.copy_from_slice(sub_data);
                return NodeDescriptor::IsIs { iso_node_id: id };
            }
            516 if sub_len == 4 => {
                return NodeDescriptor::Ospf {
                    router_id: std::net::Ipv4Addr::from([sub_data[0], sub_data[1], sub_data[2], sub_data[3]])
                };
            }
            _ => {}
        }
    }
    NodeDescriptor::Unknown { code: 0, data: buf.to_vec() }
}

fn parse_link_descriptors(buf: &[u8]) -> Result<Vec<LinkDescriptor>> {
    let mut result = Vec::new();
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let code = u16::from_be_bytes([buf[pos], buf[pos+1]]);
        let len  = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) as usize;
        pos += 4;
        if pos + len > buf.len() { break; }
        let data = &buf[pos..pos+len];
        pos += len;
        let ld = match code {
            258 if len == 8 => {
                let local  = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let remote = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                LinkDescriptor::LinkLocalRemoteId { local, remote }
            }
            259 if len == 4 => LinkDescriptor::IPv4InterfaceAddr(
                std::net::Ipv4Addr::from([data[0], data[1], data[2], data[3]])),
            260 if len == 4 => LinkDescriptor::IPv4NeighborAddr(
                std::net::Ipv4Addr::from([data[0], data[1], data[2], data[3]])),
            _ => LinkDescriptor::Unknown { code, data: data.to_vec() },
        };
        result.push(ld);
    }
    Ok(result)
}

fn parse_prefix_descriptors(buf: &[u8]) -> Result<Vec<PrefixDescriptor>> {
    let mut result = Vec::new();
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let code = u16::from_be_bytes([buf[pos], buf[pos+1]]);
        let len  = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) as usize;
        pos += 4;
        if pos + len > buf.len() { break; }
        let data = &buf[pos..pos+len];
        pos += len;
        let pd = match code {
            264 if len >= 1 => PrefixDescriptor::OspfRouteType(data[0]),
            265 => {
                let prefix_len = data[0];
                let octets = (prefix_len as usize + 7) / 8;
                let prefix = if data.len() >= 1 + octets {
                    if octets <= 4 {
                        let mut a = [0u8; 4]; a[..octets].copy_from_slice(&data[1..1+octets]);
                        std::net::Ipv4Addr::from(a).to_string()
                    } else {
                        let mut a = [0u8; 16]; a[..octets.min(16)].copy_from_slice(&data[1..1+octets.min(16)]);
                        std::net::Ipv6Addr::from(a).to_string()
                    }
                } else { String::new() };
                PrefixDescriptor::IpReachabilityInfo { prefix, prefix_len }
            }
            _ => PrefixDescriptor::Unknown { code, data: data.to_vec() },
        };
        result.push(pd);
    }
    Ok(result)
}
```

#### RV2-2 T2 — Wire BGP-LS into PathAttributes and dispatch

**File**: `crates/rbmp-core/src/bgp/types.rs`

```rust
pub bgpls_reach:   Option<Vec<BgpLsNlri>>,
pub bgpls_unreach: Option<Vec<BgpLsNlri>>,
```

**File**: `crates/rbmp-core/src/bgp/attributes.rs`

In `dispatch_nlri_decode`, add:
```rust
(Afi::BgpLs, Safi::Unicast) | (Afi::BgpLs, Safi::Unknown(71)) => {
    // BGP-LS NLRI — store in bgpls_reach, not in prefix list
    Vec::new()
}
```

In `parse_mp_reach`, after the EVPN check, add:
```rust
if afi_safi.afi == Afi::BgpLs {
    attrs.bgpls_reach = Some(decode_bgpls_nlri(remaining)?);
}
```

#### RV2-2 T3 — BGP-LS DuckDB table

**File**: `crates/rbmp-store/src/schema.rs`

```sql
CREATE TABLE IF NOT EXISTS bgpls_nodes (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    action          VARCHAR     NOT NULL,  -- 'announce' | 'withdraw'
    protocol_id     UTINYINT,              -- 1=IS-IS L1, 2=IS-IS L2, 3=OSPFv2, 5=OSPFv3
    identifier      UBIGINT,
    node_type       VARCHAR,               -- 'isis' | 'ospf' | 'bgp'
    router_id       VARCHAR,               -- BGP router-id or ISIS iso-node-id hex
    asn             UINTEGER
);

CREATE TABLE IF NOT EXISTS bgpls_links (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    action          VARCHAR     NOT NULL,
    protocol_id     UTINYINT,
    local_router_id VARCHAR,
    remote_router_id VARCHAR,
    local_ip        VARCHAR,
    remote_ip       VARCHAR,
    link_local_id   UINTEGER,
    link_remote_id  UINTEGER
);
```

**Tests**: Unit tests for Node NLRI (IS-IS type), Link NLRI with IPv4 descriptor, IPv4 Prefix NLRI decode.

---

### Epic RV2-3: RPKI Enrichment — Proper Caching + RTR Protocol

**New crate**: `crates/rbmp-enrichment/`  
**Enhanced Python**: `bmppy/rbmppy/rpki.py` (new, replaces RPKI portion of peering.py)

#### RV2-3 T1 — rbmp-enrichment crate scaffold

```
crates/rbmp-enrichment/
├── Cargo.toml
└── src/
    ├── lib.rs          # pub mod rpki; pub mod peeringdb; pub mod registry;
    ├── rpki.rs         # VRP cache + RTR client
    ├── peeringdb.rs    # PeeringDB API + cache
    └── registry.rs     # Speaker IP → metadata mapping
```

**`Cargo.toml`**:
```toml
[package]
name = "rbmp-enrichment"
description = "Internet intelligence enrichment for rustybmp"
version.workspace = true
edition.workspace = true

[dependencies]
rbmp-core  = { workspace = true }
tokio      = { workspace = true }
serde      = { workspace = true }
serde_json = { workspace = true }
tracing    = { workspace = true }
chrono     = { workspace = true }
thiserror  = { workspace = true }
reqwest    = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
dashmap    = { workspace = true }
```

#### RV2-3 T2 — RTR Protocol Client (RFC 8210) in rbmp-enrichment

The RTR protocol is how a router (or rustybmp) fetches VRPs from an RPKI validator (rpki-client, Routinator, OctoRPKI). It runs over TCP (or TLS), using a simple binary protocol.

**File**: `crates/rbmp-enrichment/src/rpki.rs`

```rust
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;
use tracing::{info, warn, debug};
use serde::{Deserialize, Serialize};
use ipnet::{Ipv4Net, Ipv6Net};
use chrono::{DateTime, Utc};

/// A single Validated ROA Payload entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vrp {
    pub prefix:    IpAddr,
    pub prefix_len: u8,
    pub max_len:   u8,    // max prefix length
    pub origin_as: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpkiValidity {
    Valid,
    Invalid,
    NotFound,
}

impl std::fmt::Display for RpkiValidity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid    => write!(f, "valid"),
            Self::Invalid  => write!(f, "invalid"),
            Self::NotFound => write!(f, "not-found"),
        }
    }
}

/// Thread-safe VRP cache, updated via RTR or HTTP.
#[derive(Default, Clone)]
pub struct VrpCache {
    /// IPv4 prefix → list of matching VRPs
    v4: Arc<RwLock<Vec<Vrp>>>,
    /// IPv6 prefix → list of matching VRPs
    v6: Arc<RwLock<Vec<Vrp>>>,
    pub last_updated: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl VrpCache {
    pub fn new() -> Self { Self::default() }

    pub async fn load_vrps(&self, vrps: Vec<Vrp>) {
        let mut v4 = self.v4.write().await;
        let mut v6 = self.v6.write().await;
        v4.clear();
        v6.clear();
        for vrp in vrps {
            match vrp.prefix {
                IpAddr::V4(_) => v4.push(vrp),
                IpAddr::V6(_) => v6.push(vrp),
            }
        }
        *self.last_updated.write().await = Some(Utc::now());
        info!(v4_count = v4.len(), v6_count = v6.len(), "VRP cache loaded");
    }

    /// Validate a prefix/origin pair against the VRP cache.
    /// RFC 6483: A route is VALID if there is a VRP where:
    ///   - prefix is covered by VRP prefix (VRP_prefix is an equal or less-specific)
    ///   - origin AS matches the VRP origin AS
    ///   - prefix length <= VRP max_len
    /// A route is INVALID if covered by a VRP but fails origin or max_len check.
    /// A route is NOT_FOUND if no VRP covers it.
    pub async fn validate(
        &self,
        prefix: &str,
        prefix_len: u8,
        origin_as: u32,
    ) -> RpkiValidity {
        // Parse the prefix
        let addr: IpAddr = match prefix.parse() {
            Ok(a) => a,
            Err(_) => return RpkiValidity::NotFound,
        };

        let cache = match addr {
            IpAddr::V4(_) => self.v4.read().await,
            IpAddr::V6(_) => self.v6.read().await,
        };

        let mut covered = false;
        for vrp in cache.iter() {
            // Check if VRP prefix covers the announced prefix
            if !prefix_covered_by_vrp(addr, prefix_len, vrp) {
                continue;
            }
            covered = true;
            // Origin AS must match and prefix_len must not exceed max_len
            if vrp.origin_as == origin_as && prefix_len <= vrp.max_len {
                return RpkiValidity::Valid;
            }
        }

        if covered {
            RpkiValidity::Invalid
        } else {
            RpkiValidity::NotFound
        }
    }
}

fn prefix_covered_by_vrp(addr: IpAddr, prefix_len: u8, vrp: &Vrp) -> bool {
    if prefix_len < vrp.prefix_len { return false; } // announced prefix is less specific
    match (addr, vrp.prefix) {
        (IpAddr::V4(a), IpAddr::V4(v)) => {
            let mask = if vrp.prefix_len == 0 { 0u32 } else {
                !0u32 << (32 - vrp.prefix_len as u32)
            };
            (u32::from(a) & mask) == (u32::from(v) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(v)) => {
            let mask = if vrp.prefix_len == 0 { 0u128 } else {
                !0u128 << (128 - vrp.prefix_len as u128)
            };
            (u128::from(a) & mask) == (u128::from(v) & mask)
        }
        _ => false,
    }
}

/// RTR client (RFC 8210 §5) — connects to Routinator/rpki-client
/// and downloads the VRP table via the RTR binary protocol.
pub struct RtrClient {
    pub validator_addr: String,
    pub cache: VrpCache,
}

impl RtrClient {
    pub fn new(validator_addr: impl Into<String>, cache: VrpCache) -> Self {
        Self { validator_addr: validator_addr.into(), cache }
    }

    /// Connect to RTR server and perform a full reset query.
    /// Downloads all VRPs and populates the cache.
    pub async fn sync(&self) -> anyhow::Result<()> {
        info!(addr = %self.validator_addr, "RTR: connecting");
        let mut stream = TcpStream::connect(&self.validator_addr).await?;

        // RTR Reset Query: version=1, type=2 (Reset Query), session_id=0, length=8
        let reset_query: [u8; 8] = [1, 2, 0, 0, 0, 0, 0, 8];
        stream.write_all(&reset_query).await?;

        let mut vrps = Vec::new();
        loop {
            // Read RTR PDU header: version(1) + type(1) + session_id(2) + length(4)
            let mut header = [0u8; 8];
            stream.read_exact(&mut header).await?;
            let pdu_type = header[1];
            let length   = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
            let body_len = length.saturating_sub(8);
            let mut body = vec![0u8; body_len];
            if body_len > 0 {
                stream.read_exact(&mut body).await?;
            }
            match pdu_type {
                4 => {
                    // IPv4 Prefix PDU: flags(1)+prefix_len(1)+max_len(1)+zero(1)+prefix(4)+max_as(4)
                    if body.len() >= 12 {
                        let prefix_len = body[1];
                        let max_len    = body[2];
                        let prefix     = Ipv4Addr::from([body[4], body[5], body[6], body[7]]);
                        let origin_as  = u32::from_be_bytes([body[8], body[9], body[10], body[11]]);
                        if body[0] == 1 { // flags & ANNOUNCE
                            vrps.push(Vrp { prefix: IpAddr::V4(prefix), prefix_len, max_len, origin_as });
                        }
                    }
                }
                6 => {
                    // IPv6 Prefix PDU: flags(1)+prefix_len(1)+max_len(1)+zero(1)+prefix(16)+max_as(4)
                    if body.len() >= 24 {
                        let prefix_len = body[1];
                        let max_len    = body[2];
                        let mut paddr  = [0u8; 16]; paddr.copy_from_slice(&body[4..20]);
                        let prefix     = Ipv6Addr::from(paddr);
                        let origin_as  = u32::from_be_bytes([body[20], body[21], body[22], body[23]]);
                        if body[0] == 1 {
                            vrps.push(Vrp { prefix: IpAddr::V6(prefix), prefix_len, max_len, origin_as });
                        }
                    }
                }
                7 => {
                    // End of Data — sync complete
                    info!(count = vrps.len(), "RTR: End of Data received");
                    break;
                }
                10 => {
                    // Cache Reset PDU — server wants a fresh reset
                    warn!("RTR: Cache Reset received, retrying");
                    break;
                }
                _ => {}
            }
        }

        self.cache.load_vrps(vrps).await;
        Ok(())
    }

    /// Spawn a background task that re-syncs every `interval_secs` seconds.
    pub fn spawn_sync_loop(self, interval_secs: u64) {
        let client = Arc::new(self);
        tokio::spawn(async move {
            loop {
                if let Err(e) = client.sync().await {
                    warn!(error = %e, "RTR sync failed — retrying in {}s", interval_secs);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;
            }
        });
    }
}
```

#### RV2-3 T3 — HTTP-based VRP bootstrap (Cloudflare + RIPE)

For environments without a local RPKI validator, provide HTTP-based VRP loading:

```rust
// crates/rbmp-enrichment/src/rpki.rs (continue)

/// Download VRPs from Cloudflare's RPKI dataset (JSON format).
/// URL: https://rpki.cloudflare.com/rpki.json
pub async fn load_vrps_from_cloudflare(cache: &VrpCache) -> anyhow::Result<usize> {
    let resp = reqwest::get("https://rpki.cloudflare.com/rpki.json").await?;
    let body: serde_json::Value = resp.json().await?;
    let roas = body["roas"].as_array().ok_or_else(|| anyhow::anyhow!("no roas field"))?;

    let mut vrps = Vec::with_capacity(roas.len());
    for roa in roas {
        let prefix_str = roa["prefix"].as_str().unwrap_or("");
        let max_len    = roa["maxLength"].as_u64().unwrap_or(0) as u8;
        let origin_as  = roa["asn"].as_str().unwrap_or("AS0")
            .trim_start_matches("AS").parse::<u32>().unwrap_or(0);
        // Parse prefix/len
        if let Some((addr_str, len_str)) = prefix_str.split_once('/') {
            if let (Ok(addr), Ok(prefix_len)) = (addr_str.parse::<IpAddr>(), len_str.parse::<u8>()) {
                vrps.push(Vrp { prefix: addr, prefix_len, max_len, origin_as });
            }
        }
    }
    let count = vrps.len();
    cache.load_vrps(vrps).await;
    info!(count, "VRPs loaded from Cloudflare RPKI");
    Ok(count)
}
```

#### RV2-3 T4 — RPKI config and wiring into server

**File**: `crates/rbmp-server/src/config.rs`

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RpkiConfig {
    /// RTR server address (e.g., "127.0.0.1:3323" for Routinator)
    pub rtr_addr:           Option<String>,
    /// Fall back to Cloudflare RPKI JSON if RTR unavailable
    #[serde(default = "default_true")]
    pub cloudflare_fallback: bool,
    /// Resync interval in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_rpki_interval")]
    pub sync_interval_secs: u64,
    /// Disable RPKI enrichment entirely
    #[serde(default)]
    pub disabled:            bool,
}
fn default_rpki_interval() -> u64 { 3600 }
```

Wire in `main.rs`: create `VrpCache`, start RTR or Cloudflare sync, pass `Arc<VrpCache>` to the RIB manager for per-route annotation.

#### RV2-3 T5 — Per-route RPKI annotation in DuckDB

**File**: `crates/rbmp-store/src/schema.rs`

Add to `route_events`:
```sql
rpki_validity  VARCHAR,  -- 'valid' | 'invalid' | 'not-found' | NULL (unknown/unenriched)
```

In the writer, after inserting to `route_events`, if RPKI cache has data, do an async lookup and UPDATE the row:
```sql
UPDATE route_events SET rpki_validity = ? WHERE id = ?
```

Or (better for performance) — compute RPKI validity synchronously during the write, include it in the initial INSERT. Pass `Option<Arc<VrpCache>>` to the persist function.

#### RV2-3 T6 — rbmppy/rpki.py Python client

**File**: `bmppy/rbmppy/rpki.py`

```python
"""RPKI validation utilities — local VRP cache + Cloudflare API fallback."""
from __future__ import annotations

import ipaddress
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal, Optional
import httpx

RpkiState = Literal["valid", "invalid", "not-found", "unknown"]

@dataclass
class Vrp:
    prefix:     str
    prefix_len: int
    max_len:    int
    origin_as:  int

@dataclass
class ValidationResult:
    prefix:     str
    origin_as:  int
    state:      RpkiState
    matching_vrps: list[Vrp] = field(default_factory=list)


class LocalVrpCache:
    """
    In-process VRP cache loaded from a local JSON file (Routinator export,
    RIPE NCC RPKI Validator, or Cloudflare rpki.json format).

    Usage:
        cache = LocalVrpCache.load_from_cloudflare_json("rpki.json")
        result = cache.validate("192.0.2.0/24", 65000)
    """

    def __init__(self, vrps: list[Vrp]):
        self._v4: list[Vrp] = [v for v in vrps if ':' not in v.prefix]
        self._v6: list[Vrp] = [v for v in vrps if ':' in v.prefix]

    @classmethod
    def load_from_cloudflare_json(cls, path: str | Path) -> "LocalVrpCache":
        data = json.loads(Path(path).read_text())
        vrps = []
        for roa in data.get("roas", []):
            prefix_str = roa["prefix"]
            max_len    = int(roa.get("maxLength", 0))
            asn_str    = str(roa["asn"]).lstrip("AS")
            try:
                net = ipaddress.ip_network(prefix_str, strict=False)
                vrps.append(Vrp(
                    prefix=str(net.network_address),
                    prefix_len=net.prefixlen,
                    max_len=max_len,
                    origin_as=int(asn_str),
                ))
            except ValueError:
                pass
        return cls(vrps)

    @classmethod
    async def download_from_cloudflare(cls) -> "LocalVrpCache":
        """Download current VRP table from Cloudflare RPKI."""
        async with httpx.AsyncClient(timeout=60) as client:
            r = await client.get("https://rpki.cloudflare.com/rpki.json")
            r.raise_for_status()
            data = r.json()
        vrps = []
        for roa in data.get("roas", []):
            prefix_str = roa["prefix"]
            max_len    = int(roa.get("maxLength", 0))
            asn_str    = str(roa["asn"]).lstrip("AS")
            try:
                net = ipaddress.ip_network(prefix_str, strict=False)
                vrps.append(Vrp(
                    prefix=str(net.network_address),
                    prefix_len=net.prefixlen,
                    max_len=max_len,
                    origin_as=int(asn_str),
                ))
            except ValueError:
                pass
        return cls(vrps)

    def validate(self, prefix_cidr: str, origin_as: int) -> ValidationResult:
        """
        Validate a prefix/origin_as pair against the local VRP cache.
        
        RFC 6483 algorithm:
        - VALID: at least one matching VRP with correct origin + prefix_len <= max_len
        - INVALID: covered by a VRP but fails origin or max_len
        - NOT-FOUND: no VRP covers the prefix
        """
        try:
            net = ipaddress.ip_network(prefix_cidr, strict=False)
        except ValueError:
            return ValidationResult(prefix_cidr, origin_as, "unknown")

        vrp_list = self._v6 if isinstance(net, ipaddress.IPv6Network) else self._v4
        covered = False
        matching = []

        for vrp in vrp_list:
            try:
                vrp_net = ipaddress.ip_network(f"{vrp.prefix}/{vrp.prefix_len}", strict=False)
            except ValueError:
                continue
            # VRP must be an equal or less-specific prefix
            if not net.subnet_of(vrp_net) and net != vrp_net:
                continue
            covered = True
            if vrp.origin_as == origin_as and net.prefixlen <= vrp.max_len:
                matching.append(vrp)

        if matching:
            return ValidationResult(prefix_cidr, origin_as, "valid", matching)
        elif covered:
            return ValidationResult(prefix_cidr, origin_as, "invalid")
        else:
            return ValidationResult(prefix_cidr, origin_as, "not-found")

    def bulk_validate(self, routes: list[tuple[str, int]]) -> list[ValidationResult]:
        """Validate a list of (prefix_cidr, origin_as) pairs."""
        return [self.validate(prefix, origin_as) for prefix, origin_as in routes]

    @property
    def size(self) -> int:
        return len(self._v4) + len(self._v6)
```

---

### Epic RV2-4: PeeringDB + RIPE STAT Caching Client

**File**: `bmppy/rbmppy/internet.py` (new — replaces the non-caching `peering.py` functions)

```python
"""
Internet intelligence: PeeringDB, RIPE STAT, IRR/WHOIS.
All results are cached in-process with configurable TTLs.
"""
from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass, field
from typing import Optional
import httpx

@dataclass
class AsnInfo:
    asn: int
    name: str
    # From PeeringDB
    network_type: str        # "Content" | "NSP" | "Enterprise" | "Route Server" | ...
    peering_policy: str      # "Open" | "Selective" | "Restrictive" | "No"
    irr_as_set: Optional[str]
    prefixes_v4: int
    prefixes_v6: int
    ix_presences: list[str]  # list of IX names
    # From RIPE STAT
    announced_prefixes: list[str]
    rir: Optional[str]       # "RIPE" | "ARIN" | "APNIC" | "LACNIC" | "AFRINIC"
    country: Optional[str]
    org_name: Optional[str]


@dataclass
class PrefixInfo:
    prefix: str
    asn: int
    asn_name: str
    rir: Optional[str]
    country: Optional[str]
    # ROA status from RIPE STAT (separate from local cache)
    rpki_status: Optional[str]
    # IRR route object existence
    has_irr_route: Optional[bool]
    announced_since: Optional[str]  # ISO8601


class InternetIntelligenceClient:
    """
    Multi-source internet intelligence client.
    
    Caches results in-process with configurable TTLs.
    
    Sources:
    - PeeringDB API (ASN network info, IX presence, peering policy)
    - RIPE STAT API (announced prefixes, routing history, origin lookup)
    - Cloudflare RPKI API (validation status, per-prefix)
    
    Usage:
        intel = InternetIntelligenceClient()
        info  = await intel.asn_info(13335)   # Cloudflare
        pfx   = await intel.prefix_info("1.1.1.0/24")
    """

    PEERINGDB_BASE = "https://www.peeringdb.com/api"
    RIPESTAT_BASE  = "https://stat.ripe.net/data"

    def __init__(self, asn_ttl: int = 86400, prefix_ttl: int = 3600):
        self._asn_cache: dict[int, tuple[float, AsnInfo]]    = {}
        self._pfx_cache: dict[str, tuple[float, PrefixInfo]] = {}
        self.asn_ttl    = asn_ttl
        self.prefix_ttl = prefix_ttl

    async def asn_info(self, asn: int) -> Optional[AsnInfo]:
        now = time.monotonic()
        if asn in self._asn_cache:
            ts, data = self._asn_cache[asn]
            if now - ts < self.asn_ttl:
                return data

        info = await self._fetch_asn_info(asn)
        if info:
            self._asn_cache[asn] = (now, info)
        return info

    async def prefix_info(self, prefix: str) -> Optional[PrefixInfo]:
        now = time.monotonic()
        if prefix in self._pfx_cache:
            ts, data = self._pfx_cache[prefix]
            if now - ts < self.prefix_ttl:
                return data

        info = await self._fetch_prefix_info(prefix)
        if info:
            self._pfx_cache[prefix] = (now, info)
        return info

    async def bulk_asn_info(self, asns: list[int]) -> dict[int, Optional[AsnInfo]]:
        tasks = {asn: asyncio.create_task(self.asn_info(asn)) for asn in asns}
        return {asn: await task for asn, task in tasks.items()}

    async def _fetch_asn_info(self, asn: int) -> Optional[AsnInfo]:
        try:
            async with httpx.AsyncClient(timeout=15) as client:
                # PeeringDB
                pdb_r = await client.get(f"{self.PEERINGDB_BASE}/net?asn={asn}")
                pdb_data = pdb_r.json().get("data", [{}])[0] if pdb_r.status_code == 200 else {}
                # RIPE STAT — routing history
                rs_r = await client.get(
                    f"{self.RIPESTAT_BASE}/ris-asns/data.json?query_time=latest",
                )
                # RIPE STAT — prefixes
                ap_r = await client.get(
                    f"{self.RIPESTAT_BASE}/announced-prefixes/data.json?resource=AS{asn}"
                )
                ap_data = ap_r.json().get("data", {}) if ap_r.status_code == 200 else {}
                prefixes = [p["prefix"] for p in ap_data.get("prefixes", [])][:50]

                # IX presence (PeeringDB)
                ix_r = await client.get(f"{self.PEERINGDB_BASE}/netixlan?asn={asn}")
                ix_data = ix_r.json().get("data", []) if ix_r.status_code == 200 else []
                ix_names = list({ix.get("name", "") for ix in ix_data if ix.get("name")})

            return AsnInfo(
                asn=asn,
                name=pdb_data.get("name", ""),
                network_type=pdb_data.get("info_type", ""),
                peering_policy=pdb_data.get("policy_general", ""),
                irr_as_set=pdb_data.get("irr_as_set") or None,
                prefixes_v4=pdb_data.get("info_prefixes4", 0),
                prefixes_v6=pdb_data.get("info_prefixes6", 0),
                ix_presences=ix_names,
                announced_prefixes=prefixes,
                rir=None,
                country=pdb_data.get("country") or None,
                org_name=pdb_data.get("org") or None,
            )
        except Exception:
            return None

    async def _fetch_prefix_info(self, prefix: str) -> Optional[PrefixInfo]:
        try:
            async with httpx.AsyncClient(timeout=15) as client:
                po_r = await client.get(
                    f"{self.RIPESTAT_BASE}/prefix-overview/data.json?resource={prefix}"
                )
                po_data = po_r.json().get("data", {}) if po_r.status_code == 200 else {}
                asn = 0
                asn_name = ""
                asns_list = po_data.get("asns", [])
                if asns_list:
                    asn = asns_list[0].get("asn", 0)
                    asn_name = asns_list[0].get("holder", "")

            return PrefixInfo(
                prefix=prefix,
                asn=asn,
                asn_name=asn_name,
                rir=po_data.get("resource"),
                country=po_data.get("country") or None,
                rpki_status=None,
                has_irr_route=None,
                announced_since=None,
            )
        except Exception:
            return None

    def cache_stats(self) -> dict:
        return {"asn_cache_size": len(self._asn_cache), "prefix_cache_size": len(self._pfx_cache)}
```

---

### Epic RV2-5: Analytics Engine — Z-score + Hijack/Leak Detection

**File**: `bmppy/rbmppy/analytics.py` (full rewrite implementing the paper's model)

```python
"""
BGP Analytics Engine — Z-score anomaly detection, hijack detection, route leak detection.

Based on: Hiremath (2026) "Real-Time BGP Monitoring with BMP and Streaming Telemetry"
         IMACSI 2025, equations 2–4.

Performance targets (from paper Table 1):
  - Anomaly detection accuracy: 97.2%
  - False positive rate: 2.1%
  - Prefix hijack detection: 100%
  - Alert dispatch: < 300ms
"""
from __future__ import annotations

import math
import time
from collections import defaultdict, deque
from dataclasses import dataclass, field
from typing import Optional, Callable
from datetime import datetime, timezone, timedelta
import duckdb
import pandas as pd


# ─── Anomaly alert types ──────────────────────────────────────────────────────

@dataclass
class AnomalyAlert:
    alert_type:    str          # "frequency_spike" | "origin_change" | "route_leak" | "session_flap"
    prefix:        str
    speaker_addr:  str
    peer_addr:     str
    z_score:       Optional[float]
    detail:        str
    severity:      str          # "warning" | "critical"
    occurred_at:   datetime = field(default_factory=lambda: datetime.now(timezone.utc))


# ─── Per-prefix frequency tracker (equations 2-4 from paper) ─────────────────

class PrefixAnomalyDetector:
    """
    Detects anomalous prefix announcement frequency using a Z-score model.
    
    For each prefix Pi, maintains a sliding window of announcement counts
    per time slot. When a new event arrives:
      1. Update frequency count for current time slot
      2. Compute μPi and σPi over the window
      3. Compute Zi = (f(Pi, Tnew) - μPi) / σPi
      4. Fire alert if |Zi| > threshold (default 3.0)
    
    Also detects origin AS changes, which signal possible hijacking.
    """

    def __init__(
        self,
        window_slots:  int   = 12,   # number of time slots in window
        slot_seconds:  int   = 300,  # 5 minutes per slot = 1 hour total window
        z_threshold:   float = 3.0,  # Z-score threshold for anomaly
        min_slots:     int   = 3,    # minimum history before detection fires
    ):
        self.window_slots = window_slots
        self.slot_seconds = slot_seconds
        self.z_threshold  = z_threshold
        self.min_slots    = min_slots

        # prefix → deque of (slot_ts, count)
        self._freq: dict[str, deque]  = defaultdict(lambda: deque(maxlen=window_slots))
        # prefix → last_seen_origin_as
        self._origins: dict[str, int] = {}
        # prefix → current slot (slot_ts, count)
        self._current: dict[str, tuple[int, int]] = {}

    def _slot_ts(self) -> int:
        """Current slot timestamp (floor to slot_seconds)."""
        now = int(time.time())
        return now - (now % self.slot_seconds)

    def record(
        self,
        prefix:      str,
        action:      str,   # "announce" | "withdraw"
        origin_as:   Optional[int],
        speaker:     str,
        peer:        str,
    ) -> list[AnomalyAlert]:
        alerts: list[AnomalyAlert] = []
        slot = self._slot_ts()

        # Update frequency for current slot
        if prefix in self._current:
            cs, cc = self._current[prefix]
            if cs == slot:
                self._current[prefix] = (slot, cc + 1)
            else:
                # Commit completed slot to window
                self._freq[prefix].append(cc)
                self._current[prefix] = (slot, 1)
        else:
            self._current[prefix] = (slot, 1)

        # Z-score check (only on announce to avoid over-counting)
        if action == "announce" and len(self._freq[prefix]) >= self.min_slots:
            counts = list(self._freq[prefix])
            current_count = self._current[prefix][1]
            n  = len(counts)
            mu = sum(counts) / n
            if n > 1:
                sigma = math.sqrt(sum((c - mu) ** 2 for c in counts) / n)
            else:
                sigma = 0.0

            if sigma > 0:
                zi = (current_count - mu) / sigma
                if abs(zi) > self.z_threshold:
                    alerts.append(AnomalyAlert(
                        alert_type="frequency_spike",
                        prefix=prefix,
                        speaker_addr=speaker,
                        peer_addr=peer,
                        z_score=zi,
                        detail=(
                            f"Announcement frequency {current_count} vs "
                            f"baseline μ={mu:.1f} σ={sigma:.1f} → Z={zi:.2f}"
                        ),
                        severity="critical" if abs(zi) > 5.0 else "warning",
                    ))

        # Origin AS change detection (hijack signal)
        if action == "announce" and origin_as is not None:
            prev_origin = self._origins.get(prefix)
            if prev_origin is not None and prev_origin != origin_as:
                alerts.append(AnomalyAlert(
                    alert_type="origin_change",
                    prefix=prefix,
                    speaker_addr=speaker,
                    peer_addr=peer,
                    z_score=None,
                    detail=(
                        f"Origin AS changed: {prev_origin} → {origin_as} "
                        f"(possible BGP hijack)"
                    ),
                    severity="critical",
                ))
            self._origins[prefix] = origin_as

        return alerts


# ─── Session flap detector ────────────────────────────────────────────────────

class SessionFlapDetector:
    """
    Detects BGP session instability.
    
    Fires when a peer goes down ≥ threshold times within window_seconds.
    Also detects simultaneous multi-peer collapse (blast radius event).
    """

    def __init__(self, threshold: int = 3, window_seconds: int = 300):
        self.threshold      = threshold
        self.window_seconds = window_seconds
        # (speaker, peer) → deque of down-event timestamps
        self._downs: dict[tuple[str, str], deque] = defaultdict(deque)

    def record_peer_down(
        self, speaker: str, peer: str
    ) -> Optional[AnomalyAlert]:
        now    = time.time()
        cutoff = now - self.window_seconds
        key    = (speaker, peer)
        q      = self._downs[key]

        # Trim old events
        while q and q[0] < cutoff:
            q.popleft()
        q.append(now)

        if len(q) >= self.threshold:
            return AnomalyAlert(
                alert_type="session_flap",
                prefix="",
                speaker_addr=speaker,
                peer_addr=peer,
                z_score=None,
                detail=(
                    f"BGP peer {peer} on speaker {speaker} has gone down "
                    f"{len(q)} times in {self.window_seconds}s"
                ),
                severity="critical",
            )
        return None

    def flap_count(self, speaker: str, peer: str) -> int:
        return len(self._downs.get((speaker, peer), []))


# ─── Route leak detector ──────────────────────────────────────────────────────

class RouteLeakDetector:
    """
    Detect BGP route leaks using RFC 9234 OTC attribute.
    
    A route leak occurs when a customer route is re-advertised to a provider
    or peer without the OTC attribute being respected.
    
    Detection methods:
    1. OTC attribute present on a route received from a provider
       (OTC should only be set by providers; if a customer-learned route
       carries OTC, it may indicate a leak)
    2. AS path analysis: route appearing in unexpected AS relationship
       (customer AS appearing in provider's route table advertising routes
       that include both provider ASes — valley-free violation)
    """

    def __init__(self, local_asn: int = 0, provider_asns: Optional[list[int]] = None):
        self.local_asn    = local_asn
        self.provider_asns = set(provider_asns or [])

    def check(
        self,
        prefix:       str,
        as_path:      Optional[str],
        otc_asn:      Optional[int],   # from OTC attribute (type 35)
        speaker:      str,
        peer:         str,
    ) -> Optional[AnomalyAlert]:
        # OTC attribute present on a route from provider → possible leak
        if otc_asn is not None and self.provider_asns and peer in {str(a) for a in self.provider_asns}:
            return AnomalyAlert(
                alert_type="route_leak",
                prefix=prefix,
                speaker_addr=speaker,
                peer_addr=peer,
                z_score=None,
                detail=(
                    f"OTC attribute (AS {otc_asn}) present on route from "
                    f"provider peer {peer} — possible route leak"
                ),
                severity="warning",
            )
        return None


# ─── DuckDB-backed analytics ──────────────────────────────────────────────────

class RouteAnalytics:
    """
    DuckDB-backed BGP analytics.
    Provides historical analysis across all stored route events.
    """

    def __init__(self, db_path: str):
        self.conn = duckdb.connect(db_path, read_only=True)

    def current_rib(self, peer_addr: Optional[str] = None) -> pd.DataFrame:
        """
        Return the current RIB (latest announced state per prefix).
        Correctly excludes prefixes whose most recent event is 'withdraw'.
        """
        peer_filter = f"AND peer_addr = '{peer_addr}'" if peer_addr else ""
        return self.conn.execute(f"""
            SELECT * FROM (
                SELECT *, ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
                FROM route_events
                WHERE 1=1 {peer_filter}
            ) WHERE rn = 1 AND action = 'announce'
        """).df()

    def prefix_history(self, prefix: str, limit: int = 200) -> pd.DataFrame:
        return self.conn.execute("""
            SELECT occurred_at, speaker_addr, peer_addr, peer_as, rib_type, action,
                   as_path, next_hop, local_pref, med, communities, rpki_validity
            FROM route_events
            WHERE prefix = ?
            ORDER BY occurred_at DESC
            LIMIT ?
        """, [prefix, limit]).df()

    def top_churning_prefixes(self, limit: int = 20, hours: int = 24) -> pd.DataFrame:
        """Top N most active prefixes by announce+withdraw count in the last N hours."""
        return self.conn.execute("""
            SELECT prefix,
                   COUNT(*) AS total_events,
                   SUM(CASE WHEN action='announce' THEN 1 ELSE 0 END) AS announces,
                   SUM(CASE WHEN action='withdraw' THEN 1 ELSE 0 END) AS withdraws
            FROM route_events
            WHERE occurred_at >= NOW() - INTERVAL (?) HOUR
            GROUP BY prefix
            ORDER BY total_events DESC
            LIMIT ?
        """, [hours, limit]).df()

    def peer_session_history(self, peer_addr: Optional[str] = None) -> pd.DataFrame:
        peer_filter = f"AND peer_addr = '{peer_addr}'" if peer_addr else ""
        return self.conn.execute(f"""
            SELECT occurred_at, speaker_addr, peer_addr, peer_as, event_type,
                   local_as, hold_time, reason
            FROM peer_events
            WHERE 1=1 {peer_filter}
            ORDER BY occurred_at DESC
            LIMIT 500
        """).df()

    def rpki_summary(self) -> pd.DataFrame:
        """Breakdown of RPKI validity status across all current routes."""
        return self.conn.execute("""
            SELECT rpki_validity, COUNT(*) AS route_count
            FROM (
                SELECT prefix, rpki_validity,
                       ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
                FROM route_events WHERE action = 'announce'
            ) WHERE rn = 1
            GROUP BY rpki_validity
        """).df()

    def origin_as_distribution(self, top_n: int = 25) -> pd.DataFrame:
        """Top N origin ASes by route count."""
        return self.conn.execute("""
            SELECT CAST(trim(list_last(string_split(as_path, ' '))) AS INTEGER) AS origin_asn,
                   COUNT(DISTINCT prefix) AS prefix_count
            FROM route_events
            WHERE action = 'announce' AND as_path IS NOT NULL AND as_path <> ''
            GROUP BY origin_asn
            ORDER BY prefix_count DESC
            LIMIT ?
        """, [top_n]).df()

    def convergence_times(self, hours: int = 24) -> pd.DataFrame:
        """
        Compute BGP convergence time for each prefix withdraw/re-announce cycle.
        Returns: prefix, withdraw_time, re_announce_time, convergence_ms
        """
        return self.conn.execute("""
            WITH ordered AS (
                SELECT prefix, action, occurred_at,
                       LEAD(action)       OVER (PARTITION BY prefix ORDER BY occurred_at) AS next_action,
                       LEAD(occurred_at)  OVER (PARTITION BY prefix ORDER BY occurred_at) AS next_time
                FROM route_events
                WHERE occurred_at >= NOW() - INTERVAL (?) HOUR
            )
            SELECT prefix,
                   occurred_at AS withdraw_time,
                   next_time   AS re_announce_time,
                   DATEDIFF('millisecond', occurred_at, next_time) AS convergence_ms
            FROM ordered
            WHERE action = 'withdraw'
              AND next_action = 'announce'
              AND next_time IS NOT NULL
            ORDER BY convergence_ms DESC
            LIMIT 100
        """, [hours]).df()

    def as_path_delta(self) -> pd.DataFrame:
        """
        Detect prefixes where different peers advertise different AS_PATHs.
        High path diversity may indicate manipulation or policy divergence.
        """
        return self.conn.execute("""
            WITH latest AS (
                SELECT prefix, peer_addr, as_path,
                       ROW_NUMBER() OVER (PARTITION BY prefix, peer_addr ORDER BY occurred_at DESC) AS rn
                FROM route_events WHERE action = 'announce' AND as_path IS NOT NULL
            )
            SELECT prefix,
                   COUNT(DISTINCT peer_addr) AS peer_count,
                   COUNT(DISTINCT as_path)   AS unique_paths,
                   MIN(as_path)              AS path_sample_1,
                   MAX(as_path)              AS path_sample_2
            FROM latest
            WHERE rn = 1
            GROUP BY prefix
            HAVING COUNT(DISTINCT as_path) > 1
            ORDER BY unique_paths DESC
            LIMIT 50
        """).df()

    def stats_timeline(
        self,
        speaker_addr: str,
        peer_addr: str,
        counter_name: str,
        hours: int = 24,
    ) -> pd.DataFrame:
        """Time-series of a specific BMP stats counter for a peer."""
        return self.conn.execute("""
            SELECT occurred_at, counter_value, afi, safi
            FROM stats_events
            WHERE speaker_addr = ? AND peer_addr = ? AND counter_name = ?
              AND occurred_at >= NOW() - INTERVAL (?) HOUR
            ORDER BY occurred_at
        """, [speaker_addr, peer_addr, counter_name, hours]).df()


# ─── Feature extraction for ML ───────────────────────────────────────────────

def extract_route_features(route_event: dict) -> dict:
    """
    Extract ML feature vector from a route change event (equation 1 from paper).
    
    Returns a dict suitable for scikit-learn / pandas feature matrix.
    """
    as_path = route_event.get("as_path", "") or ""
    asns    = [int(a) for a in as_path.split() if a.isdigit()]

    # Private ASN ranges
    def is_private(asn: int) -> bool:
        return 64512 <= asn <= 65534 or 4200000000 <= asn <= 4294967294

    prev = None
    has_prepend = False
    for a in asns:
        if a == prev:
            has_prepend = True
            break
        prev = a

    communities = route_event.get("communities", "") or ""
    community_count = len([c for c in communities.split(",") if c.strip()])

    return {
        # AS_PATH features
        "hop_count":              len(asns),
        "origin_asn":             asns[-1] if asns else 0,
        "first_asn":              asns[0]  if asns else 0,
        "unique_asns":            len(set(asns)),
        "has_prepend":            int(has_prepend),
        "has_private_asn":        int(any(is_private(a) for a in asns)),
        "path_diversity_ratio":   len(set(asns)) / max(len(asns), 1),

        # Route attributes
        "has_local_pref":         int(route_event.get("local_pref") is not None),
        "local_pref":             route_event.get("local_pref") or 0,
        "has_med":                int(route_event.get("med") is not None),
        "med":                    route_event.get("med") or 0,
        "community_count":        community_count,
        "is_announce":            int(route_event.get("action") == "announce"),

        # RPKI
        "rpki_valid":             int(route_event.get("rpki_validity") == "valid"),
        "rpki_invalid":           int(route_event.get("rpki_validity") == "invalid"),
        "rpki_not_found":         int(route_event.get("rpki_validity") == "not-found"),
    }
```

---

### Epic RV2-6: Receiver Supervisor (Reconnect + Speaker Management)

**Scope**: `crates/rbmp-server/`  
**New file**: `crates/rbmp-server/src/supervisor.rs`

When an XRD speaker drops its TCP connection to rustybmp (router reboot, link flap, BMP config change), the current code just logs the error and exits `handle_connection`. Routes remain stale in the RIB. There is no reconnect.

```rust
// crates/rbmp-server/src/supervisor.rs

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use rbmp_core::bmp::types::BmpMessage;
use crate::archive::BmpArchive;
use crate::config::BmpConfig;
use crate::governor::ShedSignal;

/// Tracks per-speaker connection state
#[derive(Debug, Clone)]
pub struct SpeakerState {
    pub addr:              IpAddr,
    pub connected:         bool,
    pub reconnect_attempts: u32,
    pub last_seen:         std::time::Instant,
}

/// Spawn a supervisor that:
/// 1. Detects when a speaker disconnects (TCP EOF)
/// 2. Sends a synthetic SpeakerDown message so the RIB clears routes
/// 3. Waits with exponential backoff before allowing reconnect
///
/// This is a passive supervisor — XRD speakers CONNECT to us (BMP is
/// always initiated by the router). So "reconnect" here means: after a
/// speaker disappears, emit the SpeakerDown event, then mark the speaker
/// as "awaiting reconnect" so when they do reconnect we know it's a recovery.
pub struct ReceiverSupervisor {
    speakers: HashMap<IpAddr, SpeakerState>,
    msg_tx:   mpsc::Sender<BmpMessage>,
}

impl ReceiverSupervisor {
    pub fn new(msg_tx: mpsc::Sender<BmpMessage>) -> Self {
        Self { speakers: HashMap::new(), msg_tx }
    }

    pub fn on_connect(&mut self, addr: IpAddr) {
        let state = self.speakers.entry(addr).or_insert_with(|| SpeakerState {
            addr,
            connected: false,
            reconnect_attempts: 0,
            last_seen: std::time::Instant::now(),
        });
        if state.connected {
            // Double-connect: already connected speaker sent new connection
            // This happens after router BMP session reset
            warn!(%addr, "Speaker reconnected (duplicate connection detected)");
        }
        state.connected = true;
        state.last_seen  = std::time::Instant::now();
        info!(%addr, "Speaker connected");
    }

    pub fn on_disconnect(&mut self, addr: IpAddr) {
        if let Some(state) = self.speakers.get_mut(&addr) {
            state.connected = false;
            state.reconnect_attempts += 1;
            info!(%addr, attempts = state.reconnect_attempts, "Speaker disconnected");
        }
    }

    /// Compute the wait before marking a speaker as definitively offline.
    /// Uses exponential backoff: 5s, 10s, 20s, 40s, max 120s.
    pub fn backoff_secs(attempts: u32) -> u64 {
        let base: u64 = 5;
        let max: u64  = 120;
        (base * 2u64.pow(attempts.min(5))).min(max)
    }
}
```

#### RV2-6 T2 — Synthetic BmpPayload::Termination on TCP drop

**File**: `crates/rbmp-server/src/receiver.rs`

When `handle_connection` exits (any reason), synthesize a `BmpMessage::Termination` so the `RibManager` cleanly removes the speaker:

```rust
// At the end of handle_connection, after the loop exits:
let termination = BmpMessage {
    id:           uuid::Uuid::new_v4(),
    received_at:  chrono::Utc::now(),
    speaker_addr: speaker_addr.into(),
    payload: rbmp_core::bmp::types::BmpPayload::Termination {
        reason_code: 0xFFFF,  // synthetic: TCP disconnect
        reason_text: Some("TCP connection closed".to_string()),
    },
};
let _ = tx.send(termination).await;
info!(%peer, "Synthetic Termination sent — RIB will clear routes");
```

This is critical for correctness: without it, the RIB holds stale routes for a disconnected speaker forever.

---

### Epic RV2-7: Write Coordinator — Batched DuckDB Inserts

**Scope**: `crates/rbmp-store/`  
**New file**: `crates/rbmp-store/src/coordinator.rs`

At 1500 msg/sec, individual DuckDB inserts behind a `Mutex<RouteStore>` become the bottleneck. Batch writes significantly improve throughput.

```rust
// crates/rbmp-store/src/coordinator.rs

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, warn};
use rbmp_rib::event::RibEvent;
use crate::duck::RouteStore;
use crate::writer::persist;

/// Batches DuckDB writes for high-throughput performance.
/// Accumulates events for `flush_interval_ms` or `batch_size` events,
/// then flushes in a single implicit DuckDB transaction.
pub struct WriteCoordinator {
    batch_size:        usize,
    flush_interval_ms: u64,
}

impl WriteCoordinator {
    pub fn new(batch_size: usize, flush_interval_ms: u64) -> Self {
        Self { batch_size, flush_interval_ms }
    }

    /// Spawn the coordinator task. Consumes from `rx`, writes to `store`.
    pub fn spawn(
        self,
        store: Arc<Mutex<RouteStore>>,
        mut rx: broadcast::Receiver<RibEvent>,
    ) {
        tokio::spawn(async move {
            let mut batch: Vec<RibEvent> = Vec::with_capacity(self.batch_size);
            let flush_interval = Duration::from_millis(self.flush_interval_ms);
            let mut deadline   = tokio::time::Instant::now() + flush_interval;

            loop {
                tokio::select! {
                    result = rx.recv() => {
                        match result {
                            Ok(ev) => {
                                batch.push(ev);
                                if batch.len() >= self.batch_size {
                                    Self::flush(&store, &mut batch);
                                    deadline = tokio::time::Instant::now() + flush_interval;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!(%n, "WriteCoordinator lagged — {} events dropped", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        if !batch.is_empty() {
                            Self::flush(&store, &mut batch);
                        }
                        deadline = tokio::time::Instant::now() + flush_interval;
                    }
                }
            }

            // Final flush on shutdown
            if !batch.is_empty() {
                Self::flush(&store, &mut batch);
            }
        });
    }

    fn flush(store: &Arc<Mutex<RouteStore>>, batch: &mut Vec<RibEvent>) {
        let locked = store.lock().unwrap();
        // Begin transaction for the entire batch
        if let Err(e) = locked.conn().execute_batch("BEGIN TRANSACTION") {
            error!(?e, "Failed to begin batch transaction");
            batch.clear();
            return;
        }
        let mut written = 0usize;
        for ev in batch.drain(..) {
            if let Err(e) = persist(store, &ev) {
                error!(?e, "Failed to persist event in batch");
            } else {
                written += 1;
            }
        }
        if let Err(e) = locked.conn().execute_batch("COMMIT") {
            error!(?e, "Failed to commit batch transaction");
        }
        debug!(written, "WriteCoordinator: batch committed");
    }
}
```

**Config addition** in `crates/rbmp-server/src/config.rs`:
```rust
pub struct StoreConfig {
    // ... existing fields ...
    #[serde(default = "default_batch_size")]
    pub write_batch_size:     usize,   // default: 100
    #[serde(default = "default_flush_ms")]
    pub write_flush_interval_ms: u64,  // default: 50ms
}
fn default_batch_size()  -> usize { 100 }
fn default_flush_ms()    -> u64   { 50  }
```

Replace `run_store_writer` call in `main.rs` with `WriteCoordinator::new(cfg.store.write_batch_size, cfg.store.write_flush_interval_ms).spawn(...)`.

---

### Epic RV2-8: Speaker Registry — IP to Metadata Mapping

**Scope**: `crates/rbmp-server/`  
**New file**: `crates/rbmp-server/src/registry.rs`

Operators need to know that `10.0.0.1` is `xrd-pe1 (Cisco IOS-XR, Site-A)`. Currently, all API responses only show bare IP addresses.

```rust
// crates/rbmp-server/src/registry.rs

use std::collections::HashMap;
use std::net::IpAddr;
use serde::{Deserialize, Serialize};

/// Rich metadata for a known BMP speaker
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpeakerMeta {
    /// Human-readable hostname (e.g., "xrd-pe1")
    pub hostname:  String,
    /// Vendor string (e.g., "Cisco IOS-XR", "Nokia SR Linux", "FRRouting")
    pub vendor:    String,
    /// Role in the network (e.g., "pe", "rr", "p", "border")
    pub role:      String,
    /// Site name (e.g., "Singapore-DC1")
    pub site:      String,
    /// AS number that this speaker belongs to
    pub asn:       Option<u32>,
}

impl SpeakerMeta {
    pub fn display_name(&self) -> &str {
        if self.hostname.is_empty() { &self.vendor } else { &self.hostname }
    }
}

/// Thread-safe registry of BMP speaker metadata.
/// Populated from config at startup and enriched from BMP Initiation messages.
#[derive(Debug, Clone, Default)]
pub struct SpeakerRegistry {
    entries: HashMap<IpAddr, SpeakerMeta>,
}

impl SpeakerRegistry {
    pub fn new() -> Self { Self::default() }

    /// Load from config file entries
    pub fn load(entries: Vec<(IpAddr, SpeakerMeta)>) -> Self {
        Self { entries: entries.into_iter().collect() }
    }

    pub fn get(&self, addr: &IpAddr) -> Option<&SpeakerMeta> {
        self.entries.get(addr)
    }

    /// Auto-register a speaker from its BMP Initiation message.
    /// Only adds hostname/descr if not already in registry.
    pub fn register_from_initiation(
        &mut self,
        addr: IpAddr,
        sys_name: Option<&str>,
        sys_descr: Option<&str>,
    ) {
        let entry = self.entries.entry(addr).or_default();
        if entry.hostname.is_empty() {
            if let Some(name) = sys_name {
                entry.hostname = name.to_string();
            }
        }
        if entry.vendor.is_empty() {
            if let Some(descr) = sys_descr {
                // Infer vendor from sysDescr (best-effort)
                entry.vendor = infer_vendor(descr).to_string();
            }
        }
    }
}

fn infer_vendor(sys_descr: &str) -> &str {
    let d = sys_descr.to_lowercase();
    if d.contains("cisco")     { "Cisco" }
    else if d.contains("ios-xr")  { "Cisco IOS-XR" }
    else if d.contains("juniper") { "Juniper" }
    else if d.contains("nokia")   { "Nokia" }
    else if d.contains("arista")  { "Arista" }
    else if d.contains("frrouting") || d.contains("frr") { "FRRouting" }
    else { "Unknown" }
}
```

**Config addition**:
```toml
# rustybmp.toml
[[speakers]]
addr     = "10.0.0.1"
hostname = "xrd-pe1"
vendor   = "Cisco IOS-XR"
role     = "pe"
site     = "Singapore-DC1"
asn      = 65000

[[speakers]]
addr     = "10.0.0.2"
hostname = "xrd-pe2"
vendor   = "Cisco IOS-XR"
role     = "pe"
site     = "Singapore-DC1"
asn      = 65000
```

**API enrichment**: All `/api/speakers` and `/api/peers` responses should include the registry metadata when available.

---

### Epic RV2-9: LLGR State Machine

**Scope**: `crates/rbmp-rib/src/session.rs`, `crates/rbmp-rib/src/table.rs`

RFC 9494 LLGR extends GR to allow stale routes to remain for a much longer time (configurable, up to weeks). The capability is already parsed in RV1. Now we need the state machine.

```rust
// crates/rbmp-rib/src/session.rs — additions

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlgrState {
    /// AFI-SAFIs for which LLGR is active with this peer
    pub active_families: Vec<(AfiSafi, u32)>, // (AFI-SAFI, stale_time_secs)
    /// When LLGR stale timer started for each family (on PeerDown)
    pub stale_started: HashMap<String, DateTime<Utc>>,
}

impl PeerSession {
    pub fn on_llgr_peer_down(&mut self, at: DateTime<Utc>) {
        for (afi_safi, _) in &self.llgr_state.active_families {
            self.llgr_state.stale_started
                .insert(format!("{}", afi_safi), at);
        }
    }

    pub fn llgr_stale_expired(&self, afi_safi: &AfiSafi, now: DateTime<Utc>) -> bool {
        let key = format!("{}", afi_safi);
        if let Some(started) = self.llgr_state.stale_started.get(&key) {
            // Find the stale_time for this family
            if let Some((_, stale_time_secs)) = self.llgr_state.active_families.iter()
                .find(|(a, _)| a == afi_safi)
            {
                let elapsed = (now - *started).num_seconds() as u64;
                return elapsed > *stale_time_secs as u64;
            }
        }
        false
    }
}
```

**File**: `crates/rbmp-rib/src/table.rs`

```rust
pub struct RibEntry {
    // ... existing ...
    /// Route is LLGR-stale (RFC 9494): should be kept but marked stale
    pub llgr_stale: bool,
}
```

Stats type 28 (`per-afi-safi-llgr-stale-routes`) from RFC 9972 should now reflect actual stale route counts.

---

### Epic RV2-10: Prometheus Metrics Improvements

**Scope**: `crates/rbmp-server/src/api/health.rs`

Currently `/metrics` is a skeleton. Expose meaningful BGP monitoring metrics:

```rust
// In health.rs metrics handler:

// BMP speaker state (gauge per speaker)
gauge!("rustybmp_speaker_up",
    "Speaker connection state (1=up, 0=down)",
    "speaker" => speaker_addr, "hostname" => hostname
);

// Per-peer route counts (gauge)
gauge!("rustybmp_peer_routes_total",
    "Current route count per peer per RIB type",
    "peer" => peer_addr, "rib_type" => rib_type
);

// Per-peer session state (gauge: 1=up, 0=down)
gauge!("rustybmp_peer_up",
    "BGP peer session state",
    "speaker" => speaker_addr, "peer" => peer_addr
);

// Route event rate (counter)
counter!("rustybmp_route_events_total",
    "Total route change events (announce + withdraw)",
    "action" => action, "afi" => afi
);

// RPKI validity distribution (gauge)
gauge!("rustybmp_rpki_routes_valid",    "Routes with valid RPKI status");
gauge!("rustybmp_rpki_routes_invalid",  "Routes with invalid RPKI status");
gauge!("rustybmp_rpki_routes_notfound", "Routes with not-found RPKI status");

// BMP stats passthrough — RFC 9972 gauges
// For each stat type received, expose as a gauge
gauge!("rustybmp_bmp_stat",
    "BMP statistics counter value (RFC 7854 + RFC 9972)",
    "speaker" => speaker_addr, "peer" => peer_addr,
    "stat_type" => stat_type_str, "afi" => afi_str, "safi" => safi_str
);

// Write coordinator performance
gauge!("rustybmp_write_batch_size",   "Average DuckDB write batch size");
counter!("rustybmp_write_batches_total", "Total DuckDB batch flushes");

// EVPN route counts
gauge!("rustybmp_evpn_routes",
    "Current EVPN routes per type",
    "route_type" => route_type_name
);
```

---

### Epic RV2-11: Distributed Collector/Core Architecture

This is the "scalable core + collector" the original project brief specified. Addresses large deployments with BMP speakers at multiple network sites.

**New crates** (scaffold only in RV2, full implementation in RV3):
- `crates/rbmp-collector/` — standalone collector binary
- `crates/rbmp-core-service/` — standalone core binary

**Protocol** (`crates/rbmp-core/src/collector_protocol.rs`):

```rust
// Message format for collector → core forwarding
// Using MessagePack over TCP for efficiency (no Protobuf dependency)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorEnvelope {
    /// Collector ID (unique per site)
    pub collector_id:   String,
    /// Collector site name
    pub site:           String,
    /// Original BMP message (as received)
    pub bmp_message:    BmpMessage,
}
```

**`rbmp-collector` binary** (scaffold):
- Accepts BMP on port 5000 (as today)
- Forwards `CollectorEnvelope` messages to Core via TCP (default port 5001)
- Reconnects to Core with exponential backoff
- Local buffer (ring buffer): when Core is unreachable, buffers up to N messages

**`rbmp-core-service` binary** (scaffold):
- Listens on port 5001 for collector connections
- Runs RIBManager, DuckDB store, HTTP API (as today)
- Labels every event with `collector_id` + `site`

**DuckDB schema additions** for multi-collector:
```sql
-- Add to all event tables:
collector_id  VARCHAR,
site          VARCHAR
```

This allows queries like: "show me all hijack events across all collectors" or "compare route tables between Singapore and London collectors."

---

## Part 5 — rbmppy Additions for RV2

### New Python files

```
bmppy/rbmppy/
├── rpki.py           # RV2-3: Local VRP cache + Cloudflare bootstrap (new)
├── internet.py       # RV2-4: PeeringDB + RIPE STAT caching client (new)
├── detectors.py      # RV2-5: Real-time detector pipeline (new)
└── analytics.py      # RV2-5: Full rewrite (Z-score + DuckDB analytics)
```

**`detectors.py`** — the real-time alerting pipeline:

```python
"""
Real-time detector pipeline.
Connects to the rustybmp SSE stream and runs all detectors on incoming events.

Usage:
    pipeline = DetectorPipeline("http://localhost:7878")
    pipeline.add_handler(lambda alert: print(f"ALERT: {alert}"))
    await pipeline.run()
"""
from __future__ import annotations
import asyncio
from typing import Callable, Awaitable
from .stream import event_stream
from .analytics import PrefixAnomalyDetector, SessionFlapDetector, RouteLeakDetector, AnomalyAlert

AlertHandler = Callable[[AnomalyAlert], Awaitable[None]]

class DetectorPipeline:
    def __init__(
        self,
        base_url: str,
        z_threshold:       float = 3.0,
        flap_threshold:    int   = 3,
        flap_window_secs:  int   = 300,
    ):
        self.base_url   = base_url
        self.freq_det   = PrefixAnomalyDetector(z_threshold=z_threshold)
        self.flap_det   = SessionFlapDetector(threshold=flap_threshold, window_seconds=flap_window_secs)
        self.leak_det   = RouteLeakDetector()
        self._handlers: list[AlertHandler] = []

    def add_handler(self, fn: AlertHandler) -> None:
        self._handlers.append(fn)

    async def _dispatch(self, alert: AnomalyAlert) -> None:
        for handler in self._handlers:
            try:
                await handler(alert)
            except Exception as e:
                print(f"Handler error: {e}")

    async def run(self) -> None:
        async for event in event_stream(self.base_url):
            if event.kind == "route_change":
                d = event.data
                alerts = self.freq_det.record(
                    prefix=d.get("prefix", ""),
                    action=d.get("action", ""),
                    origin_as=int(d["as_path"].split()[-1]) if d.get("as_path") else None,
                    speaker=event.speaker,
                    peer=d.get("peer_addr", ""),
                )
                for alert in alerts:
                    await self._dispatch(alert)

                leak = self.leak_det.check(
                    prefix=d.get("prefix", ""),
                    as_path=d.get("as_path"),
                    otc_asn=None,   # RV2: wire OTC from RouteChange when available
                    speaker=event.speaker,
                    peer=d.get("peer_addr", ""),
                )
                if leak:
                    await self._dispatch(leak)

            elif event.kind == "peer_down":
                d   = event.data
                peer = d.get("peer_addr", "")
                flap_alert = self.flap_det.record_peer_down(event.speaker, peer)
                if flap_alert:
                    await self._dispatch(flap_alert)
```

---

## Part 6 — File Change Index (RV2)

### New Rust files

| File | Epic |
|------|------|
| `crates/rbmp-core/src/bgp/bgpls.rs` | RV2-2 |
| `crates/rbmp-enrichment/Cargo.toml` | RV2-3 |
| `crates/rbmp-enrichment/src/lib.rs` | RV2-3 |
| `crates/rbmp-enrichment/src/rpki.rs` | RV2-3 |
| `crates/rbmp-enrichment/src/peeringdb.rs` | RV2-4 |
| `crates/rbmp-enrichment/src/registry.rs` | RV2-8 |
| `crates/rbmp-store/src/coordinator.rs` | RV2-7 |
| `crates/rbmp-server/src/supervisor.rs` | RV2-6 |
| `crates/rbmp-server/src/registry.rs` | RV2-8 |
| `crates/rbmp-core/src/collector_protocol.rs` | RV2-11 |

### Modified Rust files

| File | Change | Epic |
|------|--------|------|
| `crates/rbmp-core/src/bgp/nlri.rs` | Add `decode_nlri_add_path()` | RV2-1 |
| `crates/rbmp-core/src/bgp/update.rs` | Add `add_path_active` param | RV2-1 |
| `crates/rbmp-core/src/bgp/types.rs` | `BgpUpdate.announced_path_ids`, ExtCommunity full decode | RV2-1 |
| `crates/rbmp-core/src/bgp/mod.rs` | Add `pub mod bgpls` | RV2-2 |
| `crates/rbmp-rib/src/session.rs` | Add `LlgrState`, `on_llgr_peer_down()` | RV2-9 |
| `crates/rbmp-rib/src/table.rs` | Add `llgr_stale` to `RibEntry` | RV2-9 |
| `crates/rbmp-rib/src/manager.rs` | Path_id wiring, synthetic Termination on disconnect, LLGR | RV2-1,6,9 |
| `crates/rbmp-store/src/schema.rs` | Add BGP-LS tables, `rpki_validity` to route_events, `collector_id`/`site` columns | RV2-2,3,11 |
| `crates/rbmp-store/src/writer.rs` | EVPN withdraw, RPKI annotation, BGP-LS writes, batch via coordinator | RV2-1,3,7 |
| `crates/rbmp-server/src/config.rs` | `RpkiConfig`, `StoreConfig` batch fields, `[[speakers]]` section | RV2-3,7,8 |
| `crates/rbmp-server/src/main.rs` | Wire enrichment, registry, coordinator, supervisor | RV2-3,6,7,8 |
| `crates/rbmp-server/src/receiver.rs` | Synthetic Termination on disconnect | RV2-6 |
| `crates/rbmp-server/src/api/health.rs` | Proper Prometheus metrics | RV2-10 |
| `Cargo.toml` | Add `rbmp-enrichment` to workspace | RV2-3 |

### New Python files

| File | Epic |
|------|------|
| `bmppy/rbmppy/rpki.py` | RV2-3 |
| `bmppy/rbmppy/internet.py` | RV2-4 |
| `bmppy/rbmppy/detectors.py` | RV2-5 |

### Modified Python files

| File | Change | Epic |
|------|--------|------|
| `bmppy/rbmppy/analytics.py` | Full rewrite — Z-score, hijack, leak, session flap, DuckDB analytics | RV2-5 |
| `bmppy/rbmppy/__init__.py` | Export new modules | RV2-3,4,5 |
| `bmppy/pyproject.toml` | Add `ipaddress` stdlib note, no new deps needed for rpki.py | RV2-3 |

---

## Part 7 — Quality Gates for RV2

```bash
# All existing RV1 tests must still pass:
cargo test --workspace

# New tests required per epic:
# RV2-1: test Add-Path NLRI with path_id extraction
# RV2-1: test EVPN withdraw writer
# RV2-1: test all ExtCommunity display variants (RT, SoO, color)
# RV2-2: test BGP-LS Node NLRI (IS-IS + OSPF), Link NLRI, Prefix NLRI
# RV2-3: test VrpCache.validate() — valid/invalid/not-found cases, IPv4 + IPv6
# RV2-3: test RTR PDU parsing for IPv4 (type 4) and IPv6 (type 6) PDUs
# RV2-5 Python: test Z-score fires at |Z| > 3.0; test origin change detection
# RV2-5 Python: test SessionFlapDetector fires at threshold

# Throughput regression gate:
# After RV2-7 (coordinator): cargo bench (placeholder benchmark)
# target: route_event INSERT throughput >= 1500/sec on Ubuntu server
```

---

## Part 8 — Updated Config Reference

```toml
[bmp]
listen_addr              = "0.0.0.0:5000"
max_frame_bytes          = 65535
shed_stats_on_pressure   = true
# archive_path           = "runtime/bmp-archive.jsonl"

[http]
listen_addr   = "0.0.0.0:7878"
serve_ui      = true
cors_origins  = []

[store]
db_path              = "runtime/routes.duckdb"
in_memory            = false
event_capacity       = 16384
checkpoint_secs      = 60
write_batch_size     = 100      # NEW RV2-7
write_flush_interval_ms = 50    # NEW RV2-7

[rpki]                          # NEW RV2-3
# rtr_addr            = "127.0.0.1:3323"   # Routinator RTR port
cloudflare_fallback  = true
sync_interval_secs   = 3600
disabled             = false

[log]
level  = "info"
format = "pretty"

# Optional: known speaker metadata         # NEW RV2-8
[[speakers]]
addr     = "10.0.0.1"
hostname = "xrd-pe1"
vendor   = "Cisco IOS-XR"
role     = "pe"
site     = "Singapore-DC1"
asn      = 65000
```

---

## Part 9 — Notes for RV3

The next session should start with the RV2 diff. RV3 targets:

1. **UI** — Svelte dashboard: live prefix table, AS path graph, RPKI overlay, peer state panels
2. **BGP-LS topology graph** — derive AS topology from BGP-LS Link NLRIs; expose via API
3. **HA** — Two-instance leader election, de-duplicated writes
4. **rbmp-collector binary** — Full collector/core split for multi-site deployments
5. **Alerting webhooks** — POST anomaly alerts to Slack/PagerDuty from rbmppy DetectorPipeline

---

*End of RUSTYBMP_BACKLOG_RV2.md — Sprint RV2*
