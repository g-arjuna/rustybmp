# RUSTYBMP — Project Context Reference
## Saved: 2026-06-18 | Retain in Claude Project

---

## What this project is

RustyBMP is a Rust-first BGP Monitoring Protocol (BMP) collector. Exclusive focus on BMP/BGP — not gNMI, not syslog. The sister project "bonsai" is a broader telemetry tool that treats BMP as one of many inputs.

**Repo structure**: Mac (VSCode) → GitHub → Ubuntu 24 (test + run) + XRD containers via ContainerLab.

---

## Current codebase state (as of RV1 backlog creation)

### Working (compilable, tested)

- `crates/rbmp-core/`: Pure BMP/BGP parser. RFC 7854 (all 7 BMP msg types), RFC 8671, RFC 9069. BGP UPDATE, OPEN, capabilities. IPv4/IPv6/VPN/Labeled NLRI. Path attributes 1-18, 32.
- `crates/rbmp-rib/`: In-memory RIB (DashMap). Per-peer tables. Session state machine. RibEvent broadcast.
- `crates/rbmp-store/`: DuckDB persistence. 4 tables: route_events, peer_events, speaker_events, stats_events.
- `crates/rbmp-server/`: Axum HTTP API. BMP TCP receiver (frame-based). Config (TOML). Axum router with 10 endpoints.
- `bmppy/`: Stub only — just `__init__.py`, no working code yet.

### Key design decisions already made

- Workspace: Cargo resolver 2, edition 2024, rust-version 1.85
- Tokio broadcast channel for events (16384 capacity)
- DuckDB bundled (no external DuckDB install needed)
- HTTP on :7878, BMP on :5000
- UUIDs (v4) on every message and event
- Axum 0.8 + Tower for HTTP
- Tracing + tracing-subscriber for logging

---

## RV1 Sprint targets (current sprint)

Epic RV1-1: RFC 9972 stats (May 2026) — stat types 18-38, 11-byte per-AFI/SAFI format  
Epic RV1-2: EVPN NLRI (RFC 7432) — all 5 route types  
Epic RV1-3: Flowspec NLRI (RFC 5575/8955) — component parser  
Epic RV1-4: Advanced path attrs — Prefix-SID (RFC 8669), OTC (RFC 9234), Tunnel Encap (RFC 9012)  
Epic RV1-5: Add-Path aware RIB (RFC 7911) — path_id compound key, best-path selection stub  
Epic RV1-6: Server hardening — archive writer, back-pressure governor, event_tx fix, DuckDB checkpoint  
Epic RV1-7: rbmppy Python SDK v1 — client.py, stream.py, models.py, analytics.py  
Epic RV1-8: ContainerLab + XRD test infrastructure — topologies, configs, scenario scripts  

---

## How sessions work

1. This project retains this context file + the latest backlog file
2. After coding on Mac: run `scripts/dev/make_diff.sh` to generate a `.patch` file
3. Upload the `.patch` file to Claude (not the full repo)
4. Claude reads the diff, marks completed tasks ✅, identifies failures, generates RUSTYBMP_BACKLOG_RV2.md

---

## Key RFC reference

| RFC | Feature | Implementation status |
|-----|---------|----------------------|
| RFC 7854 | BMP core | ✅ Done |
| RFC 8671 | Adj-RIB-Out | ✅ Done |
| RFC 9069 | Loc-RIB | ✅ Done |
| RFC 9972 | Advanced stats May 2026 | 🔲 RV1-1 |
| RFC 7432 | EVPN NLRI | 🔲 RV1-2 |
| RFC 5575 | Flowspec | 🔲 RV1-3 |
| RFC 8669 | Prefix-SID | 🔲 RV1-4 |
| RFC 9234 | OTC / BGP Role | 🔲 RV1-4 |
| RFC 7911 | Add-Path RIB | 🔲 RV1-5 |
| RFC 7752 | BGP-LS | 🔲 RV2 |
| RFC 8210 | RPKI RTR | 🔲 RV2 |

---

## Architecture: planned new crates

```
rbmp-enrichment  (RV2) — RPKI/ROA, PeeringDB, IRR, RIPE STAT
rbmp-analytics   (RV2) — churn detection, hijack/leak scoring
```

## bonsai extraction completed

The following was extracted from bonsai (one-time analysis done RV1):
- Resource governor pattern (shed_stats_on_pressure)
- Archive JSONL pattern (append-only raw PDU capture)
- XRD BMP config template (lab/sp/configs/xrd/PE1.cfg)
- Python BGP rule patterns (BgpSessionDown, BgpSessionFlap, BgpAllPeersDown)
- ContainerLab topology patterns
- Backlog file format and structure

Bonsai repo does NOT need to be re-uploaded. All relevant patterns are documented in RUSTYBMP_BACKLOG_RV1.md.
