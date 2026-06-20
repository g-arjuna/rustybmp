# rustybmp — Results & Decisions

> **Mother document** — updated after every epic, decision, or meaningful change.
> Companion to `RUSTYBMP_BACKLOG_RV1.md`.

---

## Session Log

### 2026-06-19 — Sprint RV1 Implementation (Session 2, continued)

**Completed**: wired `main.rs` + `receiver.rs` for archive/governor, built full Python SDK (RV1-7), created ContainerLab topology (RV1-8), ran `cargo build --workspace` — **zero errors, warnings only**.

**Compile fixes applied**:
- Added `use super::flowspec::FlowspecNlri;` in `bgp/attributes.rs` (type was used but not imported)
- Changed `TunnelEncapEntry.tunnel_type_name: &'static str` → `String` (lifetime conflict with `#[derive(Deserialize)]`)
- Updated construction site in `parse_tunnel_encap()` to call `.to_string()`

---

### 2026-06-19 — Sprint RV1 Implementation (Session 1)

**Goal**: Implement all 8 RV1 epics from scratch based on the RV1 backlog.

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | Keep `StatEntry.name` as `String` (not `&'static str`) | `&'static str` cannot be derived with `Deserialize` cleanly without lifetime annotations. The allocation overhead is negligible given stats are low-frequency. |
| D2 | Change `RibEventPayload::Stats.counters` from `Vec<(String, u64)>` to `Vec<StatEntry>` | Needed to propagate `afi_safi: Option<AfiSafi>` for RFC 9972 per-AFI/SAFI stats through to the DuckDB writer. Cleanest option. |
| D3 | Fix `on_up()` hold_time bug in manager.rs | Current code passes `pu.peer_header.peer_as as u16` as `hold_time` — clearly wrong. Fixed to `pu.recv_open.hold_time`. |
| D4 | Add-Path: add struct support (path_id field, compound key, best-path stub) but do NOT parse path_ids from NLRI yet | NLRI decoder changes are a separate, larger change. Structure is in place for RV2 NLRI decoder work. |
| D5 | `PrefixSid`, `Srv6L3Service`, `Srv6SubSubTlv`, `TunnelEncapEntry` defined in `bgp/types.rs`; parse functions in `bgp/srv6.rs` | Separates type definitions from parsing logic. Consistent with existing pattern (types in types.rs, parsing in dedicated files). |
| D6 | `EvpnRoute` defined in `bgp/evpn.rs`; `EvpnReachNlri`/`EvpnUnreachNlri` defined in `bgp/types.rs` | `EvpnRoute` is a complex enum that belongs with its parser. The NLRI wrapper structs belong in types.rs alongside `MpReachNlri`. |
| D7 | Fix main.rs event_tx wiring — expose `event_sender()` from `RibManager` | Current main.rs creates a dead broadcast channel for SSE events; SSE stream was completely non-functional. |
| D8 | `bmppy/rbmppy/analytics.py` is a rewrite of the existing stub | Existing stub used DuckDB direct queries (pandas); new version follows backlog spec with `PrefixMonitor`, `SessionFlap`, and feature extraction helpers. |

---

## Epic Status

| Epic | Title | Status | Notes |
|------|-------|--------|-------|
| RV1-1 | RFC 9972 Stats Decoder | ✅ Complete | Types 18-38 named, 11-byte per-AFI/SAFI parsed, DuckDB schema updated |
| RV1-2 | EVPN NLRI Parser | ✅ Complete | All 5 route types (1-5), evpn_events table in DuckDB |
| RV1-3 | Flowspec NLRI Parser | ✅ Complete | Types 1-12, numeric and bitmask ops |
| RV1-4 | Advanced Path Attributes | ✅ Complete | OTC (type 35), Prefix-SID (type 40), Tunnel Encap (type 23), BgpRole cap 9 |
| RV1-5 | Add-Path Aware RIB | ✅ Complete | Structure in place; NLRI path_id parsing deferred to RV2 |
| RV1-6 | Server Hardening | ✅ Complete | Archive writer, governor, fixed event_tx wiring, checkpoint task |
| RV1-7 | rbmppy Python SDK | ✅ Complete | client.py, stream.py, models.py, analytics.py, peering.py, pyproject.toml |
| RV1-8 | ContainerLab + XRD Lab | ✅ Complete | Topology, XRD/FRR configs, flap/withdrawal test scripts |

---

## Architecture Notes

