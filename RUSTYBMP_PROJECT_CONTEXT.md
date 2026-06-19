# RUSTYBMP — Project Context Reference
## Updated: 2026-06-19 | RV2 Sprint | Retain in Claude Project

---

## What this project is

RustyBMP is a Rust-first BGP Monitoring Protocol (BMP) collector. Exclusive focus on BMP/BGP. Sister project "bonsai" (broader telemetry) was analysed once in session 1 — no need to re-upload.

**Dev cycle**: Mac (VSCode/rust-analyzer) → GitHub → Ubuntu 24 (run + test) + XRD containers via ContainerLab.  
**Next session**: Upload `rv2_all_changes.patch` (output of `scripts/dev/make_diff.sh`) to generate RV3 backlog.

---

## Codebase State (post-RV1, going into RV2)

### Workspace crates

```
crates/rbmp-core/    ← BMP/BGP pure parser. RFC 7854/8671/9069/9972/7432/5575/8669/9234/9012
crates/rbmp-rib/     ← In-memory RIB + session state. path_id struct ready (always None — RV2-1)
crates/rbmp-store/   ← DuckDB: route_events/peer_events/speaker_events/stats_events/evpn_events
crates/rbmp-server/  ← TCP receiver + archive + governor + HTTP API. Event_tx wiring fixed.
[RV2 NEW] crates/rbmp-enrichment/  ← RTR/RPKI, PeeringDB, Speaker registry
```

### Python (bmppy/rbmppy/)
- client.py, stream.py, models.py — complete
- analytics.py — SQL bug fixed only; Z-score/hijack detection NOT YET (→ RV2-5 full rewrite)
- peering.py — live HTTP, no cache (→ RV2-3/4 replaces with rpki.py + internet.py)

---

## RV1 Epic Status (all complete)

All 8 RV1 epics done. Key bugs fixed: hold_time passing (D3), SSE dead channel (D7), StatEntry lifetime (D1).

Known deferred from RV1:
- Add-Path path_id in NLRI (struct ready, parsing deferred → RV2-1)
- LLGR state machine (capability parsed only → RV2-9)
- BGP-LS full decode (stub AFI → RV2-2)
- analytics.py Z-score model (→ RV2-5)

---

## RV2 Sprint Targets (priority order)

P0 — Critical:
- RV2-1: Add-Path NLRI path_id parsing + EVPN withdraw + ExtComm full decode
- RV2-2: BGP-LS NLRI (RFC 7752) — Node/Link/Prefix
- RV2-3: RPKI — RTR (RFC 8210) client + VrpCache + per-route annotation + rbmppy/rpki.py
- RV2-5: Analytics Engine — Z-score model (paper eq.2-4) + hijack/leak/flap + analytics.py rewrite
- RV2-6: Receiver supervisor — synthetic Termination on TCP drop (stale routes fix)

P1 — Important:
- RV2-4: PeeringDB + RIPE STAT caching client (rbmppy/internet.py)
- RV2-7: Write coordinator — batched DuckDB inserts (target: 1500 msg/sec)
- RV2-8: Speaker registry — IP→hostname/vendor/site in config + API responses

P2 — Architecture:
- RV2-9: LLGR state machine
- RV2-10: Prometheus metrics (per-speaker gauges, RPKI counters, BMP stat passthrough)
- RV2-11: Distributed collector/core scaffold (rbmp-collector + rbmp-core-service binaries)

---

## Research reference (IMACSI 2025, Hiremath)

Z-score model: μPi, σPi over sliding window; Zi = (f-μ)/σ; alert if |Zi|>3.
Origin AS change without prior withdraw = hijack signal.
Performance targets: 97.2% accuracy, 2.1% FPR, <700ms latency, 1500 msg/sec.

---

## Bonsai patterns still pending

Receiver supervisor, write coordinator, speaker registry → RV2.
Event bus, topology graph, HA coordinator → RV3.
Bonsai repo NOT needed again.
