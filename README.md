# rustybmp

The best BMP/BGP collector on the planet. Written in Rust.

---

## What it is

rustybmp is a dedicated, production-grade **BGP Monitoring Protocol (BMP)** collector and analyser. It receives BMP sessions from routers, parses every BGP message with full RFC correctness, maintains live in-memory RIB tables per peer, persists all route history to DuckDB for powerful analytical queries, and serves a real-time dashboard UI — all from a single statically-linked binary.

**Focus:** BMP + BGP only. No gNMI, no SNMP, no syslog. Maximum depth, zero scope creep.

---

## Architecture

```
Routers (RFC 7854 BMP) ──TCP──▶ BMP Receiver (rbmp-server)
                                       │
                                       ▼
                                rbmp-core parser
                                  ├── BMP message types (RFC 7854)
                                  ├── BGP UPDATE (RFC 4271)
                                  ├── Path attributes (all types)
                                  ├── MP_REACH/UNREACH (RFC 4760)
                                  ├── Communities (RFC 1997, 4360, 8092)
                                  └── BGP OPEN + capabilities
                                       │
                                       ▼
                                rbmp-rib engine
                                  ├── Per-peer RIB tables
                                  ├── Adj-RIB-In/Out pre+post policy
                                  ├── Loc-RIB (RFC 9069)
                                  └── BMP session tracking
                                       │
                              ┌────────┴────────┐
                              ▼                  ▼
                        rbmp-store          RibEvent bus
                        (DuckDB)           (broadcast)
                     route history         │
                     analytics             ▼
                     queries          HTTP API (Axum)
                                       ├── REST /api/*
                                       ├── SSE  /api/events
                                       └── UI   /  (Svelte)
```

### Crates

| Crate | Purpose |
|---|---|
| `rbmp-core` | Pure RFC parser: BMP (RFC 7854) + BGP (RFC 4271 + extensions). No I/O, no_std-friendly |
| `rbmp-rib`  | In-memory RIB engine: per-peer tables, session lifecycle, event emission |
| `rbmp-store` | DuckDB-backed persistence: route history, analytics queries |
| `rbmp-server` | Main binary: BMP TCP receiver, HTTP REST+SSE API, embedded UI |

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

### BGP Protocol
- [x] BGP OPEN with full capability negotiation
- [x] BGP UPDATE: withdrawals + announcements
- [x] All path attributes: ORIGIN, AS_PATH, NEXT_HOP, MED, LOCAL_PREF, ATOMIC_AGGREGATE, AGGREGATOR
- [x] Standard communities (RFC 1997) + well-known (NO_EXPORT, BLACKHOLE, etc.)
- [x] Extended communities (RFC 4360)
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
- [x] Graceful Restart capability (RFC 4724)
- [x] End-of-RIB marker detection (RFC 4724)
- [x] AS path loop / prepending detection
- [x] EVPN (RFC 7432) — all 5 route types (types 1-5)
- [x] BGP Flowspec (RFC 5575/8955) — component parser, numeric + bitmask ops
- [ ] BGP-LS (RFC 7752 via BMP) — planned
- [ ] RPKI ROV via RTR (RFC 6810) — planned

### Analytics (DuckDB)
- [x] Current RIB snapshot per peer
- [x] Prefix history timeline
- [x] Route change window queries
- [x] Top churning prefixes
- [x] Origin AS distribution
- [ ] AS path length distribution
- [ ] Community usage analytics
- [ ] Peer session flap analysis
- [ ] Route visibility across peers
- [ ] BGP convergence timing
- [ ] Route leak / hijack detection

### API
- [x] REST: `/api/speakers`, `/api/peers`, `/api/routes`
- [x] SSE: `/api/events` — real-time stream
- [x] Health: `/health`, `/metrics`
- [ ] Swagger / OpenAPI spec
- [ ] Authentication

### UI (Svelte 5)
- [ ] Dashboard: speakers, peers, route counts, event rate
- [ ] Route table with live filter + sort
- [ ] Peer session view with timeline
- [ ] AS path explorer
- [ ] Community browser
- [ ] Prefix history chart
- [ ] BGP topology map

---

## Quick Start

```bash
# Build
cargo build --release

# Run with defaults (BMP on :5000, HTTP on :7878)
./target/release/rustybmp

# Run with config
cp config/rustybmp.toml.example rustybmp.toml
./target/release/rustybmp rustybmp.toml

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

Once connected, check the dashboard at `http://localhost:7878`.

---

## bmppy (Python analytics)

For advanced analytics, the `bmppy/` package connects directly to the DuckDB file:

```python
from rbmppy.analytics import RouteAnalytics

r = RouteAnalytics("runtime/routes.duckdb")
print(r.churn_analysis(top_n=20))
print(r.community_usage())
r.close()
```

---

## Roadmap

### Phase 1 — Core (current)
- [x] BMP receiver + full parser
- [x] RIB engine
- [x] DuckDB persistence
- [x] REST + SSE API

### Phase 2 — UI
- [ ] Svelte 5 dashboard embedded in binary
- [ ] Real-time route table
- [ ] Peer session timeline

### Phase 3 — Analytics
- [ ] RPKI validation (RTR client)
- [ ] Route leak detection
- [ ] BGP hijack alerts
- [x] EVPN support (RFC 7432)
- [x] Flowspec support (RFC 5575/8955)

### Phase 4 — Scale
- [ ] Multi-instance clustering
- [ ] Prometheus metrics
- [ ] Alert rules engine
- [ ] Kafka / NATS export

---

## RFCs Implemented

| RFC | Title |
|---|---|
| RFC 7854 | BGP Monitoring Protocol (BMP) |
| RFC 8671 | Support for Adj-RIB-Out in BMP |
| RFC 9069 | Support for Local RIB in BMP |
| RFC 4271 | A Border Gateway Protocol 4 (BGP-4) |
| RFC 4760 | Multiprotocol Extensions for BGP-4 |
| RFC 1997 | BGP Communities Attribute |
| RFC 4360 | BGP Extended Communities Attribute |
| RFC 8092 | BGP Large Communities Attribute |
| RFC 6793 | BGP Support for Four-Octet ASN |
| RFC 5492 | Capabilities Advertisement with BGP-4 |
| RFC 4724 | Graceful Restart Mechanism for BGP |
| RFC 7911 | Advertisement of Multiple Paths in BGP |
| RFC 3107 | Carrying Label Information in BGP-4 |
| RFC 4364 | BGP/MPLS IP Virtual Private Networks |
| RFC 9972 | Advanced BMP Statistics (May 2026) — types 18-38 |
| RFC 7432 | BGP MPLS-Based Ethernet VPN (EVPN) |
| RFC 5575 | Dissemination of Flow Specification Rules (Flowspec) |
| RFC 8955 | Dissemination of Flow Specification Rules for IPv6 |
| RFC 8669 | BGP Prefix Segment Identifiers (Prefix-SID) |
| RFC 9234 | Route Leak Prevention and Detection (OTC + BGP Role) |
| RFC 9012 | BGP Tunnel Encapsulation Attribute |

---

## Development

```bash
# Run tests
cargo test

# Check all crates
cargo check --workspace

# Format + lint
cargo fmt --all && cargo clippy --workspace

# bmppy analytics
cd bmppy && python -m pytest
```

## License

Apache 2.0