### Stats flow after RV1-1
```
parse_stats_report() → Vec<StatEntry> { stat_type, name, value, afi_safi }
  → BmpPayload::StatsReport { stats: Vec<StatEntry> }
  → RibManager::process() → RibEventPayload::Stats { counters: Vec<StatEntry> }
  → run_store_writer() → stats_events (with afi, safi columns)
```

### EVPN flow after RV1-2
```
parse_mp_reach(buf) → if afi_safi == L2VPN/EVPN → decode_evpn_nlri()
  → PathAttributes.evpn_reach = Some(EvpnReachNlri { routes: Vec<EvpnRoute> })
  → RibManager emits RouteChange with EVPN attributes
  → run_store_writer() → evpn_events table
```

### Event wiring fix (RV1-6)
```
Before: main.rs created a DEAD broadcast channel for SSE — events never reached /api/events
After:  RibManager::event_sender() returns a clone of the real sender.
        main.rs captures it before Arc-wrapping RibManager.
        Both store writer and SSE handler subscribe to the same real channel.
```

---

## Known Gaps / Deferred to RV2 (resolved in RV2/RV3)

- Add-Path path_id parsing from NLRI → ✅ Done RV2
- LLGR stale tracking → ✅ Done RV3 (Bundle B)
- BGP-LS full decode → ✅ Done RV3 (Bundle A)
- Route Target ExtComm → ✅ Done RV2
- EVPN events table writer → ✅ Done RV2
- rbmppy `peering.py` stub → ✅ Done RV2 (PeeringDB + RPKI wrappers)

---

## Session Log — 2026-06-19 Sprint RV2

**Completed**: Add-Path NLRI parsing, EVPN withdraw, ExtComm/RT decode, BGP-LS NLRI scaffolding, RPKI RTR client scaffold, analytics rewrite (ZScore, HijackDetector, RouteLeakDetector, FlapScorer). 38 tests pass.

---

## Session Log — 2026-06-19 Sprint RV3 (Bundles A-G)

### Bundles Completed

| Bundle | Epic | Outcome |
|--------|------|---------|
| A | RV3-1,2 | SR Policy SAFI73 (types A-K), EVPN types 6-11, BGP-LS full TLVs, RTC SAFI132 |
| B | RV3-8,9 | YAML filter DSL, LLGR state machine (Normal/StaleMarked/Deleted) |
| C | RV3-4,7 | DNS PTR cache (TTL-bounded, OS resolver), BMP proxy (tee + upstream forward) |
| D | RV3-5   | Kafka output crate (rdkafka FutureProducer, lz4, typed topics) |
| E | RV3-6   | MRT crate (RFC 6396 reader + writer, BGP4MP + TABLE_DUMP_V2, 8 tests) |
| F | RV3-3   | Python: rpki.py (RtrVrpCache, RFC 6811), internet.py (IRR/RDAP/BGP.Tools), detectors.py (4 detectors + pipeline) |
| G | RV3-10  | Distributed: CollectorEnvelope (MessagePack), rbmp-collector binary, Core TCP listener :5001, schema collector_id |

**Final test count**: 49 Rust tests, 0 failures.

#### Decisions Made (RV3)

| # | Decision | Rationale |
|---|----------|-----------|
| D9 | `body_len` in MRT writer must include AFI u16 (was missing +2) | Root cause of all reader test failures — MRT header declared 2 fewer bytes than were written, causing parse under-reads. Fixed in `write_bgp4mp_message` and `write_bgp4mp_state_change`. |
| D10 | Collector protocol uses `rmp-serde` (MessagePack) over raw TCP with 4-byte BE length prefix | Compact binary, self-describing, zero-copy decode with `rmp_serde::from_slice`. Simpler than Protobuf for this use case. Max frame 8 MiB. |
| D11 | `rbmp-collector` uses `try_send()` to a bounded `mpsc::channel` as ring buffer | Non-blocking drop on overflow is correct for an edge collector — it is better to lose PDUs than to back-pressure the BMP TCP session and cause router disconnect. |
| D12 | `handle_collector_conn` re-parses raw BMP bytes on the Core side | Core always re-parses; collector only frames+forwards. This keeps the collector binary minimal and avoids sending structs over the wire. |
| D13 | `detectors.py` extracts `origin_as` by scanning the last integer token in `RouteEvent.as_path` | `RouteEvent` has `as_path: Optional[str]` (space-separated), not a structured list. `_origin_as()` and `_as_path_list()` helpers added. |
| D14 | `RtrVrpCache` uses sorted in-process list; validation via linear scan | VRP tables are ~400K entries; linear scan is O(n) but fast enough for alert pipelines. Production upgrade: use an interval tree if needed. |

