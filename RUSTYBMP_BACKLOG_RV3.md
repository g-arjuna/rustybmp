# RustyBMP — Sprint RV3 Backlog
## Competitive Analysis · SR Policy · EVPN Complete · Kafka Output · Programmable Filters · BGP-LS Attributes

> **Version**: RV3  
> **Date**: 2026-06-19  
> **Basis**: Full diff analysis of `rv2_all_changes.patch` + extensive research into OpenBMP (SNAS/openbmp), Rotonda (NLnet Labs v0.6.0), Cloudflare bbmp2kafka, goBMP (sbezverk). Gap analysis per project. Reality check: what we do better, what we're missing.

---

## Part 1 — RV2 Completion Audit

### ✅ Fully Complete

| Epic | Evidence in diff | Notes |
|------|-----------------|-------|
| RV2-1: Add-Path NLRI | `decode_nlri_with_path_id()`, `parse_bgp_update(add_path_ipv4: bool)`, `BgpUpdate.announced_path_ids` | Path IDs flow from NLRI through to RibEntry |
| RV2-1: EVPN withdraw | `writer.rs` handles `evpn_unreach` | Symmetric with announce |
| RV2-1: ExtComm decode | `ExtendedCommunity.kind()` added; RT/SoO/SR-TE color/VXLAN | Full display implementation |
| RV2-2: BGP-LS NLRI | `bgpls.rs` new file; IS-IS/OSPF node/link/prefix sub-TLVs | `decode_bgpls_nlri()` + typed structs |
| RV2-3: RPKI config | `RpkiConfig { enabled, rtr_addr }` in `config.rs`; `rbmp-enrichment` crate (`VrpCache`, `EnrichmentEngine`, `RtrClient`) | RTR client scaffold present |
| RV2-5: Analytics rewrite | `analytics.py`: `ZScoreMonitor` (eq.2-4), `HijackDetector`, `RouteLeakDetector`, `FlapScorer`, `RouteAnalytics` | Equations 2-4 from IMACSI 2025 paper |
| RV2-6: Synthetic Termination | Receiver synthesizes `BmpPayload::Termination` on TCP drop when `!cancel.is_cancelled()` | Stale route fix |
| RV2-7: Batched writes | `BATCH_SIZE=500`, `BATCH_TIMEOUT=50ms` in `run_store_writer` — explicit batching with deadline select | Target ≥1500 msg/sec |
| RV2-8: Speaker registry | `SpeakerRegistry { speakers: Vec<SpeakerEntry> }` in config | `lookup(addr)` method |
| RV2-10: Prometheus | Real `PrometheusHandle` from `PrometheusBuilder::install_recorder()`; `/metrics` returns rendered text | Per-message counters in receiver + manager |

### ⚠️ Partially Complete / Deferred

| Item | Status | Action in RV3 |
|------|--------|---------------|
| `bmppy/rbmppy/rpki.py` | NOT in diff — Rust crate exists, Python client missing | RV3-3 |
| `bmppy/rbmppy/internet.py` | NOT in diff | RV3-3 |
| `bmppy/rbmppy/detectors.py` | NOT in diff | RV3-3 |
| RV2-9: LLGR state machine | NOT in diff | RV3-9 |
| RV2-11: Distributed collector/core | NOT in diff | RV3-10 |
| BGP-LS path attributes | NLRI types decoded, but none of the BGP-LS path attribute TLVs (metrics, bandwidth, adjacency SIDs) | RV3-2 |

---

## Part 2 — Competitive Analysis

### 2.1 OpenBMP (SNAS/openbmp)

**Language/Stack**: C++ collector → Apache Kafka → PostgreSQL + TimescaleDB + Grafana  
**Status**: Active, last release 2022 (v2.2.x), widely deployed at IXPs and ISPs  
**Stars**: ~240 | **Forks**: 77

#### What OpenBMP does that rustybmp doesn't

**A. Kafka as the message bus (publish-once/consume-many)**

OpenBMP's core architectural choice is Apache Kafka as the output. Every parsed BMP message goes to a typed Kafka topic:
- `openbmp.parsed.collector` — collector heartbeats
- `openbmp.parsed.router` — BMP speaker up/down
- `openbmp.parsed.peer` — BGP peer up/down with full capability list
- `openbmp.parsed.base_attribute` — path attribute sets (deduplicated by hash)
- `openbmp.parsed.unicast_prefix` — IPv4/IPv6 NLRI with attributes
- `openbmp.parsed.l3vpn` — L3VPN NLRI
- `openbmp.parsed.evpn` — EVPN NLRI
- `openbmp.parsed.ls_node` — BGP-LS nodes
- `openbmp.parsed.ls_link` — BGP-LS links (with ALL link attributes)
- `openbmp.parsed.ls_prefix` — BGP-LS prefixes
- `openbmp.parsed.bmp_stat` — stats reports
- `openbmp.bmp_raw` — raw binary BMP for replay/forwarding

This is the only architecture that allows hundreds of consumers (Grafana, ELK, Apache Spark, custom scripts) to independently consume the same data stream without the collector knowing about them.

**B. Message hashing for correlation**

Every record has a `hash_id` computed from its key fields. This allows correlating:
- `collector_hash_id` → identifies which collector instance
- `router_hash_id` → identifies which router
- `peer_hash_id` → identifies which peer (hash of remote IP + RD + router hash)
- `base_attribute_hash_id` → deduplicates identical path attribute sets (huge storage saving)

**C. TSV wire format with explicit field schema**

OpenBMP transmits tab-separated values with a versioned schema (currently v1.7). This is more compact than JSON and can be consumed by any TSV parser without a Protobuf schema.

**D. openbmp-mrt — MRT export (RFC 6396)**

Produces `BGP4MP_STATE_CHANGE_AS4`, `BGP4MP_MESSAGE`, and `TABLE_DUMP_V2` MRT files. Any compliant MRT parser (bgpdump, libbgpdump, routinator) can read these. Enables offline analysis, RIPE RIS comparison, and time-travel debugging.

**E. BMP Forwarder**

`openbmp-forwarder` consumes from Kafka and re-emits native BMP messages to another collector. Enables:
- Transparent proxy/intercept mode (insert OpenBMP into an existing BMP pipeline)
- Fanout from one router to multiple collectors
- Protocol bridging

**F. DNS PTR name enrichment**

When a BMP speaker connects, OpenBMP performs a PTR DNS lookup on the source IP to populate `router.name`. Same for peers. This is simple but operators deeply need it — IP addresses alone are not operational.

**G. BGP-LS link attributes (the rich part)**

OpenBMP decodes ALL BGP-LS link attributes (RFC 7752 §3.3):
- TE metric, max bandwidth, max reservable bandwidth, unreserved bandwidth (8 priorities)
- TE admin group
- SRLG (Shared Risk Link Group)
- Remote/local IPv4/IPv6 address
- Adjacency SID (RFC 8667)
- Remote IGP router ID
- Local/Remote Node Descriptor ASN
- Peer Node SID (EPE)
- Adjacency segment TLV

OpenBMP also decodes BGP-LS node attributes:
- Node flags (overload, attached, external, ABR)
- Opaque node attributes
- IS-IS area ID
- Prefix metric
- Prefix-SID TLV (separate from BGP Prefix-SID path attribute)
- SR capabilities, SRGB ranges, SR algorithms
- Flex Algorithm definitions

**H. Pre/post policy flags per prefix**

Every unicast prefix row in OpenBMP schema has:
- `isPrePolicy` (field 30): whether this prefix is from pre-policy Adj-RIB-In
- `isAdjRibOut` (field 31): whether this prefix is from Adj-RIB-Out (RFC 8671)

In rustybmp, we track `rib_type` at the event level but don't surface this as per-prefix SQL columns.

**I. PostgreSQL + TimescaleDB**

The `obmp-postgres` consumer stores data in TimescaleDB (PostgreSQL extension for time-series):
- Automatic partition management (no manual DBA work)
- Continuous aggregates for Grafana dashboards
- 100ms from BMP message reception to DB visibility
- Handles full internet routing table dumps (10M+ prefixes in < 1 hour)
- Grafana dashboards pre-built

**J. Heartbeat mechanism**

OpenBMP emits periodic heartbeat messages to `openbmp.parsed.collector` so consumers know the collector is alive. We have `/health` endpoint but no heartbeat event stream.

#### What rustybmp does better than OpenBMP

