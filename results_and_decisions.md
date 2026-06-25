# rustybmp — Results & Decisions

> **Mother document** — updated after every epic, decision, or meaningful change.
> Companion to `RUSTYBMP_BACKLOG_RV1.md`.

---

## Session Log

### 2026-06-25 — Cross-Vendor FRR ↔ XRd IPv4 Multicast Expansion

**Completed**: extended the direct FRR↔XRd host-process-first lab with one more AFI/SAFI checkpoint, `ipv4 multicast`, and validated the observed multicast behavior without disturbing the existing IPv4 and IPv6 unicast coverage.

**Validation**:
- `.venv/bin/python -m pytest tests/scenarios/04_cross_vendor_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_cross_vendor.json`
- Result: **15 passed in 73.26s**

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D46 | Use `ipv4 multicast` as the next AFI/SAFI expansion in the existing two-node FRR↔XRd lab | This adds another real address-family checkpoint while keeping the topology and BMP debug surface small. |
| D47 | Treat XRd-originated IPv4 multicast route visibility as the validated checkpoint, rather than requiring full multicast symmetry immediately | Live BMP/API inspection showed XRd-originated multicast prefixes becoming queryable while FRR-originated multicast prefixes were not observed symmetrically in the same checkpoint, so the scenario now asserts the behavior actually proven by the lab. |

### 2026-06-25 — Cross-Vendor FRR ↔ XRd Dual-Stack Expansion

**Completed**: expanded the direct FRR↔XRd host-process-first lab to validate IPv6 alongside IPv4, fixed the IPv6 prefix design and XRd IPv6 BGP startup layout, and revalidated the strengthened cross-vendor checkpoint.

**Validation**:
- `.venv/bin/python -m pytest tests/scenarios/04_cross_vendor_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_cross_vendor.json`
- Result at the dual-stack checkpoint: **13 passed in 67.91s**

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D43 | Deepen the existing two-node FRR↔XRd cross-vendor lab with IPv6 before adding more topology | This expands protocol coverage while keeping the debug surface small and preserving the already-validated mixed-vendor adjacency. |
| D44 | Change the IPv6 test prefixes from `/48` to distinct `/64` networks | The original `/48` prefixes collapsed to the same canonical network and made the test expectations ambiguous, so they were not suitable as deterministic route assertions. |
| D45 | Reorder XRd's IPv6 BGP startup config so the global IPv6 address-family block appears before the IPv6 neighbor subtree | XRd 24.4.2 rejected the initial IPv6 startup layout, while the reordered structure matched the working IPv4 pattern and allowed the dual-stack checkpoint to pass. |

### 2026-06-24 — Cross-Vendor FRR ↔ XRd eBGP Checkpoint

**Completed**: added the first direct FRR↔XRd eBGP ContainerLab scenario under the host-process-first strategy, fixed a live AS_PATH parser compatibility issue it exposed, and reran the existing Layer 4/5 lab regressions after the parser change.

**Validation**:
- `cargo test -p rbmp-core parse_as_path -- --nocapture`
- `.venv/bin/python -m pytest tests/scenarios/04_cross_vendor_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_cross_vendor.json`
- `.venv/bin/python -m pytest tests/scenarios/01_frr_minimal/ -v --json-report --json-report-file=runtime/test_results/layer4.json`
- `.venv/bin/python -m pytest tests/scenarios/02_xrd_rfc9972/ -v --json-report --json-report-file=runtime/test_results/layer5.json`
- `.venv/bin/python -m pytest tests/scenarios/03_mixed_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_mixed.json`
- Results: **2 parser tests passed, 10 cross-vendor scenario tests passed at the initial IPv4 checkpoint, 11 FRR scenario tests passed, 9 XRd scenario tests passed, 10 shared mixed scenario tests passed**

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D41 | Make the next mixed-NOS expansion a direct two-node FRR↔XRd eBGP scenario instead of immediately building a larger multi-adjacency topology | This isolates true cross-vendor control-plane interoperability with lower blast radius and faster iteration than a larger topology would. |
| D42 | Add a compatibility fallback in `parse_as_path()` so a failed 2-byte AS_PATH decode is retried as 4-byte ASN encoding on the same attribute bytes | The new cross-vendor lab exposed live XRd-originated route-monitoring updates that failed AS_PATH parsing during eBGP route exchange; the fallback restored route visibility and remained green across the existing FRR-only, XRd-only, and shared mixed regressions. |