---

## Files Changed — RV2

### Modified files
- `crates/rbmp-core/src/bgp/types.rs` — Add-Path struct, ExtComm types, BGP-LS NLRI stubs
- `crates/rbmp-core/src/bgp/update.rs` — path_id parsing from NLRI
- `crates/rbmp-core/src/bgp/attributes.rs` — ExtComm full decode, RT community
- `crates/rbmp-core/src/bgp/bgpls.rs` — NLRI type scaffolding
- `crates/rbmp-enrichment/src/rtr.rs` — RTR client scaffold
- `bmppy/rbmppy/analytics.py` — ZScoreMonitor, HijackDetector, RouteLeakDetector, FlapScorer

---

## Files Changed — RV3

### New crates
- `crates/rbmp-kafka/` — Kafka output (producer, sink, topics, error)
- `crates/rbmp-mrt/` — MRT import/export (types, reader, writer, error)

### New Rust files
- `crates/rbmp-core/src/bgp/srpolicy.rs` — SR Policy NLRI SAFI 73
- `crates/rbmp-core/src/collector_protocol.rs` — MessagePack framing protocol
- `crates/rbmp-server/src/dns.rs` — DNS PTR cache
- `crates/rbmp-server/src/proxy.rs` — BMP proxy
- `crates/rbmp-server/src/bin/collector.rs` — `rbmp-collector` edge binary

### New Python files
- `bmppy/rbmppy/rpki.py` — RtrVrpCache, RFC 6811 validation, poll_rtr_cache()
- `bmppy/rbmppy/internet.py` — IrrClient, RdapClient, BgpToolsClient, resolve_origin()
- `bmppy/rbmppy/detectors.py` — OriginChangeDetector, RouteLeakDetector, MEDOscillationDetector, BGPHijackDetector, DetectorPipeline

### Modified Rust files
- `Cargo.toml` — rdkafka, rmp-serde workspace deps; rbmp-kafka, rbmp-mrt members
- `crates/rbmp-core/Cargo.toml` — added rmp-serde, tokio
- `crates/rbmp-core/src/lib.rs` — pub mod collector_protocol
- `crates/rbmp-core/src/bgp/evpn.rs` — EVPN types 6-11
- `crates/rbmp-core/src/bgp/bgpls.rs` — full link/node/prefix attribute TLVs
- `crates/rbmp-core/src/bgp/types.rs` — SR Policy Safi, RTC, LLGR state
- `crates/rbmp-core/src/bgp/attributes.rs` — wire type 29 BGP-LS, SR Policy dispatch
- `crates/rbmp-rib/src/manager.rs` — filter engine, LLGR handling
- `crates/rbmp-rib/src/session.rs` — LLGR state machine
- `crates/rbmp-store/src/schema.rs` — collector_id in route_events/peer_events/speaker_events
- `crates/rbmp-server/Cargo.toml` — rbmp-kafka dep, rbmp-collector [[bin]]
- `crates/rbmp-server/src/config.rs` — KafkaConfig, DnsConfig, ProxyConfig
- `crates/rbmp-server/src/main.rs` — Kafka sink, DNS, proxy, collector listener
- `crates/rbmp-server/src/receiver.rs` — DNS PTR lookup on connect
- `bmppy/rbmppy/__init__.py` — export rpki/internet/detectors symbols

---

## Known Gaps / Deferred to RV4

- Add-Path path_id parsing from NLRI (NLRI decoder needs changes)
- LLGR stale tracking (capability parsed, state machine not yet)
- BGP-LS full decode (stub AFI only — type decode deferred)
- Route Target extended community full decode (partial — basic RT shown)
- EVPN events table not written in writer.rs yet (schema created, writer TODO marked)
- rbmppy `peering.py` is a stub (PeeringDB + RPKI wrappers for RV2)

---

---

## Session Log — 2026-06-20 Sprint RV6

### Goal
UI completeness · Roto-level filter language scaffold · Protocol completeness (ASPA, BGPsec, MCAST-VPN) · Comprehensive quality gate (0 cargo warnings, 0 npm errors).

### Bundles Completed

