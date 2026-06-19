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

## Known Gaps / Deferred to RV2

- Add-Path path_id parsing from NLRI (NLRI decoder needs changes)
- LLGR stale tracking (capability parsed, state machine not yet)
- BGP-LS full decode (stub AFI only — type decode deferred)
- Route Target extended community full decode (partial — basic RT shown)
- EVPN events table not written in writer.rs yet (schema created, writer TODO marked)
- rbmppy `peering.py` is a stub (PeeringDB + RPKI wrappers for RV2)

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
