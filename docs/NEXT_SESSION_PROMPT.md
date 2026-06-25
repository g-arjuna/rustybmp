# Next Session Prompt

Use this to resume RustyBMP testing from the current checkpoint:

```text
Continue RustyBMP testing from the latest main checkpoint after Layer 4 and Layer 5 turned green.

First read:
- docs/TESTING_PROGRESS.md
- docs/CODEX_TESTING.md
- RUSTYBMP_TESTING_STRATEGY.md
- results_and_decisions.md
- docs/NEXT_SESSION_PROMPT.md

Important strategy:
- Keep the host-process-first approach for Layer 4 and Layer 5.
- Do not spend time on Docker image build/debug for rustybmp yet.
- Run rustybmp directly as a host process on Ubuntu.
- Use ContainerLab only for router nodes.
- Defer in-lab rustybmp container validation and Docker packaging fixes until after the major testing pass is stable.

Current validated checkpoints:
- Layer 4 FRR smoke is green:
  - cargo build -p rbmp-server --bins
  - .venv/bin/python -m pytest tests/scenarios/01_frr_minimal/ -v --json-report --json-report-file=runtime/test_results/layer4.json
  - Result: 11 passed
- Layer 5 XRd host-process-first topology is green:
  - XRd image in use: ios-xr/xrd-control-plane:24.4.2
  - XRd boot is stable
  - BGP is up
  - BMP peer-up works
  - BMP route-monitoring exports routes
  - BMP stats are visible via /api/bmpstats/history
  - Scenario result:
    - .venv/bin/python -m pytest tests/scenarios/02_xrd_rfc9972/ -v --json-report --json-report-file=runtime/test_results/layer5.json
    - Result: 9 passed
- Layer 5 mixed FRR + XRd host-process-first topology is green:
  - FRR image: quay.io/frrouting/frr:10.6.1
  - XRd image: ios-xr/xrd-control-plane:24.4.2
  - host-run rustybmp receives concurrent BMP from all 4 routers
  - /api/speakers shows 4 speakers
  - /api/peers shows 4 peers up
  - /api/routes exposes FRR and XRd announced prefixes
  - /api/bmpstats/history shows XRd stats in the mixed lab
  - Scenario result:
    - .venv/bin/python -m pytest tests/scenarios/03_mixed_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_mixed.json
    - Result: 10 passed
- Layer 5 cross-vendor FRR ↔ XRd host-process-first topology is green:
  - FRR image: quay.io/frrouting/frr:10.6.1
  - XRd image: ios-xr/xrd-control-plane:24.4.2
  - direct FRR↔XRd eBGP adjacency is up
  - host-run rustybmp receives BMP from both routers
  - /api/speakers shows both vendors connected
  - /api/peers shows the cross-vendor peer state up
  - /api/routes exposes FRR and XRd announced prefixes for IPv4 unicast and IPv6 unicast
  - /api/routes also exposes XRd-originated IPv4 multicast prefixes in the same direct cross-vendor lab
  - /api/bmpstats/history shows XRd stats in the direct cross-vendor lab
  - Scenario result:
    - .venv/bin/python -m pytest tests/scenarios/04_cross_vendor_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_cross_vendor.json
    - Result: 15 passed

Important clarifications:
- FRR and XRd have each been validated in separate scenarios.
- A combined mixed FRR + XRd topology has now been built and validated in this testing pass.
- The current mixed topology uses separate FRR and XRd pairings under one shared host collector; it does not yet introduce cross-vendor BGP adjacencies.
- A direct cross-vendor FRR↔XRd BGP adjacency has now also been built and validated in this testing pass.
- That direct cross-vendor FRR↔XRd checkpoint now covers:
  - IPv4 unicast route visibility
  - IPv6 unicast route visibility
  - XRd-originated IPv4 multicast route visibility
- XRd 24.4.2 in this exact topology/config was observed sending legacy/global BMP stats counters (types 7, 8, 9, 10).
- No type 30 or AFI/SAFI RFC 9972 gauge rows were observed on the wire for this checkpoint.

What was fixed in the last session:
- /api/bmpstats/history had a query-layer bug in crates/rbmp-store/src/query.rs:
  - stats rows existed in stats_events
  - the API still returned []
  - row-mapping errors for unsigned DuckDB fields were being silently dropped
- The query now casts stat_type / afi / safi before mapping and fails loudly instead of discarding rows.
- The XRd Layer 5 test now waits for stats readiness before asserting, which avoids racing the first 30s stats-reporting interval.
- The direct FRR↔XRd cross-vendor lab exposed a live AS_PATH parser compatibility issue in `crates/rbmp-core/src/bgp/attributes.rs`:
  - the direct eBGP peer came up
  - BMP speakers and peers were visible
  - route-monitoring updates with non-empty XRd AS_PATHs could fail decoding
- `parse_as_path()` now retries the same attribute bytes as 4-byte ASN encoding when 2-byte decoding fails.
- After that parser fix, the direct cross-vendor lab passed and the existing FRR-only, XRd-only, and shared mixed lab regressions stayed green.
- The first IPv6 expansion attempt of the direct cross-vendor lab found two config/test issues:
  - `/48` IPv6 test prefixes were too coarse and collapsed multiple intended routes into the same canonical network
  - XRd 24.4.2 rejected the first IPv6 BGP startup layout until the global IPv6 address-family block was ordered before the IPv6 neighbor subtree
- The direct cross-vendor lab now uses distinct `/64` IPv6 prefixes and passes as a dual-stack checkpoint.
- The next AFI/SAFI expansion on the same lab used `ipv4 multicast`:
  - XRd-originated multicast prefixes became queryable through BMP/API
  - FRR-originated multicast prefixes were not observed symmetrically in the same checkpoint
- The direct cross-vendor lab now passes with multicast assertions aligned to the behavior that was actually validated.

Suggested next steps:
1. Keep the current Layer 4 and Layer 5 checkpoints green by rerunning them if more changes touch BMP/API/store/parser code:
   - cargo build -p rbmp-server --bins
   - .venv/bin/python -m pytest tests/scenarios/01_frr_minimal/ -v --json-report --json-report-file=runtime/test_results/layer4.json
   - .venv/bin/python -m pytest tests/scenarios/02_xrd_rfc9972/ -v --json-report --json-report-file=runtime/test_results/layer5.json
   - .venv/bin/python -m pytest tests/scenarios/03_mixed_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_mixed.json
   - .venv/bin/python -m pytest tests/scenarios/04_cross_vendor_frr_xrd/ -v --json-report --json-report-file=runtime/test_results/layer5_cross_vendor.json
2. Decide the next testing expansion:
   - broaden Layer 5 to another NOS/topology checkpoint, or
   - move upward to deferred packaging/in-lab rustybmp validation, or
   - treat symmetric FRR-originated IPv4 multicast visibility as a separate capability investigation
3. If XRd RFC 9972 type 30 / AFI-SAFI gauge validation is still desired, treat it as a separate capability investigation:
   - confirm whether a different XRd config or image version actually emits those counters
   - do not assume the current two-node topology should produce them

Be careful not to revert unrelated local changes if the worktree is dirty.
```