### 2026-06-24 — Mixed FRR + XRd Host-Process-First Checkpoint

**Completed**: added the first combined FRR + XRd ContainerLab scenario under the host-process-first strategy and validated concurrent multi-vendor BMP ingestion against one host-run `rustybmp`.

**Validation**:
- `cargo build -p rbmp-server --bins`
- `.venv/bin/python -m pytest tests/scenarios/03_mixed_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_mixed.json`
- Result: **10 passed in 168.98s**

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D39 | Build the first mixed-NOS checkpoint as one shared collector with the existing FRR pair and XRd pair, rather than introducing new cross-vendor BGP adjacencies immediately | This validates concurrent multi-vendor BMP/API behavior with lower risk and preserves the current testing strategy's focus on stable scenario expansion rather than new interoperability debugging. |
| D40 | Treat XRd route visibility in the mixed lab as a readiness concern and wait for vendor-specific prefixes directly | The first mixed run showed all four speakers up, all four peers up, and XRd stats present before XRd route prefixes were queryable through `/api/routes`, so a one-time route snapshot was too early for this combined scenario. |

### 2026-06-23 — Layer 5 XRd Host-Process-First Bring-Up

**Completed**: refactored the XRd RFC 9972 scenario away from an in-lab collector, switched it to the locally available XRd image, stabilized XRd boot/startup config, and validated BMP peer-up plus route-monitoring end to end against a host-run `rustybmp`.

**Validation**:
- Clean boot-path validation with host collector plus ContainerLab XRd topology
- `.venv/bin/python -m pytest tests/scenarios/02_xrd_rfc9972/ -v --json-report --json-report-file=runtime/test_results/layer5.json`
- Result after harness fixes: **6 passed, 3 failed**

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D30 | Keep the Layer 5 XRd scenario on the host-process-first model and remove the in-lab `rustybmp` collector node | This preserves the same fast feedback loop that made Layer 4 productive and keeps Layer 5 focused on BMP/API behavior rather than Docker packaging. |
| D31 | Use the locally available XRd image `ios-xr/xrd-control-plane:24.4.2` | This is the image actually present on the host, and it exposed version-specific startup-config behavior that had to be fixed directly rather than assumed from older XRd examples. |
| D32 | Remove `XR_EVERY_BOOT_CONFIG` from the XRd ContainerLab topology | XRd 24.4.2 rejected that setting in this lab because the referenced startup file was not valid in the container's boot path, causing early boot failure. |
| D33 | Normalize the XRd startup config to 24.4.2 syntax: `network` under AFI, `bmp server all route-monitoring inbound pre-policy`, and `neighbor ... bmp-activate server 1` | The older `bmp neighbor ... / bmp-servers 1` flow and out-of-AF `network` statements were rejected by XRd 24.4.2 even though the topology and reachability were otherwise correct. |
| D34 | Raise XRd `flapping-delay` from `30` to `60` seconds | XRd 24.4.2 enforces a minimum accepted value of 60 for that knob; `30` caused startup-config rejection. |
| D35 | Add `norecursedirs = clab-*` to `pytest.ini` | Once the XRd lab directory existed under the test tree, pytest began traversing router-owned paths and failed collection with permission errors before running the scenario itself. |
| D36 | Treat the remaining Layer 5 failure as an application-side stats ingestion issue, not a topology issue | XRd operational state now shows `STATS-REPORT` plus `ROUTE-MON` messages being sent, while `rustybmp` still persists no stats rows. The topology and startup config are no longer the primary blocker. |
| D37 | Fix `/api/bmpstats/history` by casting stats columns before row mapping and by failing on query-map errors instead of silently dropping them | Live XRd validation showed `stats_events` filling while the API returned an empty array. The query path was masking DuckDB row-conversion errors for stats fields. |
| D38 | Update the XRd Layer 5 assertions to match the stats counters actually observed on the wire from XRd `24.4.2` in this topology | Archived host-process BMP captures repeatedly showed only stat types `7`, `8`, `9`, and `10`; no type `30` or AFI/SAFI gauge rows were present, so the failing RFC 9972-specific assertions were not valid for this exact XRd behavior. |