| Bundle | Epic | Outcome |
|--------|------|---------|
| RV6-1 | Filter Engine | `filter_reload`/`filter_test`/`filter_stats` endpoints; `RouteCtx` + `roto_ctx.rs` scaffold for future Roto JIT embed; `config/filters.yaml` default |
| RV6-2 | Protocol | ASPA (RFC 9319) validate_as_path + unit tests; MCAST-VPN full RFC 6514 types 1-7 (`bgp/mvpn.rs`); BGPsec_Path parse (RFC 8205 type 30); SRv6 uSID scaffold |
| RV6-3 | UI Components | `TimelineChart.svelte` (D3 area/line), `AsnSankey.svelte` (d3-sankey), `RpkiBadge.svelte`, `VirtualTable.svelte` (virtual-scroll), `MetricCard.svelte`, `sse.ts` (RAF batching + reconnect) |
| RV6-4 | Schema/Store | `srpolicy_events`, `aspa_validations` tables; composite indexes; `aspath_graph()`, `bmpstats_history()`, `srpolicy_current()`, `ml_anomalies_recent()` queries |
| RV6-5 | API | 18 new endpoints: `aspath_graph`, `bmpstats_history`, `srpolicy_list/by_peer`, `peer_capabilities`, `rpki_coverage`, `bgpls_path`, `ml_model_status`, filter CRUD; `onboard` wizard 4 steps |
| RV6-6 | UI Pages | 4 new pages: `/filters`, `/srpolicy`, `/bgpls-path`, `/rpki-coverage`; upgraded `/aspath` (Sankey+MetricCards), `/ml` (model status+severity), `/stats` (history+MetricCards), `/peers/[addr]` ($derived fix), `+page.svelte` (typed API unwrapping) |
| RV6-7 | Quality Gate | `cargo build --workspace` 0 warnings (18 files fixed); `npm run check` 0 errors (60→0: `@types/node`, vite.config, `$:` → `$derived`, fx/fy types, string|undefined params, API response types) |

**Final test count**: 77 Rust tests, 0 failures.

#### Decisions Made (RV6)

| # | Decision | Rationale |
|---|----------|-----------|
| D15 | Scaffold `RouteCtx` + `roto_ctx.rs` but do NOT embed Roto crate yet | Roto v0.11 (cranelift JIT) API is still stabilising; embedding it would add build-time complexity and potential breaking changes before RV7. The scaffold gives operators the full RouteCtx shape to write filters against. |
| D16 | Keep YAML filter DSL alongside RouteCtx scaffold | Operators already have working YAML filters; removing them before Roto embed would break existing deployments. Both coexist until RV7 cuts over. |
| D17 | `filter_reload` Axum handler: `spawn_blocking` + explicit `drop(RwLockWriteGuard)` | Root cause: `RwLockWriteGuard` held across `.await` (not `Send`). `spawn_blocking` for file I/O avoids blocking the async runtime; explicit `drop` before the `Ok(...)` return prevents the guard from being held when the future is polled again. |
| D18 | `AsnSankey` uses `(sankey as any)()` + `sankeyLinkHorizontal() as any` | `d3-sankey` generic type constraints are overly restrictive for our pre-indexed node pattern. The `as any` casts are isolated to the D3 call sites — component inputs/outputs are still fully typed. |
| D19 | All runes-mode Svelte pages use `$derived` (not `$:`) | `$:` is forbidden in Svelte 5 runes mode. Pages using `$state` must use `$derived`/`$effect` for reactivity. Non-runes pages (`srpolicy`) correctly use `$:`. |
| D20 | Install `@types/node` in UI devDependencies | Eliminates 24+ `Buffer`/`node:*` errors from vite/sveltekit internals that svelte-check traverses. Standard practice for SvelteKit projects. |
| D21 | `vite.config.ts` import: `@sveltejs/kit/vite` not `@sveltejs/vite-plugin-svelte` | `sveltekit()` is exported from `@sveltejs/kit/vite`. The wrong import source caused a TS error in svelte-check even though vite itself resolved it at runtime. |
| D22 | Topology `N` type: add `fx?: number \| null; fy?: number \| null` | D3 drag pinning requires setting `fx`/`fy` on force simulation nodes. TypeScript rightly rejects unknown properties — the type annotation is the correct fix. |

---

## Epic Status (cumulative)

