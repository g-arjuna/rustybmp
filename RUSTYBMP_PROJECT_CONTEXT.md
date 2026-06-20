# RUSTYBMP — Project Context Reference
## Updated: 2026-06-20 | RV5 Sprint | Retain in Claude Project

---

## Project status: RV4 complete (7 crates, Svelte UI, ML pipeline, full security)

### Workspace crates (post-RV4)
```
rbmp-core         — BMP RFC7854+8671+9069+9972 + BGP EVPN(1-11)+SR Policy+BGP-LS full+Flowspec+RTC+SRv6+VPLS
rbmp-rib          — RIB + LLGR + YAML filter DSL (expression parser in RV5)
rbmp-store        — DuckDB + retention (hourly sweep) + Parquet export API
rbmp-server       — TCP recv + JWT auth + TLS + HA (Redis) + Kafka + DNS + Proxy + Collector listener
rbmp-enrichment   — RTR RPKI client (VrpCache)
rbmp-kafka        — Kafka output (rdkafka, lz4)
rbmp-mrt          — MRT RFC 6396 reader/writer
rbmp-nats         — NATS output (async-nats)
```

### Python (bmppy/)
- rbmppy: client, stream, models, analytics (Z-score+hijack+leak+flap), rpki, internet, detectors, parquet, topology
- ml: train_route_anomaly (IsolationForest), topology_snapshot (to_pyg stub), parquet_store

### UI (Svelte 5, 5 pages)
- Dashboard: stat cards + live SSE feed
- Peers: table with search
- Prefixes: live route table
- Topology: D3 BGP-LS force-directed graph
- Alerts: SSE-backed alert list

---

## Confirmed out of scope (RV6)
- Active BGP session connector (requires full BGP FSM, cross-project with Rotonda)
- BGPsec path validation (requires RPKI router certs + per-UPDATE crypto)
- MCAST-VPN full RFC 6514 type decode (stub arm in bgp/types.rs preserves raw bytes)

---

## RV5 priorities

### P0 — Filter expression language (closes Rotonda gap)
- `filter_expr.rs`: Expr AST + RouteCtx + eval()
- `filter.pest`: PEG grammar via pest crate
- YAML `expr:` field: `"rpki == 'invalid' AND prefix_len > 24"`
- Hot-reload via inotify (notify crate)

### P0 — ML completions
- `topology_snapshot.to_pyg()` — HeteroData edge tensors
- `train_bgp_stgnn.py` — GATv2-GRU STGNN (NEW)
- `export_prefix_aggregates()` — windowed aggregation in parquet.py
- Fix IsolationForest: use per-prefix aggregated features, not per-event rows
- `ml_anomalies` DuckDB table + API + UI page

### P0 — UI: Prefix Explorer
- `/ui/src/routes/prefix/[prefix]/+page.svelte`
- Announcement timeline + AS path per peer + event history + enrichment

### P1 — Device Onboarding Wizard
- Register speaker + BMP config snippet generator (XRD, Junos, SRL, FRR)
- BMP connection test endpoint
- EOR onboarding progress tracker

### P1 — Missing UI pages
- AS Path Visualizer (D3 DAG)
- RPKI Analysis page
- Policy Analysis (pre vs post-policy diff)
- Peer health timeline
- BMP Stats viewer (RFC 9972)
- SR Policy view

### P2 — Operational improvements
- SR Policy events DuckDB table (was deferred from RV3-1)
- Topology graph TTL cache (60s)
- AsTopology using distinct AS pairs query

---

## RV4 decisions to remember
- JWT HS256, config-gated (disabled by default)
- TLS via rustls + tokio-rustls, build_acceptor() returns Option<TlsAcceptor>
- HA: Redis SETNX, AtomicBool shared leader flag, always-leader when disabled
- Retention: hourly sweep, skips first tick (no sweep on startup)
- Parquet export: ALLOWED_TABLES whitelist prevents SQL injection

## Filter gap vs Rotonda
- Our YAML DSL: linear scan, fixed predicates, no expressions
- Our RV5 addition: PEG-parsed expressions via pest (AND/OR/NOT, comparisons, IN sets)
- Rotonda Roto: compiled to machine code, more powerful, still no loops
- Gap remaining after RV5: multi-RIB routing (different RIBs per filter result) — RV6
