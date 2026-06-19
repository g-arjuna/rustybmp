# rustybmp

The best BMP/BGP collector on the planet. Written in Rust.

**Sprint**: RV3 complete — 49 tests, 0 failures.

---

## What it is

rustybmp is a dedicated, production-grade **BGP Monitoring Protocol (BMP)** collector and analyser. It receives BMP sessions from routers, parses every BGP message with full RFC correctness, maintains live in-memory RIB tables per peer, persists all route history to DuckDB for powerful analytical queries, and serves a real-time HTTP API and SSE event stream — all from a single statically-linked binary.

In distributed deployments, lightweight `rbmp-collector` edge processes forward raw BMP PDUs to a central Core over an authenticated, length-framed MessagePack protocol — enabling multi-site collection without running a full Kafka stack.

**Focus:** BMP + BGP only. No gNMI, no SNMP, no syslog. Maximum depth, zero scope creep.

---

## Architecture

```
Routers (RFC 7854 BMP)                     rbmp-collector (edge, optional)
        │                                          │ CollectorEnvelope
        │ TCP :5000                                │ TCP :5001
        ▼                                          ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │                      rbmp-server (Core)                         │
 │                                                                 │
 │  BMP Receiver  ◄──────────────────────────────────────────────  │
 │       │                                                         │
 │       ▼                                                         │
 │  rbmp-core parser                                               │
 │    ├── BMP RFC 7854 + RFC 8671 + RFC 9069                       │
 │    ├── BGP UPDATE + all path attributes                         │
 │    ├── EVPN types 1-11 (RFC 7432 + RFC 8365 + RFC 9251)        │
 │    ├── BGP-LS NLRI + full link/node/prefix attrs (RFC 7752)     │
 │    ├── SR Policy NLRI SAFI 73 (RFC 9256)                        │
 │    ├── Route Target Constraint SAFI 132 (RFC 4684)              │
 │    └── Flowspec (RFC 5575/8955)                                 │
 │       │                                                         │
 │       ▼                                                         │
 │  rbmp-rib engine                                                │
 │    ├── Per-peer RIB + LLGR state machine (RFC 9494)             │
 │    ├── YAML filter DSL (prefix, AS, community, length)          │
 │    └── BMP session lifecycle                                    │
 │       │                                                         │
 │       ├──────────────┬────────────────┬────────────────┐        │
 │       ▼              ▼                ▼                ▼        │
 │  rbmp-store    rbmp-kafka       rbmp-mrt          RibEvent bus  │
 │  (DuckDB)      (Kafka output)   (MRT R/W)         (broadcast)  │
 │  route history  typed topics    RFC 6396           │            │
 │  analytics      lz4 compressed  BGP4MP+TABLE_DUMP  ▼            │
 │  queries        fire-and-forget import/export   HTTP API (Axum) │
 │                                                   ├── REST /api │
 │                                                   ├── SSE /api/events
 │                                                   └── /metrics  │
 │                                                                 │
 │  rbmp-enrichment                                                │
 │    ├── RTR client → VRP cache (RFC 6810)                        │
 │    └── Per-route RPKI validation                                │
 └─────────────────────────────────────────────────────────────────┘

bmppy/ (Python SDK)
  ├── RtrVrpCache + DetectorPipeline
  ├── IrrClient / RdapClient / BgpToolsClient
  └── OriginChange / RouteLeak / MED / Hijack detectors
```

### Crates

| Crate | Purpose |
|---|---|
| `rbmp-core` | Pure RFC parser: BMP + BGP (no I/O). Also hosts `collector_protocol` framing |
| `rbmp-rib` | In-memory RIB engine: per-peer tables, LLGR, filter DSL, event emission |
| `rbmp-store` | DuckDB-backed persistence: route history, analytics queries, `collector_id` tagging |
| `rbmp-server` | Main binary (`rustybmp`) + edge binary (`rbmp-collector`) |
| `rbmp-enrichment` | RTR client, VRP cache, per-route RPKI enrichment |
| `rbmp-kafka` | Kafka output sink: `rdkafka` FutureProducer, typed topics, lz4 compression |
| `rbmp-mrt` | MRT import/export: RFC 6396 BGP4MP + TABLE_DUMP_V2 reader/writer |

---

## Features

### BMP Protocol (RFC 7854)
- [x] Route Monitoring (BGP UPDATE encapsulation)
- [x] Statistics Report — 38 counter types (RFC 7854 + RFC 9972)
- [x] Peer Up / Peer Down with BGP OPEN capture
- [x] Initiation / Termination messages
- [x] Route Mirroring (RFC 8671)
- [x] Adj-RIB-Out (RFC 8671)
- [x] Loc-RIB (RFC 9069)
- [x] Multi-session: concurrent BMP speakers
- [x] DNS PTR enrichment on speaker connect (RFC 1035)
- [x] BMP proxy / transparent intercept mode

