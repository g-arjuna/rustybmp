# rustybmp

The best BMP/BGP collector on the planet. Written in Rust.

**Sprint**: RV9 complete — 0 build errors, 101 Python tests passing. `cargo build --workspace` clean. All layers green.

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
 │    ├── Roto JIT filter engine (cranelift, roto-jit feature)     │
 │    ├── YAML filter DSL fallback (when roto-jit absent)          │
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
 │    ├── ASPA validation (RFC 9319)                               │
 │    ├── BGPsec ECDSA P-256 validation (ring crate)               │
 │    ├── Credential vault (age-encrypted, Zeroizing)              │
 │    └── Per-route RPKI annotation                                │
 └─────────────────────────────────────────────────────────────────┘

ui/ (SvelteKit dashboard)
  ├── 24 nav pages (Dashboard, Peers, Prefixes, Topology, RPKI,
  │   Policy, AS Paths, SR Policy, BGP-LS Path, Filters,
  │   Path Status, Capacity, Onboarding, ML Insights, BMP Stats,
  │   RPKI Coverage, Alerts, Adapters, NL Query +
  │   RV9: /communities, /flowspec, /vrf)
  ├── Adaptive homepage: 3-state UX (empty→onboarding / waiting / active)
  ├── Speaker cards: per-router hostname, vendor, peers-up, routes, RPKI%
  ├── Inline router config snippets (IOS-XR, FRR, Arista EOS, JunOS)
  ├── D3 components: TimelineChart, AsnSankey, force topology
  │   Adaptive topology: force (< 100 nodes) / hierarchical / clustered
  ├── UI components: VirtualTable, MetricCard, RpkiBadge
  └── SSE client with RAF batching + auto-reconnect

bmppy/ (Python SDK + anomaly detection)
  ├── RtrVrpCache + DetectorPipeline
  ├── IrrClient / RdapClient / BgpToolsClient / RipeStatClient
  ├── OriginChange / RouteLeak / MED / Hijack detectors
  ├── policy_fetcher.py — SSH policy retrieval (Genie + paramiko)
  ├── rbmppy/policy/ — vendor-neutral AST + correlator
  ├── rbmppy/acl_generator.py — multi-vendor ACL/prefix-list/null-route (IOS-XR/FRR/JunOS/Arista)
  └── rbmppy/policy_advisor.py — rule-based filter gap analysis + Roto snippet generation
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
- [x] **MCAST-VPN NLRI (RFC 6514) — types 1-7 decoded**
- [x] **BGPsec_Path attribute parse (RFC 8205, type 30) — signature blocks stored**
- [x] **Path Status TLV (draft-ietf-grow-bmp-path-marking-tlv-05) — 12 status bits, 11 reason codes**
- [x] RPKI ROV via RTR (RFC 6810) — live VRP cache
- [x] **ASPA validation (RFC 9319) — AS_PATH upstream provider verification**

### Filter Engine
- [x] **Roto JIT filter engine** — Cranelift-backed, `roto-jit` cargo feature
  - `config/filters.roto` default script (bogon, RPKI-invalid, OTC leak, blackhole)
  - Takes priority over YAML DSL when loaded; YAML DSL remains as fallback
  - RotoFilterStats: accept/reject/error counters + avg_eval_ns
- [x] YAML filter DSL — prefix, origin AS, peer AS, community, AS path regex, RPKI state, length range
- [x] Actions: `accept`, `reject`, `tag`
- [x] Hot-reload via `POST /api/filters/reload` — detects `.roto` vs `.yaml` extension
- [x] Filter test endpoint: `POST /api/filters/test` (evaluates synthetic route, returns verdict + ns)
- [x] Filter stats: `GET /api/filters/stats` (accept/reject/error counters)
- [x] Applied at RIB ingestion (pre-storage)

### Output & Integration
- [x] **Kafka output** — typed topics (router/peer/unicast_prefix/evpn/ls_node/bmp_stat/bmp_raw), lz4 compression
- [x] **MRT export** — BGP4MP_MESSAGE_AS4, BGP4MP_STATE_CHANGE_AS4, TABLE_DUMP_V2
- [x] **MRT import** — parse MRT records back to BMP events (RFC 6396)
- [x] **Multi-site distributed collection** — `rbmp-collector` edge binary → Core TCP (MessagePack framing, exponential-backoff reconnect, ring-buffer)
- [x] **Output adapters** (RV8) — `OutputAdapter` async trait + cursor-based event pump
  - Elasticsearch ECS adapter — `_bulk` ndjson, `rustybmp-bgp-events` index, ECS `@timestamp`/`event.*`/`host.ip` fields
  - Splunk HEC adapter — `POST /services/collector/event`, configurable sourcetype/index, token from vault