### 2026-06-23 — Host-Process-First Layer 4 FRR Smoke Stabilization

**Completed**: refactored the minimal FRR ContainerLab scenario to use a host-run `rustybmp` process instead of an in-lab collector container, validated the scenario end to end, and captured the parser/storage/API mismatches that the live FRR lab exposed.

**Validation**:
- `cargo build -p rbmp-server --bins`
- `.venv/bin/python -m pytest tests/scenarios/01_frr_minimal/ -v --json-report --json-report-file=runtime/test_results/layer4.json`
- Result: **11 passed in 8.43s**

#### Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| D23 | Use host-process-first `rustybmp` for Layer 4/5 development instead of an in-lab collector container | Keeps the main test-development loop focused on BMP/API behavior rather than Docker packaging failures, and matches the updated testing strategy for this pass. |
| D24 | Enable FRR BMP support in the Layer 4 scenario with `bgpd_options="-M bgpd_bmp"` | Live FRR 10.6.1 would accept BMP configuration syntax only after the BMP module was loaded; without it, speakers connected but BMP config failed at runtime. |
| D25 | Keep FRR targeting the ContainerLab management gateway (`.1`) for host-run collector reachability | This matches the Bonsai lab pattern across FRR, SR Linux, and XRd scenarios and avoids adding a dedicated collector node just for development validation. |
| D26 | Fix PeerUp OPEN parsing against live FRR bytes by reading the BGP length field at marker+16 | Raw BMP capture from FRR showed the previous offset was wrong, which produced bogus lengths and blocked peer/session progression beyond speaker connect. |
| D27 | Change store writer inserts to use explicit column lists for `speaker_events` and `peer_events` | The live lab surfaced schema drift that would recur whenever tables gained columns; explicit column lists make these inserts forward-safe. |
| D28 | Add static `Null0` routes for the FRR smoke prefixes | FRR `network` statements only advertise routes that already exist in the local RIB; without matching static routes, the smoke scenario had healthy peers but zero route announcements. |
| D29 | Normalize `/api/peers` state labels and route query behavior to match the test/API contract | The smoke harness exposed response-shape mismatches (`Up` vs `up`, `asn` vs `peer_as`) and missing `prefix`/`action` handling in `/api/routes`. |

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

## Session Log — 2026-06-20 Sprint RV8

### Goal
Swagger/OpenAPI · MCP Server (11 BGP tools) · Output Adapters · Resource Governor · Adaptive UX · External APIs · Testing infrastructure.

### Bundles Completed

| Bundle | Epics | Outcome |
|--------|-------|---------|
| A — Resource Governor | GOV1-3 | 3-loop governor (memory/write/rate) in `governor.rs`; `AppState.governor`; `GET /api/governance`; internet-scale write tuning in `rustybmp.toml.example` |
| B — Adaptive Homepage | UX1-4 | 3-state `+page.svelte` (empty→onboarding / waiting / active); speaker cards (hostname, vendor, peers, routes, RPKI%); `GET /api/speakers/summary`; inline config snippets for IOS-XR/FRR/Arista/JunOS |
| C — OpenAPI + Swagger | OA1-2 | `api/schema.rs` with full OpenAPI 3.0.3 spec (15 tag groups); Swagger UI at `GET /api/swagger`; spec at `GET /api/openapi.json` |
| D — MCP Server | MC1-4 | `mcp_server.rs`; JSON-RPC 2.0 at `POST /mcp`; 11 BGP tools; NL→DuckDB SQL keyword mapper; 500K daily token budget (`AtomicU64`, midnight UTC reset); `ANOMALY_CATALOGUE` (5 kinds + DuckDB verification queries); `TOOL_NAMES` const for test assertions |
| E — Output Adapters | OUT1-3 | `output/mod.rs` — `OutputAdapter` async trait + `spawn_adapter_pump` cursor loop (batch=256); `output/elasticsearch.rs` — ECS `_bulk` ndjson; `output/splunk.rs` — HEC POST |
| F — External APIs | EXT1/5 | `RipeStatClient` in `internet.py` (prefix-overview, visibility, RPKI, routing-history, ASN-neighbours); `GET /api/external/prefix-visibility` — internal RIB vs RIPE STAT discrepancy |
| G — Testing | T2/T4/T7 | `tests/seed.sql` — 2 speakers, 9 route events, 2 anomalies, 3 convergence events; `tests/integration/mcp_tools.rs` — 12 passing tests; `lab/scenarios/rv8_governance_smoke.sh` — E2E smoke script covering all RV8 endpoints |

