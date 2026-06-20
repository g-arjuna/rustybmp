# RUSTYBMP — Project Context Reference
## Updated: 2026-06-20 | RV4 Sprint | Retain in Claude Project

---

## What this project is

RustyBMP: Rust-first BGP Monitoring Protocol (BMP) collector. BMP/BGP exclusive.
Dev cycle: Mac → GitHub → Ubuntu 24 + XRD ContainerLab.
Next session: Upload rv4_all_changes.patch.

---

## Codebase State (post-RV3, going into RV4)

### Workspace (7 crates, 49 tests passing)
```
rbmp-core         — BMP RFC7854+8671+9069+9972 + BGP EVPN(1-11)+SR Policy+BGP-LS(full TLVs)+Flowspec+RTC
rbmp-rib          — RIB + LLGR state machine (Normal/StaleMarked/Deleted) + YAML filter DSL + session
rbmp-store        — DuckDB: route_events/peer_events/speaker_events/stats_events/evpn_events; collector_id column; 500-msg batched writes
rbmp-server       — TCP recv + archive + governor + HTTP API + Prometheus + Kafka sink + DNS PTR + BMP proxy + Core collector listener (:5001)
rbmp-enrichment   — VrpCache + RtrClient (RTR RFC 8210) + EnrichmentEngine
rbmp-kafka        — Kafka output (rdkafka FutureProducer, lz4, typed topics)
rbmp-mrt          — MRT RFC 6396 reader + writer (BGP4MP + TABLE_DUMP_V2, 8 tests)
```

### Python (bmppy/rbmppy/) — complete
- client.py, stream.py, models.py — full HTTP + SSE client
- analytics.py — ZScoreMonitor (IMACSI 2025 eq.2-4), HijackDetector, RouteLeakDetector, FlapScorer
- rpki.py — RtrVrpCache, RFC 6810 validation, poll_rtr_cache()
- internet.py — IrrClient, RdapClient, BgpToolsClient, resolve_origin()
- detectors.py — 4 detectors + DetectorPipeline

### Distributed deployment
- rbmp-collector binary: edge BMP→Core forwarder (MessagePack, 4-byte length, 8MiB max, try_send ring buffer)
- Core listener: :5001 TCP for CollectorEnvelope frames
- All events tagged with collector_id

---

## RV3 Key Decisions (reference)
- D9: MRT body_len +2 bug caught by tests
- D10: MessagePack over TCP (simpler than Protobuf, self-describing)
- D11: try_send() drop at collector (don't back-pressure BMP TCP session)
- D12: Core re-parses raw BMP bytes (collector is just a framer)
- D13: origin_as = last integer in as_path string (known limitation with AS_SETs)
- D14: Linear VRP scan O(n) adequate at 400K VRPs

---

## Bonsai Architecture Questions — Final Answers

### Graph DB (KuzuDB): NOT for rustybmp
Bonsai needs graph DB because it has physical topology from LLDP+gNMI. RustyBMP is BMP-only.
BGP-LS data in DuckDB bgpls_nodes/bgpls_links is the topology store.
Solution: Python NetworkX graph derived from DuckDB (rbmppy/topology.py, RV4-6).

### Parquet for ML: YES — high value, trivially implemented
DuckDB exports Parquet natively (single SQL statement).
Adapt bonsai's ML pipeline:
- parquet.py: DuckDB → Parquet export
- ml/train_route_anomaly.py: IsolationForest on route features (adapted from bonsai train_anomaly.py)
- ml/topology_snapshot.py: BGP peer graph snapshots for STGNN (adapted from bonsai snapshot_store.py)
RV4-4 epic covers this.

---

## RV4 Sprint Targets

| Epic | Title | Priority |
|------|-------|----------|
| RV4-1 | Security: JWT auth, TLS, token bucket rate limiting | P0 |
| RV4-2 | DuckDB retention policy + Parquet export API | P0 |
| RV4-3 | Svelte 5 UI dashboard | P1 |
| RV4-4 | ML pipeline: Parquet export + IsolationForest + STGNN prep | P1 |
| RV4-5 | Protocol: SRv6 SID NLRI (SAFI 72), VPLS, OTC wiring | P1 |
| RV4-6 | BGP topology graph: NetworkX from BGP-LS DuckDB | P1 |
| RV4-7 | HA leader election + NATS output | P2 |
| RV4-8 | Dockerfile + docker-compose + GitHub Actions CI | P0 |
| RV4-9 | Integration tests + cargo bench + Ubuntu testing runbook | P0 |

## Uncovered README areas (genuinely still missing)
1. API authentication — NO AUTH currently (critical gap)
2. TLS for BMP connections — plaintext only
3. DuckDB retention — grows forever
4. UI dashboard — zero frontend code
5. Integration tests — unit tests only
6. CI/CD pipeline
7. Dockerfile
8. Parquet ML pipeline
9. BGP topology graph (NetworkX)
10. SRv6 SID NLRI SAFI 72