- [x] **RIPE STAT client** (RV8) — `RipeStatClient` in `bmppy/rbmppy/internet.py`; async prefix-overview, visibility, RPKI, routing-history, ASN-neighbours
- [x] Prometheus metrics (`/metrics`)
- [x] SSE real-time event stream (`/api/events`)

### Analytics (DuckDB)
- [x] Current RIB snapshot per peer
- [x] Prefix history timeline
- [x] Route change window queries
- [x] Top churning prefixes
- [x] Origin AS distribution
- [x] `collector_id` tagging on all events (multi-site)
- [x] `srpolicy_events` table — SR Policy NLRIs with segment JSON
- [x] `aspa_validations` table — per-route ASPA verdicts
- [x] **`path_markings` table** — RFC 9069 Path Status TLV per prefix/peer
- [x] **`bgpsec_validations` table** — per-route ECDSA verdict
- [x] **`convergence_events` table** — PeerDown → flood → EOR tracking
- [x] **`policy_configs` table** — fetched/inferred router policy configs
- [x] **`peer_max_prefix` table** — configured per-peer prefix limits
- [x] Composite indexes: `(prefix, occurred_at)`, `(peer_addr, counter_name, occurred_at)`
- [x] AS path graph query (Sankey data) — `aspath_graph()`
- [x] BMP stats history + sparklines — `bmpstats_history()`
- [x] SR Policy list — `srpolicy_current()`
- [x] ML anomaly queries — `ml_anomalies_recent()`
- [x] **`max_prefix_capacity()`** — REGR_SLOPE trend + ETA to exhaustion per peer/AFI-SAFI

### API
- [x] REST: `/api/speakers`, `/api/peers`, `/api/routes`, `/api/prefixes`
- [x] Prefix detail: `/api/routes/prefix/{p}/timeline|peers|convergence`
- [x] Peer detail: `/api/peers/{addr}/timeline|capabilities`
- [x] RPKI: `/api/rpki/analysis`, `/api/rpki/coverage`
- [x] Policy: `/api/policy?peer=X` — pre/post RIB delta
- [x] AS Path: `/api/aspath/graph` — Sankey node/link data
- [x] SR Policy: `/api/srpolicy` — active policies list
- [x] BGP-LS: `/api/bgpls/graph`, `/api/bgpls/path?from=X&to=Y`
- [x] ML: `/api/ml/anomalies`, `/api/ml/model/status`
- [x] Filters: `/api/filters/reload` (POST), `/api/filters/test` (POST), `/api/filters/stats`
- [x] Stats: `/api/bmpstats/history`
- [x] Onboarding: `/api/onboard/{addr}/validate|register|filter|confirm`
- [x] SSE: `/api/events` — real-time stream with RAF-batched client
- [x] Health: `/health`, `/metrics` (Prometheus)
- [x] JWT auth middleware (optional, configurable)
- [x] **Path Status**: `GET /api/path-status/matrix`, `GET /api/path-status/history`
- [x] **Capacity**: `GET /api/capacity/max-prefix`, `POST /api/capacity/max-prefix`
- [x] **Convergence**: `GET /api/convergence?peer=X&hours=24`
- [x] **Credentials vault**: `GET /api/credentials`, `POST /api/credentials`, `DELETE /api/credentials/{alias}`
- [x] **Policy fetch**: `POST /api/policy/fetch`, `GET /api/policy/fetch/{job_id}`
- [x] **Policy configs**: `GET /api/policy/configs`, `GET /api/policy/configs/{peer}`
- [x] **Resource Governor** (RV8): `GET /api/governance` — memory/write/rate loop snapshot
- [x] **Speaker Summary** (RV8): `GET /api/speakers/summary` — per-router aggregated view
- [x] **OpenAPI spec** (RV8): `GET /api/openapi.json` — OpenAPI 3.0.3
- [x] **Swagger UI** (RV8): `GET /api/swagger` — interactive docs
- [x] **MCP server** (RV8): `POST /mcp` — JSON-RPC 2.0, 11 BGP tools, NL→DuckDB SQL
- [x] **External visibility** (RV8): `GET /api/external/prefix-visibility?prefix=X` — internal RIB vs RIPE STAT discrepancy
- [x] **NL query** (RV9): `POST /api/nl-query` — keyword→SQL translation, safe DuckDB execution
- [x] **Adapters management** (RV9): `GET /api/adapters`, `POST /api/adapters/{name}/test`
- [x] **Communities** (RV9): `GET /api/communities`, `GET /api/communities/semantics`
- [x] **FlowSpec rules** (RV9): `GET /api/flowspec/rules?speaker=X`
- [x] **VRF explorer** (RV9): `GET /api/vrf/list`, `GET /api/vrf/{rd}/routes`