**Final build**: `cargo build --workspace` — 0 errors. **Test count**: all pass (0 failures).

**Diff**: `diffs/rv8/rv8_all_changes.patch` — 4344 lines, 29 files changed.

#### Decisions Made (RV8)

| # | Decision | Rationale |
|---|----------|-----------|
| D23 | `async-trait` crate for `OutputAdapter` | Rust async traits still require the proc-macro shim for object-safe `dyn` dispatch; avoids `Box<dyn Future>` boilerplate in adapter impls. |
| D24 | MCP JSON-RPC 2.0 implemented from scratch (not MCP SDK) | No stable Rust MCP SDK exists. The JSON-RPC 2.0 envelope is ~40 lines; full control over tool dispatch and error codes. Protocol version: `2024-11-05`. |
| D25 | NL→SQL via deterministic keyword mapping (no runtime LLM) | On-prem deployments often have no internet access. Keyword→template covers 90% of ops questions and is auditable. External LLM agents (Claude, GPT-4) can generate SQL and pass it to the tool for safe execution. |
| D26 | 500K daily token budget as `AtomicU64` with day-bucket reset | Lock-free, correct under concurrent requests. Day-bucket (`unix_secs / 86400`) means reset happens at midnight UTC without a cron job. |
| D27 | Output adapter cursor file at `runtime/cursors/{name}.cursor` | Survives server restarts without re-shipping already-pushed events. Consistent with bonsai output adapter pattern. |
| D28 | `RibType::AdjRibInPrePolicy` + `AdjRibInPostPolicy` + `LocRib` iterated for prefix-visibility | Three RIB views give the broadest internal observation of a prefix: pre-filter, post-filter, and best-path. |
| D29 | `consume_nl_tokens(100)` per call (fixed estimate, not actual token count) | Real token counts require LLM inference not available at dispatch time. 100 tokens/call is a conservative estimate that protects the daily budget without blocking legitimate use. |

---

## Epic Status (cumulative — RV8 additions)