| Epic | Title | Status | Notes |
|------|-------|--------|-------|
| RV1-1 | RFC 9972 Stats Decoder | ✅ | Types 18-38 named, 11-byte per-AFI/SAFI |
| RV1-2 | EVPN NLRI Parser | ✅ | All 11 route types |
| RV1-3 | Flowspec NLRI Parser | ✅ | Types 1-12, numeric + bitmask |
| RV1-4 | Advanced Path Attributes | ✅ | OTC, Prefix-SID, Tunnel Encap, BGP Role |
| RV1-5 | Add-Path Aware RIB | ✅ | NLRI path_id parsing done RV2 |
| RV1-6 | Server Hardening | ✅ | Archive, governor, event_tx wiring |
| RV1-7 | rbmppy Python SDK | ✅ | client, stream, models, analytics, peering |
| RV1-8 | ContainerLab + XRD Lab | ✅ | Topology, configs, test scripts |
| RV2-* | Protocol depth | ✅ | Add-Path, EVPN withdraw, ExtComm, BGP-LS |
| RV3-* | Integration | ✅ | SR Policy, LLGR, Kafka, MRT, distributed |
| RV4-* | Scale + UI foundation | ✅ | SvelteKit scaffold, 11 pages, HA, TLS |
| RV5-* | UI wiring + API depth | ✅ | Prefix explorer, peer detail, RPKI, ML schema |
| RV6-1 | Filter Engine (YAML + Roto scaffold) | ✅ | Hot-reload, test, stats, RouteCtx |
| RV6-2 | Protocol (ASPA, BGPsec, MCAST-VPN) | ✅ | RFC 9319, 8205, 6514 |
| RV6-3 | UI Component Library | ✅ | TimelineChart, AsnSankey, VirtualTable, MetricCard, RpkiBadge, sse.ts |
| RV6-4 | DuckDB Schema + Queries | ✅ | srpolicy_events, aspa_validations, indexes, query methods |
| RV6-5 | API Completions | ✅ | 18 new endpoints |
| RV6-6 | UI Pages (9 complete) | ✅ | All 15 nav pages functional |
| RV6-7 | Quality Gate | ✅ | 0 cargo warnings, 0 npm errors, 77 tests |

---

## Files Changed — RV6

### New Rust files
- `crates/rbmp-core/src/bgp/mvpn.rs` — MCAST-VPN full RFC 6514 types 1-7
- `crates/rbmp-rib/src/roto_ctx.rs` — RouteCtx scaffold + Roto runtime builder
- `crates/rbmp-enrichment/src/aspa.rs` — ASPA RFC 9319 validation
- `crates/rbmp-server/src/api/filters.rs` — filter_reload, filter_test, filter_stats
- `crates/rbmp-server/src/api/analytics.rs` — aspath_graph, bmpstats_history
- `crates/rbmp-server/src/api/ml.rs` — ml_anomalies, ml_model_status
- `crates/rbmp-server/src/api/onboard.rs` — 4-step onboarding wizard

### Modified Rust files
- `crates/rbmp-core/src/bgp/attributes.rs` — BGPsec_Path (type 30) dispatch; unused import/constant fixes
- `crates/rbmp-core/src/bgp/types.rs` — MvpnNlri enum; unused import fix
- `crates/rbmp-core/src/bgp/update.rs` — unused import fix
- `crates/rbmp-core/src/bgp/srv6.rs` — unused import fix
- `crates/rbmp-core/src/bgp/open.rs` — unused variable fix
- `crates/rbmp-core/src/bgp/srpolicy.rs` — unused variable fix
- `crates/rbmp-core/src/bmp/parser.rs` — unused import + variable fix
- `crates/rbmp-core/src/collector_protocol.rs` — unused import fix
- `crates/rbmp-rib/src/filter.rs` — unused import fix
- `crates/rbmp-rib/src/manager.rs` — unused import fix
- `crates/rbmp-store/src/schema.rs` — srpolicy_events, aspa_validations tables, composite indexes
- `crates/rbmp-store/src/query.rs` — aspath_graph(), bmpstats_history(), srpolicy_current(), ml_anomalies_recent()
- `crates/rbmp-store/src/duck.rs` — unused import fix
- `crates/rbmp-enrichment/src/vrp_cache.rs` — unused import fix
- `crates/rbmp-enrichment/src/rtr.rs` — unused import fix
- `crates/rbmp-enrichment/src/annotate.rs` — unused import fix
- `crates/rbmp-server/src/api/mod.rs` — register filter/ml/onboard routes; unused import fix
- `crates/rbmp-server/src/api/routes.rs` — unused import fix
- `crates/rbmp-server/src/api/peers.rs` — peer_capabilities endpoint
- `crates/rbmp-server/src/api/stats.rs` — bmpstats_history endpoint
- `crates/rbmp-server/src/api/topology.rs` — bgpls_path, srpolicy_list
- `crates/rbmp-server/src/bin/collector.rs` — unused import fix
- `crates/rbmp-server/src/ha.rs` — deprecated get_async_connection → get_multiplexed_async_connection
- `crates/rbmp-server/src/dns.rs` — allow(dead_code) on cache_size utility
- `crates/rbmp-server/src/auth.rs` — removed unused ErrorBody struct
- `crates/rbmp-mrt/src/writer.rs` — removed SystemTime import + unreachable let binding