### BGP Protocol
- [x] BGP OPEN with full capability negotiation
- [x] BGP UPDATE: withdrawals + announcements
- [x] All path attributes: ORIGIN, AS_PATH, NEXT_HOP, MED, LOCAL_PREF, ATOMIC_AGGREGATE, AGGREGATOR
- [x] Standard communities (RFC 1997) + well-known
- [x] Extended communities (RFC 4360) + Route Target
- [x] Large communities (RFC 8092)
- [x] MP_REACH_NLRI / MP_UNREACH_NLRI (RFC 4760)
- [x] IPv4 + IPv6 unicast NLRI
- [x] MPLS labeled unicast (RFC 3107 / RFC 8277)
- [x] L3VPN prefixes (RFC 4364) with Route Distinguisher
- [x] 4-byte ASN (RFC 6793)
- [x] Add-Path capability + RIB (RFC 7911)
- [x] BGP Prefix-SID (RFC 8669, type 40) — Label Index, Originator SRGB, SRv6 L3 Service
- [x] Only-to-Customer / BGP Role (RFC 9234, type 35 + cap 9)
- [x] Tunnel Encapsulation (RFC 9012, type 23)
- [x] Graceful Restart capability (RFC 4724) + End-of-RIB
- [x] Long-Lived Graceful Restart state machine (RFC 9494)
- [x] **EVPN (RFC 7432 + RFC 8365 + RFC 9251/9572) — all 11 route types (types 1-11)**
- [x] BGP Flowspec (RFC 5575/8955) — component parser, numeric + bitmask ops
- [x] **BGP-LS (RFC 7752) — NLRI + full link/node/prefix attribute TLVs**
- [x] **SR Policy NLRI SAFI 73 (RFC 9256) — segment types A-K**
- [x] **Route Target Constraint SAFI 132 (RFC 4684)**
- [x] RPKI ROV via RTR (RFC 6810) — live VRP cache

### Filter DSL (YAML)
- [x] Match on prefix (exact / more-specific / length range)
- [x] Match on origin AS, peer AS, community value
- [x] Actions: `accept`, `reject`, `tag`
- [x] Applied at RIB ingestion (pre-storage)

### Output & Integration
- [x] **Kafka output** — typed topics (router/peer/unicast_prefix/evpn/ls_node/bmp_stat/bmp_raw), lz4 compression
- [x] **MRT export** — BGP4MP_MESSAGE_AS4, BGP4MP_STATE_CHANGE_AS4, TABLE_DUMP_V2
- [x] **MRT import** — parse MRT records back to BMP events (RFC 6396)
- [x] **Multi-site distributed collection** — `rbmp-collector` edge binary → Core TCP (MessagePack framing, exponential-backoff reconnect, ring-buffer)
- [x] Prometheus metrics (`/metrics`)
- [x] SSE real-time event stream (`/api/events`)

### Analytics (DuckDB)
- [x] Current RIB snapshot per peer
- [x] Prefix history timeline
- [x] Route change window queries
- [x] Top churning prefixes
- [x] Origin AS distribution
- [x] `collector_id` tagging on all events (multi-site)

### API
- [x] REST: `/api/speakers`, `/api/peers`, `/api/routes`
- [x] SSE: `/api/events` — real-time stream
- [x] Health: `/health`, `/metrics` (Prometheus)

---

## Quick Start

```bash
# Build all binaries
cargo build --release

# Run Core (BMP on :5000, Collector listener on :5001, HTTP on :7878)
./target/release/rustybmp

# Run with config
cp config/rustybmp.toml.example rustybmp.toml
./target/release/rustybmp rustybmp.toml

# Run edge collector (forwards to Core at 192.168.1.10:5001)
./target/release/rbmp-collector \
    --listen 0.0.0.0:5000 \
    --core   192.168.1.10:5001 \
    --id     site-fra01-col1 \
    --site   fra01

# Point your router at the BMP receiver
# IOS-XR:
#   bmp server 1 host <rustybmp-ip> port 5000
#   bmp server 1 description rustybmp
# FRR:
#   bmp targets rustybmp
#     address <rustybmp-ip> port 5000
# Juniper:
#   set routing-options bmp station rustybmp address <ip> port 5000
```

Once connected, events are available at `http://localhost:7878/api/events`.

---

## bmppy (Python SDK + anomaly detection)

