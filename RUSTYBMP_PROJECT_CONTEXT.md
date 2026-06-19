# RUSTYBMP — Project Context Reference
## Updated: 2026-06-19 | RV3 Sprint | Retain in Claude Project

---

## What this project is

RustyBMP: Rust-first BGP Monitoring Protocol (BMP) collector. BMP/BGP exclusive. Bonsai analysis was done once in session 1 — no need to re-upload.

**Dev cycle**: Mac → GitHub → Ubuntu 24 + XRD ContainerLab.
**Next session**: Upload `rv3_all_changes.patch` (output of `scripts/dev/make_diff.sh`).

---

## Codebase State (post-RV2, going into RV3)

### Workspace crates
```
crates/rbmp-core/       ← Parser: BMP RFC7854/8671/9069/9972 + BGP EVPN(1-5)/Flowspec/BGP-LS(NLRI)/SRv6/Prefix-SID/OTC/TunnelEncap/Add-Path
crates/rbmp-rib/        ← RIB + session state. Add-Path fully wired (path_id flows through).
crates/rbmp-store/      ← DuckDB: route_events/peer_events/speaker_events/stats_events/evpn_events/bgpls_nodes/bgpls_links
crates/rbmp-server/     ← TCP recv + archive + governor + HTTP API + Prometheus (real) + synthetic Termination + batched writes
crates/rbmp-enrichment/ ← VrpCache + EnrichmentEngine + RtrClient (RTR RFC 8210)
[RV3 NEW] crates/rbmp-kafka/   ← Kafka output producer (rdkafka)
[RV3 NEW] crates/rbmp-mrt/     ← MRT import/export (RFC 6396)
```

### Python (bmppy/rbmppy/)
- client.py, stream.py, models.py — complete
- analytics.py — ZScoreMonitor (eq.2-4), HijackDetector, RouteLeakDetector, FlapScorer, RouteAnalytics
- peering.py — live PeeringDB + Cloudflare RPKI (no cache — replaced by RV3-3)
- rpki.py, internet.py, detectors.py — NOT YET (RV3-3)

---

## RV2 Status (all complete)

All P0/P1 epics done. Deferred: rpki.py/internet.py/detectors.py Python files, LLGR state machine, distributed collector/core.

---

## RV3 Sprint Targets

| Epic | Title | Priority |
|------|-------|----------|
| RV3-1 | SR Policy NLRI (SAFI 73) + EVPN types 6-11 + RTC/MVPN | P0 |
| RV3-2 | BGP-LS link/node/prefix attribute TLVs + Flex Algo | P0 |
| RV3-3 | Python layer: rpki.py + internet.py + detectors.py | P0 |
| RV3-4 | DNS PTR enrichment for speaker hostname | P1 |
| RV3-5 | Kafka output producer (rdkafka crate) | P0 |
| RV3-6 | MRT import (RIPE RIS files) + export | P1 |
| RV3-7 | BMP proxy/intercept mode | P1 |
| RV3-8 | YAML filter engine at ingest (bogons, prefix-len, community) | P1 |
| RV3-9 | LLGR state machine (deferred from RV2) | P2 |
| RV3-10 | Distributed collector/core (deferred from RV2) | P2 |

---

## Competitor Analysis Summary

| Project | Language | Key gap vs rustybmp (what they have, we don't) |
|---------|----------|-----------------------------------------------|
| OpenBMP (SNAS) | C++ | Kafka output, MRT export, BMP proxy, BGP-LS link attrs, DNS PTR |
| Rotonda (NLnet) | Rust | Roto filter language, active BGP session, MRT import, MQTT |
| bbmp2kafka (Cloudflare) | Go | Kafka + Protobuf serialization |
| goBMP (sbezverk) | Go | SR Policy all 11 seg types, EVPN 6-11, Flex Algo, BGP-LS full TLVs, NATS |

## Where rustybmp leads everyone
- RFC 9972 stats types 18-38 (May 2026) — no one else has these
- Z-score anomaly detection + hijack/leak detection built-in
- DuckDB embedded analytics store (history + ad-hoc SQL)
- Python SDK with real analytics
- SSE real-time event stream

---

## New crates added in RV3
- `crates/rbmp-kafka/` — Kafka output (rdkafka, lz4 compression)
- `crates/rbmp-mrt/` — MRT format (RFC 6396) import/export