### New UI files
- `ui/src/lib/TimelineChart.svelte` — D3 area/line time-series (static imports)
- `ui/src/lib/AsnSankey.svelte` — D3 Sankey (d3-sankey, string IDs)
- `ui/src/lib/RpkiBadge.svelte` — colored validity pill
- `ui/src/lib/VirtualTable.svelte` — virtual-scroll table (Svelte 5 runes)
- `ui/src/lib/MetricCard.svelte` — stat card with optional trend
- `ui/src/lib/sse.ts` — RAF-batched SSE client with auto-reconnect
- `ui/src/routes/filters/+page.svelte` — filter test + reload + stats
- `ui/src/routes/srpolicy/+page.svelte` — SR Policy list (MetricCards + VirtualTable)
- `ui/src/routes/bgpls-path/+page.svelte` — BGP-LS shortest path computation
- `ui/src/routes/rpki-coverage/+page.svelte` — ROA coverage analysis

### Modified UI files
- `ui/src/lib/api.ts` — 8 new API methods (asPathGraph, srpolicyList, peerCapabilities, filterReload, filterStats, filterTest, rpkiCoverage, bgplsPath, mlModelStatus, bmpstatsHistory)
- `ui/src/routes/+layout.svelte` — 4 new nav items + RV6 badge
- `ui/src/routes/+page.svelte` — typed API unwrapping for peers/speakers
- `ui/src/routes/aspath/+page.svelte` — AsnSankey + MetricCards; $derived fix
- `ui/src/routes/ml/+page.svelte` — model status panel; $derived fix
- `ui/src/routes/stats/+page.svelte` — history API + MetricCards; $derived fix
- `ui/src/routes/peers/[addr]/+page.svelte` — $: → $derived; route param non-null
- `ui/src/routes/policy/+page.svelte` — $derived type annotation fix
- `ui/src/routes/prefix/[prefix]/+page.svelte` — route param non-null
- `ui/src/routes/topology/+page.svelte` — N type fx/fy fields
- `ui/vite.config.ts` — correct sveltekit import source
- `ui/package.json` — d3-sankey, @types/d3, @types/d3-sankey, @types/node

---

## Files Changed — RV1

### New files
- `crates/rbmp-core/src/bgp/evpn.rs`
- `crates/rbmp-core/src/bgp/flowspec.rs`
- `crates/rbmp-core/src/bgp/srv6.rs`
- `crates/rbmp-server/src/archive.rs`
- `crates/rbmp-server/src/governor.rs`
- `bmppy/rbmppy/__init__.py`
- `bmppy/rbmppy/client.py`
- `bmppy/rbmppy/stream.py`
- `bmppy/rbmppy/models.py`
- `bmppy/rbmppy/analytics.py` (rewrite)
- `bmppy/rbmppy/peering.py`
- `bmppy/pyproject.toml`
- `lab/xrd-bmp.clab.yml`
- `lab/configs/xrd/pe1.cfg`
- `lab/configs/xrd/pe2.cfg`
- `lab/configs/frr/frr.conf`
- `lab/configs/frr/daemons`
- `lab/scenarios/flap_peer.sh`
- `lab/scenarios/mass_withdrawal.sh`
- `scripts/dev/make_diff.sh`

### Modified files
- `crates/rbmp-core/src/bmp/types.rs`
- `crates/rbmp-core/src/bmp/parser.rs`
- `crates/rbmp-core/src/bgp/types.rs`
- `crates/rbmp-core/src/bgp/attributes.rs`
- `crates/rbmp-core/src/bgp/capabilities.rs`
- `crates/rbmp-core/src/bgp/mod.rs`
- `crates/rbmp-rib/src/event.rs`
- `crates/rbmp-rib/src/session.rs`
- `crates/rbmp-rib/src/table.rs`
- `crates/rbmp-rib/src/manager.rs`
- `crates/rbmp-store/src/schema.rs`
- `crates/rbmp-store/src/writer.rs`
- `crates/rbmp-server/src/main.rs`
- `crates/rbmp-server/src/receiver.rs`