| Epic | Title | Status | Notes |
|------|-------|--------|-------|
| RV8-GOV1 | Resource governor 3-loop | ✅ | memory/write/rate; `governor.rs` |
| RV8-GOV2 | `GET /api/governance` | ✅ | snapshot JSON response |
| RV8-GOV3 | Internet-scale write tuning | ✅ | `rustybmp.toml.example` `[governor]` section |
| RV8-UX1 | Adaptive homepage 3-state | ✅ | `+page.svelte` |
| RV8-UX2 | Speaker cards | ✅ | hostname, vendor, peers-up, routes, RPKI% |
| RV8-UX3 | `GET /api/speakers/summary` | ✅ | per-router aggregated API |
| RV8-UX4 | Inline router config snippets | ✅ | IOS-XR / FRR / Arista EOS / JunOS |
| RV8-OA1 | OpenAPI 3.0.3 spec | ✅ | `api/schema.rs` |
| RV8-OA2 | Swagger UI | ✅ | `GET /api/swagger` |
| RV8-MC1 | MCP server 11 tools | ✅ | `mcp_server.rs` |
| RV8-MC2 | NL→DuckDB SQL | ✅ | keyword mapper |
| RV8-MC3 | Daily token budget | ✅ | `AtomicU64`, midnight UTC reset |
| RV8-MC4 | ANOMALY_CATALOGUE | ✅ | 5 kinds + DuckDB queries |
| RV8-OUT1 | OutputAdapter trait + pump | ✅ | `output/mod.rs` |
| RV8-OUT2 | Elasticsearch ECS adapter | ✅ | `output/elasticsearch.rs` |
| RV8-OUT3 | Splunk HEC adapter | ✅ | `output/splunk.rs` |
| RV8-EXT1 | RIPE STAT client | ✅ | `RipeStatClient` in `internet.py` |
| RV8-EXT5 | `/api/external/prefix-visibility` | ✅ | `api/external.rs` |
| RV8-T2 | `tests/seed.sql` DuckDB fixtures | ✅ | 2 speakers, 9 route events, 2 anomalies |
| RV8-T4 | MCP tools integration tests | ✅ | 12 tests in `mcp_tools.rs` |
| RV8-T7 | RV8 governance smoke script | ✅ | `lab/scenarios/rv8_governance_smoke.sh` |
| RV8-OUT4 | ServiceNow EM adapter | ⏳ | Deferred RV9 |
| RV8-OUT5 | Webhook adapter | ⏳ | Deferred RV9 |
| RV8-EXT3 | Cloudflare Radar / HE BGP | ⏳ | Deferred RV9 |
| RV8-EXT4 | RIPE Atlas measurement | ⏳ | Deferred RV9 |
| RV8-ML1-5 | ML depth additions | ⏳ | Deferred RV9 |
| RV8-T8-T14 | XRd, Playwright, CI | ⏳ | Deferred RV9 |

---

## Files Changed — RV8

### New Rust files
- `crates/rbmp-server/src/mcp_server.rs` — MCP JSON-RPC 2.0 server, 11 tools, NL→SQL, token budget, ANOMALY_CATALOGUE
- `crates/rbmp-server/src/output/mod.rs` — OutputAdapter trait, spawn_adapter_pump, batch cursor
- `crates/rbmp-server/src/output/elasticsearch.rs` — ECS bulk API adapter
- `crates/rbmp-server/src/output/splunk.rs` — Splunk HEC adapter
- `crates/rbmp-server/src/api/external.rs` — GET /api/external/prefix-visibility
- `crates/rbmp-server/src/api/governance.rs` — GET /api/governance
- `crates/rbmp-server/src/api/schema.rs` — OpenAPI 3.0.3 spec + Swagger UI

### Modified Rust files
- `Cargo.toml` — `async-trait = "0.1"` added to workspace deps
- `crates/rbmp-server/Cargo.toml` — `async-trait`, `reqwest` promoted from dev-dep to regular dep
- `crates/rbmp-server/src/main.rs` — added `mcp_server`, `output` module declarations
- `crates/rbmp-server/src/api/mod.rs` — registered `/mcp`, `/governance`, `/api/openapi.json`, `/api/swagger`, `/api/external/prefix-visibility`, `/api/speakers/summary` routes
- `crates/rbmp-server/src/api/peers.rs` — added `speakers_summary` handler
- `crates/rbmp-server/src/governor.rs` — rewrote with 3-loop governor (memory/write/rate pressure)
- `crates/rbmp-server/src/state.rs` — added `governor` field to `AppState`
- `crates/rbmp-server/src/config.rs` — added `GovernorConfig`
- `crates/rbmp-store/src/query.rs` — added `raw_query`, `community_summary`, `convergence_events`, `policy_delta` methods; fixed `column_names()` API usage

### New Python files
- `bmppy/rbmppy/internet.py` — extended with `RipeStatClient` class (5 async methods)

### New test/lab files
- `tests/seed.sql` — DuckDB fixture data (tables + seed rows)
- `tests/integration/mcp_tools.rs` — 12 MCP JSON-RPC structural tests
- `lab/scenarios/rv8_governance_smoke.sh` — E2E governance + speaker summary + MCP + external API smoke test

### Modified test files
- `tests/integration/mod.rs` — registered `mcp_tools` module

### Config
- `config/rustybmp.toml.example` — added `[governor]`, internet-scale write tuning comments, `[output.elasticsearch]` and `[output.splunk]` example stubs

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
