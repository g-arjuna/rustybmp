# RUSTYBMP — Project Context Reference
## Updated: 2026-06-22 | RV9 Sprint | Retain in Claude Project

---

## Current State: RV9 complete (101 Python tests passing, 0 build errors, 26 Playwright tests)

### Crates (8 total)
```
rbmp-core         — RFC7854+8671+9069+9972+EVPN(1-11)+SR Policy+BGP-LS+Flowspec+RTC+
                    SRv6 uSID+VPLS+MCAST-VPN RFC6514(1-7)+BGPsec_Path parse(type30)+ASPA
rbmp-rib          — RIB+LLGR+YAML filter DSL+filter_expr+RouteCtx scaffold (Roto embed RV7)
rbmp-store        — DuckDB+retention+Parquet+srpolicy_events+aspa_validations+composite indexes
rbmp-server       — JWT auth+TLS+HA+Kafka+DNS+Proxy+Collector+18 new API endpoints in RV6
rbmp-enrichment   — RPKI RTR client+VrpCache+ASPA validate_as_path
rbmp-kafka        — Kafka output (rdkafka, lz4)
rbmp-mrt          — MRT RFC 6396
rbmp-nats         — NATS output
```

### Python (bmppy/)
- rbmppy: client, stream, models, analytics, rpki, internet, detectors, parquet, topology
- ml: train_route_anomaly (IsolationForest), topology_snapshot, parquet_store

### UI (SvelteKit, 15+ pages)
ALL PAGES IMPLEMENTED in RV6:
Dashboard, Peers, Peer Detail, Prefixes, Prefix Explorer, Topology, AS Paths,
RPKI, RPKI Coverage, Policy, SR Policy, BGP-LS Path, Filters, Onboarding,
ML Insights, BMP Stats, Alerts

Components: TimelineChart (D3), AsnSankey (d3-sankey), VirtualTable, MetricCard, RpkiBadge
SSE client: sse.ts with RAF batching + exponential-backoff reconnect

---

## RV6 Key Decisions to Remember
- D15: RouteCtx scaffold committed, Roto JIT embed DEFERRED to RV7
- D16: YAML DSL kept alongside RouteCtx through RV7 cutover
- D17: filter_reload uses spawn_blocking + explicit RwLockWriteGuard drop
- D18: AsnSankey uses (sankey as any)() — d3-sankey type constraints too restrictive
- D19: All runes-mode pages use $derived (not $:)
- D20: @types/node installed in UI devDependencies
- D21: vite.config uses @sveltejs/kit/vite not @sveltejs/vite-plugin-svelte
- D22: topology N type has fx?: number|null; fy?: number|null for drag pinning

---

## RV7 Themes

### Theme 1: Roto JIT (P0)
- RouteCtx scaffold in roto_ctx.rs READY — just wire roto = "0.11" crate
- build_roto_runtime() → register all fields + community_has/as_path_contains/prefix_in_range
- RotoFilterEngine: load() JIT-compiles via cranelift, reload() hot-reloads
- config/filters.roto: default bogon+RPKI+OTC+blackhole filter
- filter_watcher.rs: inotify → trigger reload on file change
- YAML DSL: keep as fallback with deprecation warning

### Theme 2: Path Status TLV (P0)
- draft-ietf-grow-bmp-path-marking-tlv-05 (May 2026)
- 12 status bits: Invalid/Best/Nonselected/Primary/Backup/Non-installed/
  Best-external/Add-Path/Filtered-inbound/Filtered-outbound/Stale/Suppressed
- 11 reason codes: 0x0001-0x000B
- New: path_markings DuckDB table (status, reason, status_label, reason_label)
- TLV type code is IANA TBD — make configurable in config
- UI: Path Pipeline (horizontal stages per prefix)
- UI: Redundancy Health Matrix (prefix × peer grid, filter by <2 active paths)
- UI: Max-prefix Fuel Gauge (RFC 9972 type 30 + linear regression ETA)

### Theme 3: SSH Policy Fetch (P0)
- vault.rs: COPY from bonsai credentials.rs, rename env var, add SshFetch
- policy_fetch_handler: same spawn pattern as bonsai bootstrap_device_handler
- Credentials as env vars ONLY (never CLI args or HTTP body)
- policy_fetcher.py: focused subset of bonsai bootstrap_agent.py
  - Genie testbed for Cisco/Junos/Arista (same testbed_dict construction)
  - Paramiko for Nokia SRL/FRR (same fallback pattern)
  - 3 show commands per vendor (policy structure + statistics + neighbor)
- rbmppy/policy/: 5-tier parser ecosystem (Genie/Batfish/OpenConfig/TextFSM/BMP)
- policy_configs DuckDB table
- UI: credential manager + "Fetch from Router" button on /policy page

### Theme 4: BGPsec Full Validation (P2)
- Parse done in RV6 (type 30 attribute, raw signature blocks stored)
- RV7 adds: router cert fetch + ECDSA P-256 via ring crate
- bgpsec_validations DuckDB table

### Theme 5: Topology Scale + Convergence (P1-P2)
- Adaptive rendering: force (<100) / hierarchical (100-1000) / clustered (>1000)
- BGP convergence event detection (PeerDown→withdrawals→EOR→measurement)
- convergence_events DuckDB table + /api/convergence endpoint

---

## Protocol Coverage After RV7
Complete: RFC7854, 8671, 9069, 9972, 7432 EVPN, 5575 Flowspec, 7752 BGP-LS,
6514 MCAST-VPN, 8205 BGPsec-parse, 9319 ASPA, 9514 SRv6, 4761 VPLS,
5549 unnumbered, draft-bmp-path-marking-tlv-05 (NEW RV7)
RV7 adds full BGPsec validation

## RV9 New Modules
- `bmppy/rbmppy/acl_generator.py` — 4-vendor ACL/prefix-list/null-route generator (IOS-XR/FRR/JunOS/Arista)
- `bmppy/rbmppy/policy_advisor.py` — rule-based filter gap analysis, RPKI/ASPA heuristics, Roto snippet output
- `grafana/rustybmp-dashboard.json` — 11-panel Grafana 10+ dashboard (import-ready JSON)
- `tests/scenarios/01_frr_minimal/` — ContainerLab Tier 0 FRR smoke test
- `tests/scenarios/02_xrd_rfc9972/` — ContainerLab Tier 1 XRd RFC 9972 stats test
- `ui/tests/rustybmp.spec.ts` — 26 Playwright E2E tests with mock API interception
- `docs/CODEX_TESTING.md` — 7-layer test runbook (replaces UBUNTU_TESTING.md)

## RV9 UI Pages Added
- `/communities` — community frequency table, inferred semantics, filter
- `/flowspec` — FlowSpec rules viewer, speaker filter, large-prefix alert
- `/vrf` — VRF/RD explorer, per-VRF route table
- `/query` — NL query page (example chips, SQL preview, results table)
- `/adapters` — output adapter management (health, event counts, test-connection)

## Upload pattern
Diff written: diffs/rv9/rv9_all_changes.patch (17,397 lines)