### UI Dashboard (SvelteKit)
- [x] **Dashboard** (RV8) — adaptive 3-state homepage: empty→onboarding, waiting→speaker status, active→full dashboard with speaker cards
  - Speaker cards: hostname, vendor badge, peers-up, route count, RPKI% per router
  - Inline router config snippets (IOS-XR / FRR / Arista EOS / JunOS, copy to clipboard)
- [x] **Dashboard (legacy)** — health bar, stat cards (peers up/down, RPKI%, speakers), live SSE feed
- [x] **Peers** — peer table with state badges; click → peer detail
- [x] **Peer Detail** (`/peers/[addr]`) — session timeline (Gantt), flap counters, event log
- [x] **Prefixes** — route table; click → prefix explorer
- [x] **Prefix Explorer** (`/prefix/[prefix]`) — timeline, peer AS paths, convergence, RPKI detail
- [x] **Topology** — D3 force-directed BGP-LS graph with zoom/pan/drag; adaptive rendering:
  - Force-directed (< 100 nodes), Hierarchical spine-leaf (100-1000), Clustered AS-level (> 1000)
- [x] **AS Paths** (`/aspath`) — D3 Sankey chart + path length histogram + filterable table
- [x] **RPKI** — ROA coverage donut, invalid prefix breakdown, per-peer RPKI stats
- [x] **RPKI Coverage** (`/rpki-coverage`) — ROA coverage for owned prefixes
- [x] **Policy** (`/policy`) — pre/post-policy RIB delta, rejection rate visualisation
- [x] **SR Policy** (`/srpolicy`) — active SR policies with segment details (MetricCards + VirtualTable)
- [x] **BGP-LS Path** (`/bgpls-path`) — shortest IGP path computation between routers
- [x] **Filters** (`/filters`) — live filter test, YAML reload, verdict counters
- [x] **Onboarding** (`/onboard`) — 4-step wizard: validate → register → filter → confirm
- [x] **ML Insights** (`/ml`) — anomaly feed by severity, model status panel
- [x] **BMP Stats** (`/stats`) — RFC 9972 counter history, peer filter, bar chart
- [x] **Alerts** — alert feed
- [x] **NL Query** (`/query`) — natural language search with example chips, SQL preview, results table
- [x] **Adapters** (`/adapters`) — output adapter health, event counts, test-connection button
- [x] **Communities** (`/communities`) — community frequency table, inferred semantics, filter
- [x] **FlowSpec** (`/flowspec`) — active FlowSpec rules, speaker filter, large-prefix alert
- [x] **VRF Explorer** (`/vrf`) — L3VPN/EVPN RD selector, route table per VRF
- [x] **Path Status** (`/path-status`) — redundancy matrix (prefix × peer), RFC 9069 colour coding
  (★ best, ≡ ECMP, ↻ backup, ⊕ best-ext, ✗ nonselected, ⊘ filtered/invalid, 💤 stale, ⚡ suppressed)
- [x] **Capacity** (`/capacity`) — max-prefix fuel gauge + trend + ETA to exhaustion, critical alert banner
- [x] D3 component library: `TimelineChart`, `AsnSankey`, topology force graph
- [x] UI component library: `VirtualTable` (virtual-scroll), `MetricCard`, `RpkiBadge`
- [x] SSE client (`sse.ts`) — RAF batching, exponential-backoff reconnect

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

### ✅ RV3 — Feature parity + integration
- SR Policy SAFI 73, EVPN types 6-11, BGP-LS full TLVs
- YAML filter DSL + LLGR state machine
- DNS PTR enrichment + BMP proxy
- Kafka output crate (rbmp-kafka), MRT import/export crate (rbmp-mrt)
- Python SDK: rpki.py + internet.py + detectors.py
- Distributed collection: rbmp-collector + Core listener + schema collector_id