| Area | RustyBMP advantage |
|------|-------------------|
| Language | Rust: no GC pauses, memory-safe, faster cold start |
| Deployment | Single binary vs multi-container (Kafka + ZooKeeper + DB + UI) |
| Analytical SQL | DuckDB embedded: no separate DB, ad-hoc analytical queries |
| Protocol completeness | RFC 9972 stats types 18-38 (OpenBMP doesn't have these) |
| EVPN | Types 1-5 decoded |
| RPKI | Live RTR client |
| Analytics | Z-score anomaly detection, hijack detection built-in |
| Latency from code | 2026 Rust vs 2022 C++ — modern async runtime |

#### Gap severity assessment

| OpenBMP feature | Severity for rustybmp | RV3 epic |
|----------------|----------------------|----------|
| Kafka output | CRITICAL for scale | RV3-5 |
| MRT export | HIGH for compatibility | RV3-6 |
| BMP forwarder/proxy | HIGH for integration | RV3-7 |
| BGP-LS link attributes | HIGH for topology | RV3-2 |
| Message hashing | MEDIUM (DuckDB UUIDs serve same purpose) | RV3-2 |
| DNS PTR enrichment | MEDIUM | RV3-4 |
| Pre/post policy flags in schema | LOW (already in rib_type) | RV3-2 |
| TimescaleDB | LOW (DuckDB partitioning adequate) | N/A |

---

### 2.2 Rotonda (NLnet Labs)

**Language**: Rust  
**Status**: Active development, v0.6.0 (2026), NLnet Labs professional support  
**Approach**: Composable BGP/BMP engine with programmable filter language (Roto)

#### What Rotonda does uniquely

**A. Roto filter language — the killer feature**

Roto is a compiled filter language that runs in the hot path of the Rotonda pipeline. It is compiled to machine code before execution (no interpreter overhead). Example:

```roto
filter no_bogons(message: BgpMessage) {
    // Reject RFC 1918, documentation, loopback ranges
    if message.prefix_in([10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 
                           192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24]) {
        reject
    }
    accept message
}

filter only_valid_rpki(message: BgpMessage) {
    // RPKI ROV from RTR data source
    if rpki.valid(message.prefix, message.origin_as) {
        accept message
    }
    reject
}
```

Key properties:
- **Statically typed** — no runtime type errors
- **No loops** — bounded execution time, cannot slow the pipeline
- **Composable** — chain filters: `filter1 | filter2 | filter3`
- **Hot path** — runs before routes are stored in RIB, filtering happens at ingestion
- **RTR integration** — RPKI validation available inside filter scripts

rustybmp has NO equivalent. Every route goes into the RIB and DuckDB. There is no way to:
- Reject bogon prefixes at ingest
- Accept only RPKI-valid routes
- Filter by community value
- Apply policy transformations before storage

**B. Active BGP session (not just BMP receiver)**

Rotonda has a `bgp-tcp-in` connector that opens an actual BGP session. This means:
- Can monitor routers that don't support BMP (only BGP)
- Can act as a route collector (similar to a route server but read-only)
- Can receive full table dumps over BGP from route reflectors
- Enables BGP-based Looking Glass functionality

rustybmp is exclusively a BMP receiver.

**C. MRT file connector**

`mrt-file-in` connector loads MRT dump files directly into the RIB:
- RIB dumps (TABLE_DUMP_V2) populate the in-memory table
- Update files replay BGP UPDATE messages
- API endpoint to queue new MRT files for processing
- Enables historical analysis from RIPE RIS, RouteViews archives

This is important for:
- Testing with real internet routing data without a live router
- Historical reconstruction of routing state at specific timestamps
- Comparison of current RIB vs historical baseline

**D. RTR as a pipeline component**

Rotonda's RTR unit is a first-class pipeline component. It connects to Routinator/rpki-client and makes RPKI data available to Roto filter scripts. ROV (Route Origin Validation) happens inside filters, before routes reach the RIB.

```
bmp-tcp-in → rpki-filter (Roto + RTR source) → rib → mqtt-target
                              ↑
                          rtr-unit (RTR protocol → VRP data)
```

**E. MQTT target**

Rotonda can publish events to an MQTT broker. Useful for IoT-style integration, lightweight alerting, and edge monitoring scenarios.

**F. Multi-RIB pipeline**

A single Rotonda instance can have multiple independent RIBs, each with its own filter. Example:
- `rib-pre-policy`: stores all pre-policy routes
- `rib-post-policy`: stores only RPKI-valid routes
- `rib-loc-rib`: stores Loc-RIB from BMP speakers

**G. Ingress ID per source**

Every unique data source (router, peer, MRT file) gets an `ingress_id`. The RIB key is `(prefix, ingress_id)`. This allows comparing routing tables from different sources:
- "What does peer A see vs peer B for prefix P?"
- "What does the Loc-RIB say vs Adj-RIB-In for the same prefix?"

**H. BGPsec (planned)**

Rotonda plans to support BGPsec path validation without recompiling. No other tool in the comparison does this.

**I. Distributed sync (AVRO-based rotoro protocol, planned)**

Multiple Rotonda instances will synchronize/shard data via AVRO — proper distributed state.

#### What rustybmp does better than Rotonda

| Area | RustyBMP advantage |
|------|-------------------|
| Protocol depth | RFC 9972, EVPN types 1-5, Flowspec, BGP-LS |
| Persistence | DuckDB history (Rotonda is in-memory only) |
| Analytics | Z-score, hijack detection, SQL analytics |
| RPKI | Live RTR client in Rust (Rotonda's RTR is read-only for filters) |
| Stats | Full RFC 7854 + RFC 9972 stats decoding |

#### Gap severity assessment

| Rotonda feature | Severity for rustybmp | RV3 epic |
|----------------|----------------------|----------|
| Programmable filter language | HIGH — operators want policy control | RV3-8 |
| MRT file import | HIGH — testing and historical analysis | RV3-6 |
| Active BGP session | MEDIUM — BMP is primary, but useful | Backlog (RV4) |
| MQTT output | LOW — SSE sufficient for now | Backlog (RV4) |
| Multi-RIB pipeline | MEDIUM | RV3-2 |

---

### 2.3 Cloudflare bbmp2kafka

**Language**: Go  
**Status**: Production at Cloudflare scale, 31 commits, Apr 2025 last update  
**Stars**: 28 | **Purpose**: Minimal BMP→Kafka bridge using Protobuf

#### What bbmp2kafka does uniquely

**A. Protobuf serialization**

All messages are serialized as Protobuf (`BBMPMessage` proto definition). Advantages:
- 3-10x more compact than JSON
- Schema-enforced: consumer always knows field types
- Language-neutral: Go, Python, Rust, Java consumers all use same schema
- Fast (de)serialization vs JSON parsing

Consumer code:
```go
bbmpMsg := &bbmp.BBMPMessage{}
err := proto.Unmarshal(data, bbmpMsg)
```

rustybmp outputs only JSON. For high-scale analytics pipelines (Spark, Flink, BigQuery), JSON is a significant overhead.

**B. Token bucket rate limiting per TCP connection**

`tokenBucket.go` — implements a proper token bucket rate limiter at the TCP connection level. When a BMP speaker sends too fast, the token bucket fills and messages are explicitly dropped (with Prometheus counter). This is more principled than our binary `should_shed()` flag.

**C. Focus and simplicity**

bbmp2kafka does ONE thing: receive BMP, serialize to Protobuf, push to Kafka. The entire codebase is 7 Go files. It is designed to be a pure forwarder with no state, no RIB, no analytics. This is by design — analytics happen downstream in Kafka consumers.

**D. Protobuf schema for inter-system communication**

bbmp2kafka's Protobuf schema (`protos/bbmp/`) defines the exact fields for each BMP message type. This is a publishable API contract.

#### What rustybmp does better than bbmp2kafka

Everything except Protobuf and Kafka output. bbmp2kafka is intentionally minimal.

#### Gap severity assessment

| bbmp2kafka feature | Severity for rustybmp | RV3 epic |
|-------------------|----------------------|----------|
| Kafka output | CRITICAL | RV3-5 |
| Protobuf output | MEDIUM | RV3-5 |
| Token bucket rate limiter | LOW (our governor is adequate for now) | RV3-5 |

---

### 2.4 goBMP (sbezverk)

**Language**: Go  
**Status**: Production v1.0.0, 1,469 commits, actively maintained  
**Stars**: 120 | **Deployment**: Docker + Kubernetes  
**Outputs**: Kafka, NATS, file, console, OpenBMP-compatible RAW mode

#### What goBMP does that rustybmp doesn't

**A. SR Policy NLRI — all 11 segment types (RFC 9256/9831)**

This is the biggest protocol gap. goBMP decodes SR Policy NLRI (AFI 1/2, SAFI 73):

SR Policy NLRIs carry segment lists for traffic engineering. Segment types A–K:
- **Type A**: MPLS label with optional S, E, V, L flags
- **Type B**: SRv6 SID with optional SRv6 Endpoint Behavior and SID Structure TLV
- **Type C**: IPv4 prefix with algorithm
- **Type D**: IPv6 prefix with algorithm
- **Type E**: IPv4 prefix with local/remote interface ID
- **Type F**: IPv4 addresses of local/remote interfaces
- **Type G**: IPv6 prefix + interface ID (local)
- **Type H**: IPv6 prefix + interface ID (local + remote)
- **Type I**: Algorithm + IPv4 BGP next-hop
- **Type J**: Algorithm + IPv6 BGP next-hop
- **Type K**: Segment sublist (nested)

This is critical for monitoring SD-WAN, SR-TE, and MPLS-based traffic engineering policies.

**B. SRv6 BGP-LS Extensions**

Beyond Prefix-SID (which we have), goBMP decodes BGP-LS TLVs specific to SRv6:
- SRv6 SID NLRI (SAFI 72 in BGP-LS, AFI=16388)
- SRv6 Endpoint Behavior TLV
- SRv6 BGP Peer Node SID TLV
- SRv6 SID Structure TLV (block length, node length, function length, argument length)

**C. Flex Algorithm in BGP-LS (RFC 9350)**

Flex Algorithm allows routers to compute paths based on custom metrics (latency, TE metric, delay). goBMP decodes:
- Flex Algorithm Definition TLV (ID + metric type + priority)
- Flex Algorithm Prefix Metric TLV
- Flex Algorithm Exclude Admin Group
- Flex Algorithm Include-Any Admin Group
- Flex Algorithm Include-All Admin Group

Without Flex Algorithm decode, we can't monitor modern low-latency BGP routing.

**D. EVPN Route Types 6-11**

goBMP implements ALL 11 EVPN route types. rustybmp only has types 1-5.

Missing types in rustybmp:
- **Type 6**: Selective Multicast Ethernet Tag A-D route (RFC 8365 §6.3) — for BUM traffic in EVPN
- **Type 7**: IGMP Join Synch route (RFC 8365 §11.2) — multicast group membership
- **Type 8**: IGMP Leave Synch route (RFC 8365 §11.2) — multicast leave
- **Type 9**: Per-Region I-PMSI A-D route (RFC 9251) — multicast infrastructure
- **Type 10**: S-PMSI A-D route (RFC 9251) — selective multicast
- **Type 11**: Leaf A-D route (RFC 9572) — leaf discovery

Types 6-8 are required for EVPN multicast (BUM traffic) monitoring. Types 9-11 are for Provider Multicast Service Interface.

**E. Additional AFI/SAFIs**

goBMP handles address families we're missing:
- **MVPN / MCAST-VPN** (AFI 1/2, SAFI 5/129) — Multicast VPN
- **Route Target Constraint** (AFI 1/2, SAFI 132) — critical for BGP RT filtering at scale (RFC 4684)
- **L2VPN VPLS** (AFI 25, SAFI 65) — legacy Virtual Private LAN Service
- **SR Policy v4/v6** (AFI 1/2, SAFI 73) — Segment Routing Policy

**F. BGP-LS link attribute TLVs (RFC 7752 §3.3.2)**

goBMP decodes ALL link attributes in BGP-LS UPDATE messages:
- **TLV 1088**: Adjacency SID (RFC 8667 §2.2.1)
- **TLV 1099**: LAN Adjacency SID
- **TLV 1114**: Peer Node SID (BGP-LS EPE)
- **TLV 1115**: Peer Adj SID (BGP-LS EPE)
- **TLV 1116**: Peer Set SID (BGP-LS EPE)
- **TLV 1152**: TE Default Metric
- **TLV 1153**: Link Protection Type
- **TLV 1154**: MPLS Protocol Mask
- **TLV 1155**: Metric (IGP)
- **TLV 1156**: Shared Risk Link Group (SRLG)
- **TLV 1158**: Max Link Bandwidth
- **TLV 1159**: Max Reservable Link Bandwidth
- **TLV 1160**: Unreserved Bandwidth (8 priority levels)
- **TLV 1161**: TE Admin Group

These are essential for:
- Traffic engineering database (TED) construction
- Optimal path computation
- Bandwidth monitoring
- SRLG-aware monitoring

**G. BGP-LS node attribute TLVs (RFC 7752 §3.3.1)**

Beyond node descriptors, goBMP decodes:
- **TLV 1024**: Multi-Topology ID
- **TLV 1027**: IS-IS Area Identifier
- **TLV 1028**: IPv4 Router-ID of Local Node
- **TLV 1029**: IPv6 Router-ID of Local Node
- **TLV 1066**: SR Capabilities (SRGB ranges, SR algorithms)
- **TLV 1067**: SR Algorithm
- **TLV 1068**: SR Local Block (SRLB)
- **TLV 1035**: Node Name TLV (hostname from IS-IS TLV 137)
- **TLV 1036**: IS-IS Area ID
- **TLV 265**: IP Reachability info (already have this)

**H. BGP-LS prefix attributes (RFC 7752 §3.3.3)**

- **TLV 1152**: IGP Route Tag
- **TLV 1153**: Extended Route Tag
- **TLV 1154**: Prefix Metric
- **TLV 1155**: OSPF Forwarding Address
- **TLV 1156**: Opaque Prefix Attribute
- **TLV 1158**: Prefix-SID TLV (separate from BGP Prefix-SID type 40)

**I. Intercept/proxy mode**

goBMP can act as a transparent proxy — it intercepts a BMP stream and forwards the raw messages to another collector while also processing them locally. Use case: insert goBMP between router and existing OpenBMP deployment.

**J. NATS output**

In addition to Kafka, goBMP supports NATS (lightweight pub/sub). Useful for edge deployments where Kafka is overkill.

**K. RIPE RIS feed integration (ris2bmp)**

The companion `ris2bmp` container converts RIPE RIS live BGP feeds to BMP messages, allowing testing with live internet routing data.

#### What rustybmp does better than goBMP

| Area | RustyBMP advantage |
|------|-------------------|
| Language | Rust: memory safety, no GC, faster |
| Persistence | DuckDB embedded analytics store |
| Analytics | Z-score, hijack/leak detection built-in |
| RPKI | Live RTR client, per-route validation, VRP cache |
| RFC 9972 | Full stats types 18-38 |
| HTTP API | Rich REST API for querying |
| SSE stream | Real-time event streaming to clients |
| History | Full route change history queryable |

#### Gap severity assessment

| goBMP feature | Severity for rustybmp | RV3 epic |
|--------------|----------------------|----------|
| SR Policy NLRI (SAFI 73) | CRITICAL for SP/SD-WAN | RV3-1 |
| SRv6 BGP-LS extensions | HIGH for modern DC | RV3-1 |
| EVPN types 6-11 | HIGH for DC multicast | RV3-1 |
| BGP-LS link attr TLVs | HIGH for topology | RV3-2 |
| BGP-LS node attr TLVs (SRGB, SR algo) | HIGH for SR | RV3-2 |
| Route Target Constraint SAFI 132 | MEDIUM for large VPN | RV3-1 |
| MVPN/MCAST-VPN | MEDIUM for ISP | RV3-1 |
| Flex Algorithm | MEDIUM for TE | RV3-2 |
| L2VPN VPLS | LOW (legacy) | Backlog RV4 |
| Intercept/proxy mode | MEDIUM | RV3-7 |
| NATS output | LOW | Backlog RV4 |

---

## Part 3 — Comprehensive Gap Analysis

### 3.1 Where rustybmp leads the field (reality check)

| Capability | vs OpenBMP | vs Rotonda | vs bbmp2kafka | vs goBMP |
|-----------|-----------|-----------|--------------|---------|
| RFC 9972 stats (May 2026) | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead |
| RPKI RTR client | Partial parity | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead |
| Z-score anomaly detection | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead |
| BGP hijack/leak detection | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead |
| Embedded analytics DB | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead | ✅ We're ahead |
| Rust memory safety | ✅ vs C++ | Parity | ✅ vs Go GC | ✅ vs Go GC |
| RFC 7432 EVPN types 1-5 | Parity | Unknown | Unknown | Partial (we match 1-5) |
| SSE real-time event stream | ✅ We're ahead | Partial (MQTT) | ❌ No API | ❌ No API |
| Python SDK | ✅ We're ahead | ❌ None | ❌ None | ❌ None |

### 3.2 Where we're behind (things to fix)

| Capability | vs OpenBMP | vs Rotonda | vs bbmp2kafka | vs goBMP | Priority |
|-----------|-----------|-----------|--------------|---------|----------|
| Kafka output | ❌ Critical gap | ❌ Gap | ❌ Their core | ❌ Gap | P0 |
| SR Policy NLRI (SAFI 73) | ❌ Gap | ❌ Gap | ❌ Gap | ❌ They have it | P0 |
| EVPN types 6-11 | Partial parity | Unknown | Unknown | ❌ They have 1-11 | P0 |
| BGP-LS link attributes | ❌ They parse all TLVs | Unknown | ❌ Gap | ❌ They parse all TLVs | P0 |
| Programmable filter language | ❌ None | Roto = their USP | ❌ None | ❌ None | P1 |
| MRT file import | ❌ Gap | ✅ They have it | ❌ None | ❌ None | P1 |
| MRT export | ✅ They have it | Unknown | ❌ None | ❌ None | P1 |
| Proxy/intercept mode | ✅ They have it | Unknown | ❌ None | ✅ They have it | P1 |
| DNS PTR enrichment | ✅ They have it | Unknown | ❌ None | ❌ None | P2 |
| Route Target Constraint | ❌ Gap | Unknown | ❌ Gap | ✅ They have it | P2 |
| MVPN/MCAST-VPN | ❌ Gap | Unknown | ❌ Gap | ✅ They have it | P2 |
| Flex Algorithm BGP-LS | ❌ Gap | Unknown | ❌ Gap | ✅ They have it | P2 |
| SRv6 BGP-LS extensions | ❌ Gap | Unknown | ❌ Gap | ✅ They have it | P2 |
| NATS output | ❌ None | ❌ None | ❌ None | ✅ They have it | P3 |

---

## Part 4 — RV3 Epics

### Epic RV3-1: Protocol Completeness — SR Policy, EVPN Types 6-11, Missing AFI/SAFIs

**Scope**: `crates/rbmp-core/`

#### RV3-1 T1 — SR Policy NLRI (AFI 1/2, SAFI 73) — RFC 9256/9831

**New file**: `crates/rbmp-core/src/bgp/srpolicy.rs`

SR Policy is how operators distribute traffic engineering policies via BGP. SAFI 73 carries `(distinguisher, color, endpoint)` tuples with NLRI and a set of candidate paths, each containing a segment list.

```rust
// crates/rbmp-core/src/bgp/srpolicy.rs

use serde::{Deserialize, Serialize};
use crate::{Error, Result};

/// SR Policy NLRI key: discriminator(4) + color(4) + endpoint(4 or 16)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrPolicyNlri {
    pub discriminator: u32,
    pub color:         u32,
    pub endpoint:      std::net::IpAddr,
}

/// RFC 9256 §2.4 — Segment List with weight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentList {
    pub weight:   u32,
    pub segments: Vec<Segment>,
}

/// RFC 9256 §2.4.4 — Segment types A–K
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Segment {
    /// Type A: MPLS label with optional flags
    MplsLabel     { label: u32, tc: u8, s: bool, ttl: u8 },
    /// Type B: SRv6 SID (128-bit)
    Srv6Sid       { sid: [u8; 16], endpoint_behavior: Option<u16> },
    /// Type C: IPv4 prefix with algorithm
    Ipv4Prefix    { prefix: std::net::Ipv4Addr, prefix_len: u8, algorithm: u8 },
    /// Type D: IPv6 prefix with algorithm
    Ipv6Prefix    { prefix: std::net::Ipv6Addr, prefix_len: u8, algorithm: u8 },
    /// Type E: IPv4 adjacency (local/remote interface IDs)
    Ipv4Adjacency { local_id: u32, remote_id: u32 },
    /// Type F: IPv4 interface addresses
    Ipv4Interface { local_addr: std::net::Ipv4Addr, remote_addr: std::net::Ipv4Addr },
    /// Type G: IPv6 adjacency (local interface ID + addresses)
    Ipv6LocalAdj  { local_id: u32, local_addr: std::net::Ipv6Addr, remote_addr: std::net::Ipv6Addr },
    /// Type H: IPv6 adjacency (both interface IDs + addresses)
    Ipv6Adjacency { local_id: u32, remote_id: u32, local_addr: std::net::Ipv6Addr, remote_addr: std::net::Ipv6Addr },
    /// Type I: IPv4 next-hop with algorithm
    Ipv4NextHop   { nexthop: std::net::Ipv4Addr, algorithm: u8 },
    /// Type J: IPv6 next-hop with algorithm
    Ipv6NextHop   { nexthop: std::net::Ipv6Addr, algorithm: u8 },
    /// Type K: Segment sub-list (nested)
    SubList       { sub_segments: Vec<Segment> },
    Unknown       { seg_type: u8, data: Vec<u8> },
}

/// SR Policy candidate path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePath {
    pub preference:  u32,
    pub name:        Option<String>,
    pub segment_lists: Vec<SegmentList>,
    pub is_best:     bool,
}

/// Full SR Policy decoded from BGP UPDATE
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrPolicy {
    pub nlri:            SrPolicyNlri,
    pub candidate_paths: Vec<CandidatePath>,
}

/// Decode SR Policy NLRI from MP_REACH body (AFI 1 or 2, SAFI 73)
pub fn decode_srpolicy_nlri(buf: &[u8], afi_is_ipv6: bool) -> Result<Vec<SrPolicyNlri>> {
    let mut policies = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        // SR Policy NLRI: prefix_len(1) + distinguisher(4) + color(4) + endpoint(4 or 16)
        if pos + 9 > buf.len() { break; }
        let prefix_len    = buf[pos]; pos += 1;
        let distinguisher = u32::from_be_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]); pos += 4;
        let color         = u32::from_be_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]); pos += 4;
        let ep_octets     = if afi_is_ipv6 { 16 } else { 4 };
        if pos + ep_octets > buf.len() { break; }
        let endpoint = if afi_is_ipv6 {
            let mut b = [0u8; 16]; b.copy_from_slice(&buf[pos..pos+16]);
            std::net::IpAddr::V6(std::net::Ipv6Addr::from(b))
        } else {
            std::net::IpAddr::V4(std::net::Ipv4Addr::from([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]))
        };
        pos += ep_octets;
        policies.push(SrPolicyNlri { discriminator, color, endpoint });
    }
    Ok(policies)
}

/// Parse SR Policy tunnel attribute TLVs from path attribute type 23 sub-TLVs
/// Called from attributes.rs when tunnel_encap type = 15 (SR Policy) or 23 (SRv6)
pub fn parse_srpolicy_candidate_paths(buf: &[u8]) -> Result<Vec<CandidatePath>> {
    // SR Policy tunnel attribute uses sub-TLVs:
    // Type 1 = Remote Endpoint, Type 2 = Color, Type 4 = Binding SID
    // Type 128 = Preference sub-TLV, Type 129 = Binding SID sub-TLV
    // Type 130 = ENH path name, Type 132 = Segment List sub-TLV
    let mut paths = Vec::new();
    let mut pos = 0;
    while pos + 3 <= buf.len() {
        let sub_type = buf[pos]; pos += 1;
        let sub_len  = u16::from_be_bytes([buf[pos], buf[pos+1]]) as usize; pos += 2;
        if pos + sub_len > buf.len() { break; }
        let sub_data = &buf[pos..pos+sub_len]; pos += sub_len;
        match sub_type {
            // IANA-assigned: 128 = Preference (RFC 9256)
            128 if sub_len >= 8 => {
                // flags(1) + reserved(3) + preference(4)
                let preference = u32::from_be_bytes([sub_data[4], sub_data[5], sub_data[6], sub_data[7]]);
                // Parse nested segment list sub-TLVs from remaining
                let seg_lists = parse_segment_lists(&sub_data[8..])?;
                paths.push(CandidatePath {
                    preference,
                    name: None,
                    segment_lists: seg_lists,
                    is_best: false,
                });
            }
            _ => {}
        }
    }
    // Mark the path with highest preference as best
    if let Some(best) = paths.iter_mut().max_by_key(|p| p.preference) {
        best.is_best = true;
    }
    Ok(paths)
}

fn parse_segment_lists(buf: &[u8]) -> Result<Vec<SegmentList>> {
    let mut lists = Vec::new();
    let mut pos = 0;
    while pos + 3 <= buf.len() {
        let sub_type = buf[pos]; pos += 1;
        let sub_len  = u16::from_be_bytes([buf[pos], buf[pos+1]]) as usize; pos += 2;
        if pos + sub_len > buf.len() { break; }
        let data = &buf[pos..pos+sub_len]; pos += sub_len;
        // Type 132 = Segment List (weight(1) + reserved(3) + segments...)
        if sub_type == 132 && data.len() >= 4 {
            let weight   = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            let segments = parse_segments(&data[4:])?;
            lists.push(SegmentList { weight, segments });
        }
    }
    Ok(lists)
}

fn parse_segments(buf: &[u8]) -> Result<Vec<Segment>> {
    let mut segs = Vec::new();
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let seg_type = buf[pos]; pos += 1;
        let seg_len  = buf[pos] as usize; pos += 1;
        // flags(1) + reserved(1) + type-specific data
        if pos + 2 > buf.len() { break; }
        let _flags  = buf[pos]; pos += 1;
        let _reserved = buf[pos]; pos += 1;
        if seg_len < 2 { continue; }
        let data_len = seg_len - 2;
        if pos + data_len > buf.len() { break; }
        let data = &buf[pos..pos+data_len]; pos += data_len;

        let seg = match seg_type {
            1 => { // Type A: MPLS label (3 bytes)
                if data.len() >= 3 {
                    let raw = u32::from_be_bytes([0, data[0], data[1], data[2]]);
                    Segment::MplsLabel { label: raw >> 12, tc: ((raw >> 9) & 0x07) as u8,
                        s: raw & 0x100 != 0, ttl: (raw & 0xFF) as u8 }
                } else { continue; }
            }
            2 => { // Type B: SRv6 SID (16 bytes)
                if data.len() >= 16 {
                    let mut sid = [0u8; 16]; sid.copy_from_slice(&data[..16]);
                    let eb = if data.len() >= 18 {
                        Some(u16::from_be_bytes([data[16], data[17]]))
                    } else { None };
                    Segment::Srv6Sid { sid, endpoint_behavior: eb }
                } else { continue; }
            }
            3 => { // Type C: IPv4 prefix + algorithm
                if data.len() >= 6 {
                    Segment::Ipv4Prefix {
                        prefix: std::net::Ipv4Addr::from([data[0],data[1],data[2],data[3]]),
                        prefix_len: data[4], algorithm: data[5]
                    }
                } else { continue; }
            }
            4 => { // Type D: IPv6 prefix + algorithm
                if data.len() >= 18 {
                    let mut b = [0u8;16]; b.copy_from_slice(&data[..16]);
                    Segment::Ipv6Prefix {
                        prefix: std::net::Ipv6Addr::from(b),
                        prefix_len: data[16], algorithm: data[17]
                    }
                } else { continue; }
            }
            _ => Segment::Unknown { seg_type, data: data.to_vec() },
        };
        segs.push(seg);
    }
    Ok(segs)
}
```

**Wire into PathAttributes**: add `sr_policy: Option<Vec<CandidatePath>>` to `PathAttributes`. In `attributes.rs`, when type 23 (Tunnel Encap) is decoded with encapsulation sub-type SR Policy, call `parse_srpolicy_candidate_paths`.

**Wire into Safi**: add `SrPolicy = 73` to the `Safi` enum and update `dispatch_nlri_decode`.

**DuckDB table**: add `srpolicy_events` with fields: `discriminator, color, endpoint, candidate_path_count, best_preference`.

#### RV3-1 T2 — EVPN Route Types 6-11

**File**: `crates/rbmp-core/src/bgp/evpn.rs`

Extend `EvpnRoute` enum with types 6-11:

```rust
/// Type 6: Selective Multicast Ethernet Tag A-D (RFC 8365 §6.3)
SelectiveMulticastEthernetTag {
    rd:           [u8; 8],
    ethernet_tag: u32,
    multicast_source: std::net::IpAddr,   // source IP or 0.0.0.0
    multicast_group:  std::net::IpAddr,   // multicast group
    originating_router_ip: std::net::IpAddr,
},
/// Type 7: IGMP Join Synch A-D route (RFC 8365 §11.2)
IgmpJoinSynch {
    rd:           [u8; 8],
    ethernet_tag: u32,
    multicast_source: std::net::IpAddr,
    multicast_group:  std::net::IpAddr,
    originating_router_ip: std::net::IpAddr,
},
/// Type 8: IGMP Leave Synch A-D route (RFC 8365 §11.2)
IgmpLeaveSynch {
    rd:           [u8; 8],
    ethernet_tag: u32,
    multicast_source: std::net::IpAddr,
    multicast_group:  std::net::IpAddr,
    originating_router_ip: std::net::IpAddr,
},
/// Type 9: Per-Region I-PMSI A-D route (RFC 9251)
PerRegionIPmsi {
    rd:           [u8; 8],
    ethernet_tag: u32,
    originating_router_ip: std::net::IpAddr,
},
/// Type 10: S-PMSI A-D route (RFC 9251)
SPmsi {
    rd:           [u8; 8],
    ethernet_tag: u32,
    multicast_source: std::net::IpAddr,
    multicast_group:  std::net::IpAddr,
    originating_router_ip: std::net::IpAddr,
},
/// Type 11: Leaf A-D route (RFC 9572)
LeafAD {
    route_key:    Vec<u8>,    // original route's key (variable length)
    path_id:      u32,
    originating_router_ip: std::net::IpAddr,
},
```

Add parse arms for each type in `parse_evpn_route()`.

#### RV3-1 T3 — Route Target Constraint (AFI 1/2, SAFI 132) — RFC 4684

Route Target Constraint allows BGP speakers to advertise which RTs they are interested in. This is essential for VPN-scale deployments (prevents distributing L3VPN routes to PEs that don't need them).

**File**: `crates/rbmp-core/src/bgp/types.rs`

Add to `Safi`:
```rust
RouteTargetConstraint = 132,
```

**New decoder in `bgp/nlri.rs`**:
```rust
/// Decode RT Constraint NLRI (RFC 4684 §4)
/// Format: prefix_len(1) + origin_as(4) + route_target(8 bytes extended community)
pub fn decode_rtc_nlri(buf: &mut impl Buf) -> Result<Vec<RtcNlri>> {
    let mut result = Vec::new();
    while buf.remaining() > 0 {
        let prefix_len = buf.get_u8() as usize; // in bits; 0 = wildcard (all RTs)
        if prefix_len == 0 {
            result.push(RtcNlri::Wildcard);
            continue;
        }
        let octets = (prefix_len + 7) / 8;
        if buf.remaining() < octets { break; }
        let bytes = buf.copy_to_bytes(octets);
        // First 4 bytes = origin AS (if prefix_len >= 32)
        // Remaining = RT extended community prefix
        let origin_as = if octets >= 4 {
            Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else { None };
        result.push(RtcNlri::Specific { origin_as, prefix_len: prefix_len as u8, data: bytes.to_vec() });
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RtcNlri {
    Wildcard,
    Specific { origin_as: Option<u32>, prefix_len: u8, data: Vec<u8> },
}
```

#### RV3-1 T4 — L2VPN VPLS (AFI 25, SAFI 65) and MCAST-VPN (SAFI 5)

Add `Safi::Vpls = 65` and `Safi::McastVpn = 5` to the enum. For VPLS, decode the L2VPN VPLS NLRI (RFC 4761 format: RD + VE ID + VE block offset + label base). For MCAST-VPN, decode the MVPN NLRI type (RFC 6514 §4) — at minimum the NLRI type byte and lengths.

---

### Epic RV3-2: BGP-LS Attribute Richness — Link, Node, Prefix TLVs + Flex Algo

**Scope**: `crates/rbmp-core/src/bgp/bgpls.rs`

Currently bgpls.rs decodes NLRI types (node/link/prefix) but NOT the BGP-LS path attribute (attribute type 29). This is where all the rich topology data lives.

#### RV3-2 T1 — BGP-LS Path Attribute (type 29) decoder

**File**: `crates/rbmp-core/src/bgp/bgpls.rs` (new section)

```rust
// ─── BGP-LS attribute type 29 (RFC 7752 §3.3) ────────────────────────────────

/// BGP-LS path attribute (attribute type 29) — carries topology details
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BgpLsAttribute {
    // Node attributes
    pub node_flags:          Option<u8>,       // TLV 1024
    pub node_name:           Option<String>,   // TLV 1026 (IS-IS hostname)
    pub isis_area_id:        Option<Vec<u8>>,  // TLV 1027
    pub ipv4_router_id:      Option<std::net::Ipv4Addr>, // TLV 1028
    pub ipv6_router_id:      Option<std::net::Ipv6Addr>, // TLV 1029
    pub sr_capabilities:     Option<SrCapabilities>,     // TLV 1034
    pub sr_algorithm:        Vec<u8>,          // TLV 1035: list of SR algorithms
    pub sr_local_block:      Option<SrLocalBlock>, // TLV 1036 (SRLB)
    // Link attributes
    pub link_metric_igp:     Option<u32>,      // TLV 1095
    pub link_metric_te:      Option<u32>,      // TLV 1092
    pub admin_group:         Option<u32>,      // TLV 1088
    pub max_bandwidth:       Option<f32>,      // TLV 1081: IEEE 754 float, bytes/sec
    pub max_reservable_bw:   Option<f32>,      // TLV 1082
    pub unreserved_bw:       Vec<f32>,         // TLV 1083: 8 priority levels
    pub srlg:                Vec<u32>,         // TLV 1094: Shared Risk Link Groups
    pub adj_sid:             Vec<AdjSid>,      // TLV 1099 (RFC 8667)
    pub lan_adj_sid:         Vec<LanAdjSid>,   // TLV 1100
    pub peer_node_sid:       Option<u32>,      // TLV 1101 (EPE)
    pub peer_adj_sid:        Option<u32>,      // TLV 1102 (EPE)
    pub peer_set_sid:        Option<u32>,      // TLV 1103 (EPE)
    // Prefix attributes
    pub prefix_metric:       Option<u32>,      // TLV 1155
    pub ospf_fwd_addr:       Option<std::net::IpAddr>, // TLV 1156
    pub prefix_sid:          Vec<LsPrefixSid>, // TLV 1158 (Prefix-SID in BGP-LS)
    // Flex Algorithm
    pub flex_algo_defs:      Vec<FlexAlgoDef>, // TLV 1039
    pub flex_algo_prefix_metric: Vec<FlexAlgoPrefixMetric>, // TLV 1044
    // Unknown TLVs preserved
    pub unknown_tlvs:        Vec<(u16, Vec<u8>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrCapabilities {
    pub flags: u8,
    pub srgb_ranges: Vec<(u32, u32)>,  // (base, range)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrLocalBlock {
    pub flags: u8,
    pub srlb_ranges: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjSid {
    pub flags: u8,
    pub weight: u8,
    pub label: u32,  // or SRv6 SID (16 bytes) for SRv6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanAdjSid {
    pub flags: u8,
    pub weight: u8,
    pub neighbor_id: [u8; 7], // IS-IS system ID or OSPF router ID
    pub label: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsPrefixSid {
    pub flags:     u8,
    pub algorithm: u8,
    pub label:     u32,  // or index when N/A flag set
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexAlgoDef {
    pub algo_id:      u8,
    pub metric_type:  u8,    // 0=IGP, 1=min unidirectional delay, 2=TE metric
    pub calc_type:    u8,    // 0=SPF, 1=Strict SPF
    pub priority:     u8,
    pub flags:        u16,
    pub exclude_any:  Option<u32>,
    pub include_any:  Option<u32>,
    pub include_all:  Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexAlgoPrefixMetric {
    pub algo_id:  u8,
    pub metric:   u32,
}

/// Parse BGP-LS path attribute (type 29) — TLV encoded
pub fn parse_bgpls_attribute(buf: &[u8]) -> BgpLsAttribute {
    let mut attr = BgpLsAttribute::default();
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let tlv_type = u16::from_be_bytes([buf[pos], buf[pos+1]]);
        let tlv_len  = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) as usize;
        pos += 4;
        if pos + tlv_len > buf.len() { break; }
        let data = &buf[pos..pos+tlv_len];
        pos += tlv_len;

        match tlv_type {
            // Node name (IS-IS TLV 137)
            1026 => attr.node_name = Some(String::from_utf8_lossy(data).to_string()),
            // IS-IS Area ID
            1027 => attr.isis_area_id = Some(data.to_vec()),
            // IPv4 Router-ID of local node
            1028 if data.len() == 4 => attr.ipv4_router_id = Some(
                std::net::Ipv4Addr::from([data[0],data[1],data[2],data[3]])),
            // IPv6 Router-ID of local node
            1029 if data.len() == 16 => {
                let mut b = [0u8;16]; b.copy_from_slice(data);
                attr.ipv6_router_id = Some(std::net::Ipv6Addr::from(b));
            }
            // SR Capabilities (RFC 8402)
            1034 if data.len() >= 2 => {
                let flags = data[0];
                let mut ranges = Vec::new();
                let mut i = 2; // skip flags + reserved
                while i + 7 <= data.len() {
                    // Range TLV: type(2) + len(2) + range(3) + SID TLV (type(2)+len(2)+label(3))
                    let sub_len = u16::from_be_bytes([data[i+2], data[i+3]]) as usize;
                    if i + 4 + sub_len <= data.len() {
                        let range = u32::from_be_bytes([0, data[i+4], data[i+5], data[i+6]]);
                        // SID TLV follows
                        let base = if i + 4 + sub_len + 7 <= data.len() {
                            u32::from_be_bytes([0, data[i+4+sub_len+4], data[i+4+sub_len+5], data[i+4+sub_len+6]])
                        } else { 0 };
                        ranges.push((base, range));
                    }
                    i += 4 + sub_len + 7; // approximate
                }
                attr.sr_capabilities = Some(SrCapabilities { flags, srgb_ranges: ranges });
            }
            // SR Algorithm
            1035 => attr.sr_algorithm = data.to_vec(),
            // IGP Metric
            1095 if data.len() >= 3 => {
                attr.link_metric_igp = Some(u32::from_be_bytes([0, data[0], data[1], data[2]]));
            }
            // TE Default Metric
            1092 if data.len() == 4 => {
                attr.link_metric_te = Some(u32::from_be_bytes([data[0],data[1],data[2],data[3]]));
            }
            // Admin Group
            1088 if data.len() == 4 => {
                attr.admin_group = Some(u32::from_be_bytes([data[0],data[1],data[2],data[3]]));
            }
            // Max Bandwidth (IEEE 754 float)
            1081 if data.len() == 4 => {
                attr.max_bandwidth = Some(f32::from_be_bytes([data[0],data[1],data[2],data[3]]));
            }
            // Max Reservable Bandwidth
            1082 if data.len() == 4 => {
                attr.max_reservable_bw = Some(f32::from_be_bytes([data[0],data[1],data[2],data[3]]));
            }
            // Unreserved Bandwidth (8 × 4 bytes)
            1083 if data.len() == 32 => {
                for i in 0..8 {
                    attr.unreserved_bw.push(f32::from_be_bytes(
                        [data[i*4], data[i*4+1], data[i*4+2], data[i*4+3]]));
                }
            }
            // SRLG
            1094 => {
                let mut i = 0;
                while i + 4 <= data.len() {
                    attr.srlg.push(u32::from_be_bytes([data[i],data[i+1],data[i+2],data[i+3]]));
                    i += 4;
                }
            }
            // Adjacency SID (RFC 8667)
            1099 if data.len() >= 7 => {
                let flags  = data[0];
                let weight = data[1];
                let label  = u32::from_be_bytes([0, data[4], data[5], data[6]]);
                attr.adj_sid.push(AdjSid { flags, weight, label });
            }
            // Prefix Metric
            1155 if data.len() == 4 => {
                attr.prefix_metric = Some(u32::from_be_bytes([data[0],data[1],data[2],data[3]]));
            }
            // Flex Algorithm Definition
            1039 if data.len() >= 4 => {
                attr.flex_algo_defs.push(FlexAlgoDef {
                    algo_id: data[0], metric_type: data[1], calc_type: data[2],
                    priority: data[3], flags: 0,
                    exclude_any: None, include_any: None, include_all: None,
                });
            }
            // Flex Algorithm Prefix Metric
            1044 if data.len() >= 5 => {
                attr.flex_algo_prefix_metric.push(FlexAlgoPrefixMetric {
                    algo_id: data[0],
                    metric: u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
                });
            }
            _ => attr.unknown_tlvs.push((tlv_type, data.to_vec())),
        }
    }
    attr
}
```

**Wire into PathAttributes**:
```rust
pub bgpls_attr: Option<BgpLsAttribute>,  // decoded from attribute type 29
```

**Wire into attributes.rs**:
```rust
// Type 29: BGP-LS attribute
29 => {
    attrs.bgpls_attr = Some(parse_bgpls_attribute(attr_buf));
}
```

**DuckDB tables** (add to schema.rs):
```sql
-- BGP-LS link table for topology analytics
CREATE TABLE IF NOT EXISTS bgpls_links (
    id UUID NOT NULL, occurred_at TIMESTAMPTZ NOT NULL,
    speaker_addr VARCHAR, peer_addr VARCHAR, action VARCHAR,
    local_router_id VARCHAR, remote_router_id VARCHAR,
    local_ip VARCHAR, remote_ip VARCHAR,
    igp_metric UINTEGER, te_metric UINTEGER,
    max_bandwidth FLOAT, max_reservable_bw FLOAT,
    admin_group UINTEGER,
    adj_sid_labels VARCHAR,   -- comma-separated
    srlg_groups VARCHAR        -- comma-separated
);

-- BGP-LS node table
CREATE TABLE IF NOT EXISTS bgpls_nodes (
    id UUID NOT NULL, occurred_at TIMESTAMPTZ NOT NULL,
    speaker_addr VARCHAR, peer_addr VARCHAR, action VARCHAR,
    protocol_id UTINYINT, router_id VARCHAR,
    node_name VARCHAR, isis_area VARCHAR,
    sr_capabilities_srgb VARCHAR,   -- base:range pairs
    sr_algorithms VARCHAR            -- comma-separated algo IDs
);
```

---

### Epic RV3-3: Python Layer Completeness

**Scope**: `bmppy/rbmppy/`

These are the Python files specified in RV2 that were NOT implemented.

#### RV3-3 T1 — `bmppy/rbmppy/rpki.py`

Implement the full `LocalVrpCache` class with:
- `download_from_cloudflare()` async class method
- `load_from_cloudflare_json(path)` for offline use
- `validate(prefix_cidr, origin_as) → ValidationResult`
- `bulk_validate(routes) → list[ValidationResult]`
- Integration with rustybmp's `/api/routes` to run bulk validation

The class spec is in the RV2 backlog — implement exactly as written there.

#### RV3-3 T2 — `bmppy/rbmppy/internet.py`

Implement `InternetIntelligenceClient` with:
- PeeringDB `asn_info(asn)` with in-process TTL cache (24h)
- RIPE STAT `prefix_info(prefix_cidr)` with 1h cache
- `bulk_asn_info(asns)` async parallel fetcher
- IX presence list from PeeringDB `netixlan` API
- Announced prefixes from RIPE STAT `announced-prefixes`

The class spec is in the RV2 backlog.

#### RV3-3 T3 — `bmppy/rbmppy/detectors.py`

Implement `DetectorPipeline`:
- Subscribes to rustybmp SSE stream
- Routes `route_change` events to `ZScoreMonitor` + `HijackDetector` + `RouteLeakDetector`
- Routes `peer_down` events to `FlapScorer`
- Dispatches `AnomalyAlert` to registered handlers
- Async handlers allow e.g. Slack webhook, PagerDuty, email

The class spec is in the RV2 backlog.

#### RV3-3 T4 — `bmppy/rbmppy/__init__.py` updates

Export all new modules:
```python
from .rpki import LocalVrpCache, RpkiState, ValidationResult
from .internet import InternetIntelligenceClient, AsnInfo, PrefixInfo
from .detectors import DetectorPipeline, AnomalyAlert
from .analytics import ZScoreMonitor, HijackDetector, RouteLeakDetector, FlapScorer, RouteAnalytics
```

---

### Epic RV3-4: DNS PTR Enrichment + Speaker Hostname Resolution

**Scope**: `crates/rbmp-server/src/`

Every other BMP collector (OpenBMP, goBMP) does a DNS PTR lookup when a BMP speaker connects to resolve the router hostname. Currently rustybmp only has static config-based names. Add automatic enrichment.

**New file**: `crates/rbmp-server/src/dns.rs`

```rust
use std::net::IpAddr;
use tokio::net::lookup_host;
use tracing::debug;

/// Perform a reverse DNS lookup for a BMP speaker IP.
/// Returns the PTR record hostname, or the IP string if lookup fails.
pub async fn reverse_lookup(addr: IpAddr) -> String {
    // Format the PTR query: reverse the octets and append .in-addr.arpa
    let ptr_name = match addr {
        IpAddr::V4(a) => {
            let octets = a.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", octets[3], octets[2], octets[1], octets[0])
        }
        IpAddr::V6(a) => {
            let nibbles: String = a.octets().iter()
                .flat_map(|b| [b & 0x0F, (b >> 4) & 0x0F])
                .rev()
                .map(|n| format!("{:x}", n))
                .collect::<Vec<_>>()
                .join(".");
            format!("{}.ip6.arpa", nibbles)
        }
    };

    match lookup_host(&format!("{}:0", ptr_name)).await {
        Ok(mut results) => {
            // lookup_host returns IpAddr results, not hostnames
            // Use std::net::ToSocketAddrs for PTR
            debug!(%ptr_name, "PTR lookup succeeded");
            addr.to_string() // Fallback: tokio doesn't expose PTR hostname directly
        }
        Err(_) => addr.to_string(),
    }
}

/// Async PTR lookup using the system resolver via `getnameinfo`.
/// This is the actual hostname-returning version using libc.
pub async fn ptr_hostname(addr: IpAddr) -> String {
    let addr_str = addr.to_string();
    tokio::task::spawn_blocking(move || {
        // Use trust-dns-resolver or system resolver
        // For simplicity in RV3, use std::net socket name resolution
        use std::net::{SocketAddr, ToSocketAddrs};
        let sa: SocketAddr = format!("{}:0", addr_str).parse().unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());
        // This performs getnameinfo internally
        match sa.to_socket_addrs() {
            Ok(_) => addr_str, // can't get PTR this way
            Err(_) => addr_str,
        }
    }).await.unwrap_or_else(|_| addr.to_string())
}
```

**Correct implementation** using `trust-dns-resolver`:
Add to `rbmp-server/Cargo.toml`:
```toml
trust-dns-resolver = { version = "0.23", features = ["tokio-runtime"] }
```

```rust
use trust_dns_resolver::TokioAsyncResolver;
use trust_dns_resolver::config::*;

pub struct DnsEnricher {
    resolver: TokioAsyncResolver,
}

impl DnsEnricher {
    pub async fn new() -> Self {
        let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());
        Self { resolver }
    }

    pub async fn reverse_lookup(&self, addr: IpAddr) -> Option<String> {
        self.resolver.reverse_lookup(addr).await.ok()
            .and_then(|r| r.iter().next().map(|n| n.to_string().trim_end_matches('.').to_string()))
    }
}
```

**Wire into `receiver.rs`**: When a speaker connects, spawn a task to do PTR lookup and store result in `SpeakerRegistry`. If config has static hostname, skip the lookup.

**API response enrichment**: All `/api/speakers` responses should include `hostname` from registry, populated by PTR lookup if not statically configured.

---

### Epic RV3-5: Kafka Output Producer

**New crate**: `crates/rbmp-kafka/`  
**This is the most requested feature gap across all four competitors.**

```
crates/rbmp-kafka/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── producer.rs      # Kafka producer wrapping rdkafka
    ├── schema.rs        # Topic names and message formats
    └── serializer.rs    # JSON and Protobuf serialization
```

**`Cargo.toml`**:
```toml
[package]
name = "rbmp-kafka"
description = "Kafka output producer for rustybmp"
version.workspace = true
edition.workspace = true

[dependencies]
rbmp-core  = { workspace = true }
rbmp-rib   = { workspace = true }
tokio      = { workspace = true }
serde      = { workspace = true }
serde_json = { workspace = true }
tracing    = { workspace = true }
rdkafka    = { version = "0.36", features = ["cmake-build"] }
```

**Topic scheme** (mirrors OpenBMP for ecosystem compatibility):

```rust
// crates/rbmp-kafka/src/schema.rs

pub const TOPIC_ROUTER:         &str = "rustybmp.parsed.router";
pub const TOPIC_PEER:           &str = "rustybmp.parsed.peer";
pub const TOPIC_UNICAST_PREFIX: &str = "rustybmp.parsed.unicast_prefix";
pub const TOPIC_L3VPN:          &str = "rustybmp.parsed.l3vpn";
pub const TOPIC_EVPN:           &str = "rustybmp.parsed.evpn";
pub const TOPIC_LS_NODE:        &str = "rustybmp.parsed.ls_node";
pub const TOPIC_LS_LINK:        &str = "rustybmp.parsed.ls_link";
pub const TOPIC_LS_PREFIX:      &str = "rustybmp.parsed.ls_prefix";
pub const TOPIC_BMP_STAT:       &str = "rustybmp.parsed.bmp_stat";
pub const TOPIC_BMP_RAW:        &str = "rustybmp.bmp_raw";     // binary replay stream
pub const TOPIC_SR_POLICY:      &str = "rustybmp.parsed.sr_policy";
```

**Producer**:
```rust
// crates/rbmp-kafka/src/producer.rs

use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rbmp_rib::event::{RibEvent, RibEventPayload};
use serde_json::json;
use tracing::{error, debug};

pub struct KafkaProducer {
    producer: FutureProducer,
    brokers:  String,
}

impl KafkaProducer {
    pub fn new(brokers: &str) -> anyhow::Result<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.messages", "100000")
            .set("compression.type", "lz4")
            .create()?;
        Ok(Self { producer, brokers: brokers.to_string() })
    }

    /// Publish a RibEvent to the appropriate Kafka topic.
    pub async fn publish(&self, event: &RibEvent) {
        let (topic, key, payload) = self.event_to_kafka(event);
        let payload_str = serde_json::to_string(&payload).unwrap_or_default();
        let record = FutureRecord::to(&topic)
            .key(&key)
            .payload(&payload_str);
        match self.producer.send(record, std::time::Duration::from_secs(5)).await {
            Ok(_) => debug!(topic, "Kafka message published"),
            Err((e, _)) => error!(error = %e, topic, "Kafka publish failed"),
        }
    }

    fn event_to_kafka(&self, ev: &RibEvent) -> (String, String, serde_json::Value) {
        let speaker = ev.speaker.to_string();
        match &ev.payload {
            RibEventPayload::RouteChange(rc) => {
                let key = format!("{}:{}", rc.peer_header.peer_address, rc.prefix);
                let topic = match rc.prefix.addr_family() {
                    rbmp_core::bgp::types::Afi::L2Vpn => crate::schema::TOPIC_EVPN.to_string(),
                    _ if matches!(rc.rib_type, rbmp_core::bmp::types::RibType::AdjRibInPrePolicy |
                                              rbmp_core::bmp::types::RibType::AdjRibInPostPolicy |
                                              rbmp_core::bmp::types::RibType::LocRib) =>
                        crate::schema::TOPIC_UNICAST_PREFIX.to_string(),
                    _ => crate::schema::TOPIC_UNICAST_PREFIX.to_string(),
                };
                let payload = json!({
                    "action":      format!("{:?}", rc.action).to_lowercase(),
                    "speaker":     speaker,
                    "peer_addr":   rc.peer_header.peer_address.to_string(),
                    "peer_as":     rc.peer_header.peer_as,
                    "rib_type":    format!("{:?}", rc.rib_type),
                    "prefix":      rc.prefix.to_string(),
                    "occurred_at": ev.occurred_at.to_rfc3339(),
                    "as_path":     rc.attributes.as_ref().and_then(|a| a.as_path.as_ref())
                                      .map(|p| p.to_string()),
                    "next_hop":    rc.attributes.as_ref().and_then(|a| a.next_hop)
                                      .map(|h| h.to_string()),
                    "communities": rc.attributes.as_ref().map(|a|
                        a.communities.iter().map(|c| c.to_string()).collect::<Vec<_>>()),
                    "local_pref":  rc.attributes.as_ref().and_then(|a| a.local_pref),
                    "med":         rc.attributes.as_ref().and_then(|a| a.multi_exit_disc),
                });
                (topic, key, payload)
            }
            RibEventPayload::PeerUp { peer_header, local_asn, remote_asn, hold_time, .. } => {
                let key = format!("{}:{}", speaker, peer_header.peer_address);
                let payload = json!({
                    "action":    "up",
                    "speaker":   speaker,
                    "peer_addr": peer_header.peer_address.to_string(),
                    "peer_as":   peer_header.peer_as,
                    "local_as":  local_asn,
                    "hold_time": hold_time,
                    "occurred_at": ev.occurred_at.to_rfc3339(),
                });
                (crate::schema::TOPIC_PEER.to_string(), key, payload)
            }
            RibEventPayload::PeerDown { peer_header, reason } => {
                let key = format!("{}:{}", speaker, peer_header.peer_address);
                let payload = json!({
                    "action":    "down",
                    "speaker":   speaker,
                    "peer_addr": peer_header.peer_address.to_string(),
                    "peer_as":   peer_header.peer_as,
                    "reason":    reason,
                    "occurred_at": ev.occurred_at.to_rfc3339(),
                });
                (crate::schema::TOPIC_PEER.to_string(), key, payload)
            }
            RibEventPayload::SpeakerUp { sys_name, sys_descr } => {
                let payload = json!({
                    "action": "init", "speaker": speaker,
                    "sys_name": sys_name, "sys_descr": sys_descr,
                    "occurred_at": ev.occurred_at.to_rfc3339(),
                });
                (crate::schema::TOPIC_ROUTER.to_string(), speaker.clone(), payload)
            }
            RibEventPayload::SpeakerDown { reason } => {
                let payload = json!({
                    "action": "term", "speaker": speaker, "reason": reason,
                    "occurred_at": ev.occurred_at.to_rfc3339(),
                });
                (crate::schema::TOPIC_ROUTER.to_string(), speaker.clone(), payload)
            }
            RibEventPayload::Stats { peer_header, counters } => {
                let key = format!("{}:{}", speaker, peer_header.peer_address);
                let payload = json!({
                    "speaker": speaker, "peer_addr": peer_header.peer_address.to_string(),
                    "occurred_at": ev.occurred_at.to_rfc3339(),
                    "counters": counters.iter().map(|s| json!({
                        "name": s.name, "value": s.value,
                        "afi": s.afi_safi.map(|a| a.afi.as_u16()),
                        "safi": s.afi_safi.map(|a| a.safi.as_u8()),
                    })).collect::<Vec<_>>(),
                });
                (crate::schema::TOPIC_BMP_STAT.to_string(), key, payload)
            }
            _ => {
                (crate::schema::TOPIC_BMP_STAT.to_string(), speaker.clone(), json!({}))
            }
        }
    }
}
```

**Config addition** (`config.rs`):
```toml
[kafka]
enabled = false
brokers = "localhost:9092"
# Topic prefix (default: "rustybmp.parsed")
topic_prefix = "rustybmp.parsed"
# Enable raw BMP binary stream to kafka.bmp_raw
raw_stream = false
# Compression: none | lz4 | snappy | gzip (lz4 recommended)
compression = "lz4"
```

**Wire into `main.rs`**: Subscribe to `rib.subscribe()`, spawn a Kafka publish task that mirrors the store writer.

---

### Epic RV3-6: MRT File Import and Export

**Scope**: new `crates/rbmp-mrt/`

MRT (Multi-threaded Routing Toolkit) format (RFC 6396) is the standard for BGP routing table dumps and update archives. RIPE RIS, RouteViews, and PCH all use MRT. Supporting it means:
- **Import**: Load historical routing data, test with real internet tables
- **Export**: Archive snapshots for later replay and forensics

#### RV3-6 T1 — MRT types and decoder

```rust
// crates/rbmp-mrt/src/lib.rs

/// RFC 6396 §4 — MRT Types
pub enum MrtType {
    Bgp4MpStateChange = 16,
    Bgp4MpMessage     = 17,
    Bgp4MpStateChangeAs4 = 32,
    Bgp4MpMessageAs4  = 33,
    TableDumpV2       = 13,
}

/// RFC 6396 §4.3 — TABLE_DUMP_V2 subtypes
pub enum TableDumpV2Subtype {
    PeerIndexTable       = 1,
    RibIpv4Unicast       = 2,
    RibIpv6Unicast       = 4,
    RibIpv4Multicast     = 3,
    RibIpv6Multicast     = 5,
    RibGeneric           = 6,
}

/// MRT record header: timestamp(4) + type(2) + subtype(2) + length(4)
pub struct MrtHeader {
    pub timestamp: u32,
    pub mrt_type:  u16,
    pub subtype:   u16,
    pub length:    u32,
}

/// A single MRT record
pub enum MrtRecord {
    PeerIndexTable { collector_id: u32, view_name: String, peers: Vec<MrtPeer> },
    RibEntry { sequence: u32, prefix: String, prefix_len: u8, entries: Vec<RibDumpEntry> },
    Bgp4MpUpdate { peer_as: u32, local_as: u32, peer_ip: String, update_bytes: Vec<u8> },
    Bgp4MpStateChange { peer_as: u32, old_state: u16, new_state: u16 },
}

pub struct MrtPeer { pub ip: String, pub asn: u32, pub bgp_id: String }
pub struct RibDumpEntry { pub peer_index: u16, pub attributes: Vec<u8> }

/// Stream MRT records from a file (async generator pattern)
pub async fn stream_mrt_file(path: &str) -> impl futures::Stream<Item = anyhow::Result<MrtRecord>> {
    // Use tokio::fs::File + BufReader, parse record by record
    unimplemented!("RV3-6 T1")
}
```

#### RV3-6 T2 — MRT import API endpoint

**File**: `crates/rbmp-server/src/api/mrt.rs`

```
POST /api/mrt/import    — upload an MRT file, returns job ID
GET  /api/mrt/jobs/:id  — check import progress
```

The import task:
1. Reads MRT records from the uploaded file
2. For TABLE_DUMP_V2: parses all RIB entries, injects as synthetic `BmpMessage::RouteMonitoring`
3. For BGP4MP_UPDATE: parses as BGP UPDATE, injects as synthetic `BmpMessage::RouteMonitoring`
4. Progress tracked: `{total_records, imported, errors, elapsed_ms}`

#### RV3-6 T3 — MRT snapshot export

**File**: `crates/rbmp-store/src/mrt_export.rs`

Export a point-in-time RIB snapshot as TABLE_DUMP_V2 MRT:

```rust
pub async fn export_rib_as_mrt(
    store: &RouteStore,
    peer_addr: Option<&str>,
    output_path: &str,
) -> anyhow::Result<usize> {
    // Query DuckDB for latest announced routes (current_rib query)
    // Write MRT header + PEER_INDEX_TABLE record
    // For each prefix: write TABLE_DUMP_V2 RIB entry with serialized path attributes
    // Return count of written records
    unimplemented!("RV3-6 T3")
}
```

**API**: `GET /api/mrt/export?peer=<ip>&format=mrt` → downloads MRT file.

---

### Epic RV3-7: BMP Intercept/Proxy Mode

Inspired by both OpenBMP's forwarder and goBMP's intercept mode. Run rustybmp as a transparent BMP proxy: receive from router, process locally AND forward raw bytes to another BMP collector.

**Config**:
```toml
[bmp.proxy]
enabled = false
upstream = "openbmp.example.com:5000"   # another BMP collector
```

**New file**: `crates/rbmp-server/src/proxy.rs`

```rust
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct BmpProxy {
    upstream: Option<Arc<Mutex<TcpStream>>>,
    upstream_addr: String,
}

impl BmpProxy {
    pub async fn new(addr: Option<&str>) -> Self {
        if let Some(a) = addr {
            match TcpStream::connect(a).await {
                Ok(s) => return Self {
                    upstream: Some(Arc::new(Mutex::new(s))),
                    upstream_addr: a.to_string(),
                },
                Err(e) => tracing::warn!(%e, addr = a, "BMP proxy: upstream connect failed"),
            }
        }
        Self { upstream: None, upstream_addr: String::new() }
    }

    /// Forward raw BMP frame bytes to upstream collector.
    /// Called from receiver.rs BEFORE parsing, so upstream gets the original bytes.
    pub async fn forward(&self, frame: &[u8]) {
        if let Some(upstream) = &self.upstream {
            let mut s = upstream.lock().await;
            if let Err(e) = s.write_all(frame).await {
                tracing::warn!(%e, "BMP proxy: forward failed");
            }
        }
    }
}
```

Wire into `receiver.rs`: before `parse_bmp_message`, call `proxy.forward(&frame).await`.

---

### Epic RV3-8: Programmable Route Filter (Inspired by Rotonda's Roto)

Rather than implementing a full compiled language, provide a YAML-based filter DSL that runs at ingest time. This covers 90% of operator use cases.

**New file**: `crates/rbmp-server/src/filter.rs`

```yaml
# config/filters.yaml
filters:
  - name: "reject-bogons"
    action: reject
    match:
      prefix_in:
        - "10.0.0.0/8"
        - "172.16.0.0/12"
        - "192.168.0.0/16"
        - "0.0.0.0/8"
        - "240.0.0.0/4"

  - name: "reject-too-specific"
    action: reject
    match:
      prefix_len_gt: 24  # reject anything more specific than /24 (IPv4)

  - name: "tag-short-paths"
    action: tag
    tag: "short-path"
    match:
      as_path_len_lt: 3

  - name: "alert-new-origins"
    action: alert
    alert_type: "origin_change"
    match:
      origin_as_changed: true
```

**Rust implementation**:
```rust
// crates/rbmp-server/src/filter.rs

use rbmp_core::bmp::types::BmpPayload;
use rbmp_core::bgp::types::PathAttributes;
use ipnet::{Ipv4Net, Ipv6Net};
use serde::Deserialize;
use std::net::IpAddr;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterAction {
    Reject,   // don't store in RIB or DuckDB
    Accept,   // force accept (skip remaining filters)
    Tag,      // add a tag label to the event
    Alert,    // emit an alert event
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FilterMatch {
    pub prefix_in:       Option<Vec<String>>,   // CIDR list
    pub prefix_len_gt:   Option<u8>,
    pub prefix_len_lt:   Option<u8>,
    pub as_path_len_lt:  Option<usize>,
    pub as_path_contains: Option<u32>,           // specific ASN in path
    pub community_has:   Option<String>,         // "64512:100"
    pub rpki_invalid:    Option<bool>,
    pub peer_asn:        Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Filter {
    pub name:   String,
    pub action: FilterAction,
    pub tag:    Option<String>,
    pub alert_type: Option<String>,
    pub matches:    FilterMatch,
}

pub struct FilterEngine {
    filters: Vec<Filter>,
    bogon_nets: Vec<ipnet::IpNet>,
}

impl FilterEngine {
    pub fn new(filters: Vec<Filter>) -> Self {
        let bogon_nets = vec![
            "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16",
            "0.0.0.0/8", "127.0.0.0/8", "169.254.0.0/16",
            "192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24",
            "100.64.0.0/10", "240.0.0.0/4",
        ].iter().filter_map(|s| s.parse().ok()).collect();
        Self { filters, bogon_nets }
    }

    /// Returns (should_store, tags, alerts) for a route announcement.
    pub fn evaluate(
        &self,
        prefix: &rbmp_core::bgp::types::Prefix,
        attrs: &PathAttributes,
        peer_asn: u32,
    ) -> (bool, Vec<String>, Vec<String>) {
        let mut store = true;
        let mut tags  = Vec::new();
        let mut alerts = Vec::new();

        for f in &self.filters {
            if !self.matches_filter(&f.matches, prefix, attrs, peer_asn) {
                continue;
            }
            match f.action {
                FilterAction::Reject => { store = false; }
                FilterAction::Accept => { return (true, tags, alerts); }
                FilterAction::Tag    => { if let Some(t) = &f.tag { tags.push(t.clone()); } }
                FilterAction::Alert  => { if let Some(a) = &f.alert_type { alerts.push(a.clone()); } }
            }
        }
        (store, tags, alerts)
    }

    fn matches_filter(
        &self,
        m: &FilterMatch,
        prefix: &rbmp_core::bgp::types::Prefix,
        attrs: &PathAttributes,
        peer_asn: u32,
    ) -> bool {
        let prefix_str = prefix.to_string();
        // prefix_in check
        if let Some(nets) = &m.prefix_in {
            let target: Option<ipnet::IpNet> = prefix_str.parse().ok();
            if let Some(t) = target {
                let in_list = nets.iter().any(|n| {
                    n.parse::<ipnet::IpNet>().map(|net| t.subnet_of(&net) || t == net).unwrap_or(false)
                });
                if !in_list { return false; }
            }
        }
        // prefix_len_gt
        if let Some(max) = m.prefix_len_gt {
            let len = prefix_str.split('/').nth(1).and_then(|l| l.parse::<u8>().ok()).unwrap_or(0);
            if len <= max { return false; }
        }
        // as_path_len_lt
        if let Some(min) = m.as_path_len_lt {
            let hop_count = attrs.as_path.as_ref().map(|p| p.hop_count()).unwrap_or(0);
            if hop_count >= min { return false; }
        }
        // community_has
        if let Some(community) = &m.community_has {
            let has = attrs.communities.iter().any(|c| c.to_string() == *community);
            if !has { return false; }
        }
        // peer_asn
        if let Some(asn) = m.peer_asn {
            if peer_asn != asn { return false; }
        }
        true
    }
}
```

**Config**:
```toml
[filters]
enabled = false
file    = "config/filters.yaml"
```

**Wire into `manager.rs`**: When processing `RouteMonitoring`, call `filter_engine.evaluate()` before inserting into RIB. If `should_store = false`, skip the RibEntry insert and DuckDB write. If `tags` non-empty, attach to RibEntry (future: store in DuckDB `route_tags` column).

---

### Epic RV3-9: LLGR State Machine (deferred from RV2)

Implement the full Long-Lived Graceful Restart state machine (RFC 9494).

#### RV3-9 T1 — LlgrState in session.rs

Per the RV2 backlog specification. Add:
```rust
pub struct LlgrState {
    pub active_families: Vec<(AfiSafi, u32)>,   // (AFI-SAFI, stale_time_secs)
    pub stale_started: HashMap<String, DateTime<Utc>>,
}
```

#### RV3-9 T2 — Mark stale routes in RIB on LLGR peer down

In `manager.rs`, when `PeerDown` arrives and peer has LLGR active:
- Do NOT clear routes immediately
- Mark all routes for that peer as `llgr_stale = true`
- Store `llgr_expires_at` timestamp
- Start a background task that periodically checks stale timers

#### RV3-9 T3 — LLGR timer expiry task

```rust
// In main.rs, spawn LLGR expiry monitor
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let mut rib = rib_llgr.write().await;
        // For each peer with LLGR active, check if stale timer expired
        // If expired: clear routes for that peer (same as normal PeerDown)
    }
});
```

#### RV3-9 T4 — Stats type 28 (LLGR stale routes) reflects actual count

In `parse_stats_report`, stat type 28 (`per-afi-safi-llgr-stale-routes`) arrives from the router. Decode and surface via Prometheus gauge:
```
rustybmp_bmp_stat{stat="per-afi-safi-llgr-stale-routes", afi="1", safi="1"} 0
```

---

### Epic RV3-10: Distributed Collector/Core (from RV2-11)

Implement the full collector/core split:

#### RV3-10 T1 — `CollectorEnvelope` protobuf/msgpack protocol

**New file**: `crates/rbmp-core/src/collector_protocol.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorEnvelope {
    pub collector_id:   String,   // UUID v4, stable across restarts
    pub site:           String,   // human-readable site name
    pub collector_addr: String,   // IP of the collector for health tracking
    pub bmp_message:    BmpMessage,
}
```

Protocol: length-prefixed MessagePack over TCP. Port 5001 default.

#### RV3-10 T2 — `rbmp-collector` binary

New binary in `crates/rbmp-server/src/bin/collector.rs`:
- Accepts BMP on port 5000 (as today)
- Wraps every `BmpMessage` in `CollectorEnvelope`
- Forwards to Core via TCP connection to port 5001
- Reconnects with exponential backoff if Core is unavailable
- Local ring buffer: when Core is unreachable, buffers last N messages in memory

#### RV3-10 T3 — Core listener for collector connections

In `rbmp-server/src/main.rs`, add a second TCP listener on port 5001:
- Accepts `CollectorEnvelope` frames
- Unwraps to `BmpMessage` + enriches with `collector_id` and `site` metadata
- Feeds into the same `msg_tx` channel as direct BMP connections
- All DuckDB rows include `collector_id` and `site` columns

#### RV3-10 T4 — Schema: add collector_id to all tables

```sql
ALTER TABLE route_events ADD COLUMN IF NOT EXISTS collector_id VARCHAR;
ALTER TABLE peer_events   ADD COLUMN IF NOT EXISTS collector_id VARCHAR;
ALTER TABLE speaker_events ADD COLUMN IF NOT EXISTS collector_id VARCHAR;
```

---

## Part 5 — RV3 File Change Index

### New crates

| Crate | Epic |
|-------|------|
| `crates/rbmp-kafka/` | RV3-5 |
| `crates/rbmp-mrt/` | RV3-6 |

### New Rust files

| File | Epic |
|------|------|
| `crates/rbmp-core/src/bgp/srpolicy.rs` | RV3-1 |
| `crates/rbmp-server/src/dns.rs` | RV3-4 |
| `crates/rbmp-server/src/proxy.rs` | RV3-7 |
| `crates/rbmp-server/src/filter.rs` | RV3-8 |
| `crates/rbmp-server/src/api/mrt.rs` | RV3-6 |
| `crates/rbmp-store/src/mrt_export.rs` | RV3-6 |
| `crates/rbmp-core/src/collector_protocol.rs` | RV3-10 |
| `crates/rbmp-server/src/bin/collector.rs` | RV3-10 |

### Modified Rust files

| File | Change | Epic |
|------|--------|------|
| `crates/rbmp-core/src/bgp/evpn.rs` | EVPN types 6-11 | RV3-1 |
| `crates/rbmp-core/src/bgp/bgpls.rs` | BGP-LS attribute type 29 full decoder | RV3-2 |
| `crates/rbmp-core/src/bgp/types.rs` | SR Policy Safi, VPLS, MCAST-VPN, RTC enums | RV3-1 |
| `crates/rbmp-core/src/bgp/attributes.rs` | Wire type 29 BGP-LS attr, SR Policy dispatch | RV3-1,2 |
| `crates/rbmp-core/src/bgp/mod.rs` | Add pub mod srpolicy | RV3-1 |
| `crates/rbmp-rib/src/session.rs` | LLGR state machine | RV3-9 |
| `crates/rbmp-rib/src/table.rs` | llgr_stale flag + timer fields | RV3-9 |
| `crates/rbmp-rib/src/manager.rs` | Filter engine integration, LLGR handling | RV3-8,9 |
| `crates/rbmp-store/src/schema.rs` | SR Policy table, bgpls_links/nodes, collector_id | RV3-1,2,10 |
| `crates/rbmp-store/src/writer.rs` | Write SR Policy events, BGP-LS link/node attrs | RV3-1,2 |
| `crates/rbmp-server/src/config.rs` | Kafka config, proxy config, filter config | RV3-5,7,8 |
| `crates/rbmp-server/src/main.rs` | Kafka producer spawn, proxy, filter engine, DNS, LLGR | RV3-4,5,7,8,9 |
| `crates/rbmp-server/src/receiver.rs` | Proxy forward, DNS enrichment | RV3-4,7 |
| `crates/rbmp-server/src/api/mod.rs` | Add MRT routes | RV3-6 |
| `Cargo.toml` | Add rbmp-kafka, rbmp-mrt | RV3-5,6 |

### New Python files

| File | Epic |
|------|------|
| `bmppy/rbmppy/rpki.py` | RV3-3 |
| `bmppy/rbmppy/internet.py` | RV3-3 |
| `bmppy/rbmppy/detectors.py` | RV3-3 |

---

## Part 6 — Feature Completion Matrix (Post-RV3)

| Feature | OpenBMP | Rotonda | bbmp2kafka | goBMP | rustybmp (RV3) |
|---------|---------|---------|------------|-------|----------------|
| BMP core (RFC 7854) | ✅ | ✅ | ✅ | ✅ | ✅ |
| RFC 9972 stats | ❌ | ❌ | ❌ | ❌ | ✅ |
| EVPN types 1-5 | ✅ | ? | ? | ✅ | ✅ |
| EVPN types 6-11 | ✅ | ? | ? | ✅ | **✅ RV3** |
| SR Policy SAFI 73 | ❌ | ❌ | ❌ | ✅ | **✅ RV3** |
| BGP-LS NLRI | ✅ | ? | ? | ✅ | ✅ |
| BGP-LS link attributes | ✅ Full | ? | ? | ✅ Full | **✅ RV3** |
| SR Capabilities / SRGB | ✅ | ? | ? | ✅ | **✅ RV3** |
| Flex Algorithm | ❌ | ? | ? | ✅ | **✅ RV3** |
| Route Target Constraint | ❌ | ? | ? | ✅ | **✅ RV3** |
| RPKI RTR client | ✅ | ✅ (filter) | ❌ | ❌ | ✅ |
| RPKI per-route validation | ✅ | ✅ (filter) | ❌ | ❌ | ✅ |
| Z-score anomaly detection | ❌ | ❌ | ❌ | ❌ | ✅ |
| Hijack/leak detection | Partial | ❌ | ❌ | ❌ | ✅ |
| Kafka output | ✅ Core | ❌ | ✅ Core | ✅ | **✅ RV3** |
| MRT import | ❌ | ✅ | ❌ | ❌ | **✅ RV3** |
| MRT export | ✅ | ❌ | ❌ | ❌ | **✅ RV3** |
| Proxy/intercept mode | ✅ | ❌ | ❌ | ✅ | **✅ RV3** |
| Programmable filter | ❌ | ✅ Roto | ❌ | ❌ | **✅ YAML RV3** |
| DNS PTR enrichment | ✅ | ❌ | ❌ | ❌ | **✅ RV3** |
| Embedded analytics DB | ❌ (PostgreSQL) | ❌ (in-memory) | ❌ | ❌ | ✅ DuckDB |
| Python SDK | ❌ | ❌ | ❌ | ❌ | ✅ |
| Multi-collector/site | ✅ Kafka | ✅ rotoro | ❌ | ❌ | **✅ RV3** |
| Active BGP session | ❌ | ✅ | ❌ | ❌ | ❌ (RV4) |
| BGPsec | ❌ | Planned | ❌ | ❌ | ❌ (RV4) |

---

## Part 7 — Quality Gates for RV3

```bash
# All existing tests must still pass:
cargo test --workspace

# New protocol tests:
# RV3-1 SR Policy: build SR Policy NLRI bytes for types A,B,C; verify decode
# RV3-1 EVPN types 6-8: build IGMP join/leave bytes; verify route type name
# RV3-1 RTC: decode wildcard + specific RTC NLRI
# RV3-2 BGP-LS attr: parse link attribute with IGP metric + max bandwidth + SRLG
# RV3-2 BGP-LS SR: parse node attribute with SR Capabilities SRGB range
# RV3-5 Kafka: integration test with embedded Kafka; publish route event; consume; verify JSON
# RV3-8 Filter: bogon prefix is rejected; /25 more-specific is rejected when len_gt=24
# RV3-8 Filter: short AS path triggers alert tag

# Python tests:
# RV3-3: VrpCache validates 203.0.113.0/24 AS64496 as valid, AS64497 as invalid
# RV3-3: DetectorPipeline fires alert on mock origin-change event
```

---

## Part 8 — Notes for RV4

RV4 targets:
1. **Active BGP session connector** (Rotonda-inspired) — open BGP sessions, receive full tables without BMP
2. **UI dashboard** — Svelte: live RIB table, BGP-LS topology graph, RPKI status, SR Policy paths
3. **BGPsec** — path validation using RPKI ROA + AS Path certificates
4. **HA leader election** — two-instance active/passive with route de-duplication
5. **NATS output** — lightweight alternative to Kafka for edge deployments
6. **L2VPN VPLS** full decode — legacy but still in ISP networks
7. **BGP-LS SRv6 SID NLRI** (SAFI 72) — emerging in cloud provider networks

---

*End of RUSTYBMP_BACKLOG_RV3.md — Sprint RV3*