```bash
cd bmppy && pip install -e ".[dev]"
```

```python
from rbmppy import RustybmpClient, DetectorPipeline, RtrVrpCache, poll_rtr_cache
from rbmppy import stream_route_events

# RPKI-aware anomaly detection pipeline
vrp = RtrVrpCache()
await vrp.load_from_url("http://routinator:9556/api/v1/vrps")

pipeline = DetectorPipeline(vrp_cache=vrp)
async for event in stream_route_events(client):
    for alert in pipeline.process(event):
        print(alert)   # [CRITICAL] hijack: 1.2.3.0/24 — RPKI: INVALID

# IRR / PeeringDB / BGP.Tools lookups
from rbmppy import resolve_origin
info = await resolve_origin("203.0.113.0/24", 64496)
print(info.asn_info.name, info.visible_peers)
```

---

## Roadmap

### ✅ RV1 — Core
- BMP receiver, full parser, RIB engine, DuckDB persistence, REST + SSE API

### ✅ RV2 — Protocol depth
- Add-Path NLRI, EVPN withdraw, ExtComm, BGP-LS NLRI, RPKI RTR scaffold

### ✅ RV3 — Feature parity + integration (current)
- SR Policy SAFI 73 (RFC 9256), EVPN types 6-11, BGP-LS full TLVs
- YAML filter DSL + LLGR state machine
- DNS PTR enrichment + BMP proxy
- Kafka output crate (rbmp-kafka)
- MRT import/export crate (rbmp-mrt)
- Python SDK: rpki.py + internet.py + detectors.py
- Distributed collection: rbmp-collector + Core listener + schema collector_id

### 🔲 RV4 — Scale + UI
- Svelte 5 dashboard (live RIB table, BGP-LS topology, RPKI status)
- Active BGP session connector (receive tables without BMP)
- BGPsec path validation
- HA leader election (active/passive)
- NATS output (edge-friendly Kafka alternative)
- L2VPN VPLS full decode
- BGP-LS SRv6 SID NLRI (SAFI 72)

---

## RFCs Implemented

| RFC | Title |
|---|---|
| RFC 7854 | BGP Monitoring Protocol (BMP) |
| RFC 8671 | Support for Adj-RIB-Out in BMP |
| RFC 9069 | Support for Local RIB in BMP |
| RFC 9972 | Advanced BMP Statistics — types 18-38 |
| RFC 4271 | A Border Gateway Protocol 4 (BGP-4) |
| RFC 4760 | Multiprotocol Extensions for BGP-4 |
| RFC 1997 | BGP Communities Attribute |
| RFC 4360 | BGP Extended Communities Attribute |
| RFC 8092 | BGP Large Communities Attribute |
| RFC 6793 | BGP Support for Four-Octet ASN |
| RFC 5492 | Capabilities Advertisement with BGP-4 |
| RFC 4724 | Graceful Restart Mechanism for BGP |
| RFC 9494 | Long-Lived Graceful Restart (LLGR) |
| RFC 7911 | Advertisement of Multiple Paths in BGP |
| RFC 3107 | Carrying Label Information in BGP-4 |
| RFC 4364 | BGP/MPLS IP Virtual Private Networks |
| RFC 7432 | BGP MPLS-Based Ethernet VPN (EVPN) — types 1-11 |
| RFC 8365 | EVPN Multicast (types 6-8) |
| RFC 9251 | EVPN I-PMSI / S-PMSI (types 9-10) |
| RFC 9572 | EVPN Leaf A-D route (type 11) |
| RFC 5575 | Dissemination of Flow Specification Rules (Flowspec) |
| RFC 8955 | Dissemination of Flow Specification Rules for IPv6 |
| RFC 8669 | BGP Prefix Segment Identifiers (Prefix-SID) |
| RFC 9234 | Route Leak Prevention and Detection (OTC + BGP Role) |
| RFC 9012 | BGP Tunnel Encapsulation Attribute |
| RFC 7752 | BGP-LS — NLRI + link/node/prefix attribute TLVs |
| RFC 9256 | Segment Routing Policy (SR Policy SAFI 73) |
| RFC 4684 | Route Target Constraint SAFI 132 |
| RFC 6810 | RPKI/RTR Protocol — VRP cache + ROV |
| RFC 6396 | MRT Routing Information Export Format |

---

## Development

```bash
# Run tests (49 total)
cargo test --workspace

# Check all crates
cargo check --workspace

# Format + lint
cargo fmt --all && cargo clippy --workspace

# bmppy SDK
cd bmppy && pip install -e ".[dev]" && python -m pytest
```

## License

Apache 2.0