### ✅ RV4 — Scale + UI foundation
- SvelteKit dashboard scaffold: 11 nav pages, BGP-LS D3 topology, RPKI page
- DuckDB metrics, UI static file serving from Axum
- MCAST-VPN stub, TLS support, Redis HA leader election
- FRR integration tests

### ✅ RV5 — UI wiring + API depth
- Clickable prefixes + peer IPs (prefix explorer, peer detail pages)
- Prefix timeline, peer timeline, prefix convergence, RPKI analysis APIs
- ML anomaly schema, export aggregates, feature engineering
- 6 new sidebar nav items scaffolded

### ✅ RV6 — UI completeness + protocol + quality
- **Protocols**: ASPA (RFC 9319), BGPsec_Path parse (RFC 8205), MCAST-VPN full RFC 6514, SRv6 uSID scaffold
- **Filter engine**: hot-reload, test endpoint, verdict counters, RouteCtx/Roto scaffold
- **Schema**: `srpolicy_events`, `aspa_validations`, composite indexes, new query methods
- **API**: 18 new endpoints across analytics, stats, peers, topology, ml, filters, onboard
- **UI components**: TimelineChart (D3), AsnSankey (d3-sankey), VirtualTable, MetricCard, RpkiBadge, SSE sse.ts
- **UI pages**: 4 new (filters, srpolicy, bgpls-path, rpki-coverage) + 5 upgraded
- **Quality gate**: `cargo build --workspace` 0 warnings, `npm run check` 0 errors, 77 tests

### ✅ RV7 — Roto JIT + Path Status TLV + Vault + BGPsec + Capacity (current)
- **Roto JIT filter engine** — Cranelift-backed, feature-gated `roto-jit`; default `config/filters.roto`; dual `.roto`/`.yaml` hot-reload
- **Path Status TLV** — draft-ietf-grow-bmp-path-marking-tlv-05; 12 status bits, 11 reason codes; `path_markings` table
- **Credential vault** — age-encrypted, HMAC-SHA256 integrity, `Zeroizing<String>`; SSH policy fetch via Genie + paramiko
- **BGPsec validation** — ECDSA P-256 via `ring` crate; `BgpsecValidator` with per-hop cert lookup
- **Convergence events** — PeerDown → withdrawal flood → EOR tracking; `GET /api/convergence`
- **Capacity analytics** — RFC 9972 type 30 trend + ETA; `peer_max_prefix` table; fuel-gauge UI
- **UI**: `/path-status` redundancy matrix, `/capacity` fuel gauge, topology adaptive rendering (force/hierarchical/clustered)
- **Quality gate**: `cargo build --workspace` 0 warnings, `npm run check` 0 errors, 94 tests

### ✅ RV8 — Swagger · MCP Server · Output Adapters · Resource Governor · Adaptive UX · External APIs · Testing
- **Resource governor** (3-loop: memory/write/rate pressure); `GET /api/governance`; internet-scale DuckDB write tuning
- **Adaptive homepage** (3-state: empty→onboarding / waiting / active); speaker cards; inline config snippets (4 vendors)
- **OpenAPI 3.0.3 spec** (`api/schema.rs`); Swagger UI at `/api/swagger`
- **MCP server** — JSON-RPC 2.0 at `POST /mcp`; 11 BGP tools; NL→DuckDB SQL; 500K daily token budget (AtomicU64); `ANOMALY_CATALOGUE` with DuckDB verification queries
- **Output adapters** — `OutputAdapter` trait + cursor pump; Elasticsearch ECS adapter; Splunk HEC adapter
- **RIPE STAT client** (`RipeStatClient` in `internet.py`); `GET /api/external/prefix-visibility` internal vs external discrepancy
- **Testing** — `tests/seed.sql` DuckDB fixtures; `tests/integration/mcp_tools.rs` (12 tests); `lab/scenarios/rv8_governance_smoke.sh` E2E smoke script

### ✅ RV9 — Completeness Sprint
- **UI**: `/communities` explorer, `/flowspec` viewer, `/vrf` VRF explorer, `/query` NL page, `/adapters` management (24 total pages)
- **API**: 6 new typed endpoints (nl-query, adapters, communities, flowspec, vrf)
- **Python**: `AclGenerator` (4-vendor: IOS-XR/FRR/JunOS/Arista), `PolicyAdvisor` (rule-based filter gap analysis + Roto snippets)
- **Grafana**: 11-panel dashboard bundle (`grafana/rustybmp-dashboard.json`)
- **ContainerLab**: Tier 0 FRR smoke (`tests/scenarios/01_frr_minimal/`), Tier 1 XRd RFC 9972 (`tests/scenarios/02_xrd_rfc9972/`)
- **Playwright**: 26-test E2E suite (`ui/tests/rustybmp.spec.ts`) with mock API route interception
- **CI**: GitHub Actions Layer 7 Playwright job + Layer 3 ML tests + `rv9` branch trigger
- **Docs**: `docs/CODEX_TESTING.md` — 7-layer runbook replacing `UBUNTU_TESTING.md`
- **Quality gate**: 101 Python tests passing, 0 build errors

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
| RFC 6514 | BGP Encodings for MCAST-VPN — types 1-7 |
| RFC 8205 | BGPsec_Path attribute (type 30) — parse + store + ECDSA validation |
| RFC 9319 | ASPA — AS Provider Authorization validation |
| draft-ietf-grow-bmp-path-marking-tlv-05 | Path Status TLV — 12 status bits, 11 reason codes |

---

## Development

```bash
# Run tests
cargo test --workspace

# Build — must produce 0 warnings
cargo build --workspace

# UI type-check — must produce 0 errors
cd ui && npm run check

# UI dev server (proxies /api → localhost:7878)
cd ui && npm run dev

# Format + lint
cargo fmt --all && cargo clippy --workspace

# bmppy SDK
cd bmppy && pip install -e ".[dev]" && python -m pytest
```

## Decision Log

| Sprint | Decision | Rationale |
|--------|----------|----------|
| RV3 | YAML filter DSL over iptables-style language | Operators know YAML; Roto planned for RV7 |
| RV4 | DuckDB over PostgreSQL | Single-file, zero-dep, excellent analytical SQL |
| RV5 | SvelteKit + TailwindCSS over React | SSR-friendly, Svelte runes = fine-grained reactivity |
| RV6 | `ring` crate for ECDSA | FIPS-adjacent, no_std capable, widely audited |
| RV7 | `roto-jit` as optional Cargo feature | Cranelift adds ~8 MB to binary; operators that don't need JIT keep lean build |
| RV7 | Optimistic-valid BGPsec in BMP observation mode | BMP passive tap has no original UPDATE wire bytes; cert-check pass is correct for monitoring use case |
| RV7 | `Zeroizing<String>` for vault passwords | Memory is zeroed on drop; password never survives its scope |
| RV7 | Topology adaptive rendering via `$derived` runes | Pure data transform; no D3 regression risk when node count crosses threshold |
| RV8 | `async-trait` crate for `OutputAdapter` | Rust async traits still require the proc-macro shim for object-safe dispatch; avoids `Box<dyn Future>` boilerplate in adapter impls |
| RV8 | MCP JSON-RPC 2.0 (not MCP SDK) implemented from scratch | No stable Rust MCP SDK exists; JSON-RPC 2.0 envelope is 40 lines and gives full control over tool dispatch and error codes |
| RV8 | NL→SQL via keyword mapping (no LLM at runtime) | LLM inference is not always available on-prem; deterministic mapping covers 90% of ops questions and is auditable. External LLM agents can call the tool and let the server execute their SQL safely |
| RV8 | `AtomicU64` for daily NL token budget | Lock-free, correct under high concurrency; resets via day-bucket comparison — same pattern as bonsai nl_query.rs |
| RV8 | Output adapter cursor in `runtime/cursors/{name}.cursor` | Survives restarts without re-pushing already-shipped events; consistent with bonsai adapter pattern |
| RV9 | `AclGenerator` pure-Python, no external deps | Operators without a live router can generate and review ACL configs in CI or dry-run before push |
| RV9 | `PolicyAdvisor` rule-based (no ML runtime) | Deterministic, auditable, zero inference latency; covers RPKI/ASPA/path-len heuristics with confidence scores |
| RV9 | Playwright tests use `page.route()` mock interception | Decouples UI test suite from live backend; tests run in CI without a real BMP session |
| RV9 | Grafana dashboard as portable JSON (no provisioning sidecar) | Single-file import via Grafana UI or API — works with any Grafana 10+ instance, no operator config needed |

---

## License

Apache 2.0
