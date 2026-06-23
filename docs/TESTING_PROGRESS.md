# RustyBMP Testing Progress

## 2026-06-23 — Layer 4 FRR smoke stabilized with host-process-first flow

Summary:
- Refactored `tests/scenarios/01_frr_minimal/` to remove the in-lab `rustybmp:latest` collector dependency during the main testing pass.
- Switched the FRR smoke scenario to start `rustybmp` as a host process and target the host via the ContainerLab management gateway.
- Verified the Layer 4 scenario passes end to end with `.venv/bin/python -m pytest tests/scenarios/01_frr_minimal/ -v --json-report --json-report-file=runtime/test_results/layer4.json`.

Key fixes discovered through the FRR lab:
- FRR BMP support was not active in the scenario daemon config; enabling `bgpd_bmp` was required for live BMP sessions.
- The FRR scenario prefixes were configured with `network ...` statements but had no matching local routes, so FRR never originated them until static `Null0` routes were added.
- `rbmp-core` had a live-FRR PeerUp OPEN length parsing bug that misread the BGP length field offset.
- `rbmp-store` still had schema drift in writer inserts for `speaker_events` and `peer_events`.
- `/api/peers` and `/api/routes` had response/query mismatches that the smoke harness exposed once BMP sessions became healthy.

Repo changes in this checkpoint:
- `crates/rbmp-core/src/bmp/parser.rs`
- `crates/rbmp-server/src/api/peers.rs`
- `crates/rbmp-server/src/api/routes.rs`
- `crates/rbmp-store/src/writer.rs`
- `tests/scenarios/01_frr_minimal/configs/daemons`
- `tests/scenarios/01_frr_minimal/configs/frr1.conf`
- `tests/scenarios/01_frr_minimal/configs/frr2.conf`
- `tests/scenarios/01_frr_minimal/configs/rustybmp.toml`
- `tests/scenarios/01_frr_minimal/test_frr_smoke.py`
- `tests/scenarios/01_frr_minimal/topology.clab.yml`

Validation result:
- Layer 4 FRR smoke: `11 passed in 8.43s`
- Artifact: `runtime/test_results/layer4.json`

Next recommended steps:
1. Apply the same host-process-first pattern to `tests/scenarios/02_xrd_rfc9972/`.
2. Keep Docker image build/debug deferred until Layer 4/5 behavior is stable.
3. After XRd is adapted and validated, return to the in-lab collector image as a final packaging gate.

## 2026-06-23 — Lab strategy pivot before Layer 4/5 execution

Summary:
- Re-read the testing runbook and resumed the Layer 4 FRR smoke checkpoint.
- Verified the missing scenario `rustybmp.toml` files and topology bind-path fixes were in place.
- Confirmed host build still passes with `cargo build -p rbmp-server --bins`.
- Attempted `docker build -t rustybmp:latest .` for the in-lab collector image and hit a builder-stage toolchain gap from `rdkafka-sys` (`c++` and make-program not available in the image build environment).
- Decided to pivot the documented test strategy: during the main Layer 4/5 test-development pass, run `rustybmp` directly as a host process on Ubuntu and keep ContainerLab limited to router nodes. Defer image build/debug to a final packaging-validation pass.

Why this pivot:
- It removes image-build latency from the main testing loop.
- It keeps scenario work focused on BMP/API behavior instead of Docker packaging issues.
- It still preserves a later end-to-end image validation step once the lab scenarios are substantially stable.

Docs updated for next session:
- `docs/CODEX_TESTING.md` now describes Layer 4 and Layer 5 as host-process-first flows.
- `RUSTYBMP_TESTING_STRATEGY.md` now treats containerized `rustybmp` as a final validation phase rather than the default development loop.

## 2026-06-22 — Ubuntu preliminary testing after RV9

Environment:
- Host: Ubuntu Linux
- Workspace: `/home/arjuna/rustybmp`
- Python: `3.12.3`
- Test venv: `.venv`

Summary:
- Layer 1 wiring checks: passed
- Layer 2 protocol integration: passed
- Layer 3 ML tests: passed
- Layer 3 API contract tests: passed after fixture and query fixes
- Layer 7 UI / Playwright: passed on Chromium after UI and test harness fixes

Artifacts:
- `runtime/test_results/layer2.json`
- `runtime/test_results/layer3_api.json`
- `ui/test-results/results.json`
- `ui/playwright-report/index.html`

Commands validated:
- `cargo test --workspace`
- `bash scripts/check_wiring.sh`
- `cargo build -p rbmp-server --bins`
- `.venv/bin/python -m pytest tests/protocol/ -v --json-report --json-report-file=runtime/test_results/layer2.json`
- `.venv/bin/python -m pytest tests/ml/ tests/api/ -v --ignore=tests/scenarios --json-report --json-report-file=runtime/test_results/layer3.json`
- `.venv/bin/python -m pytest tests/api/ -v --json-report --json-report-file=runtime/test_results/layer3_api.json`
- `cd ui && npm ci`
- `cd ui && npx playwright install chromium`
- `RUSTYBMP_TEST_MODE=1 ./target/debug/rustybmp runtime/api-test.toml`
- `curl -sS -H 'Content-Type: application/json' -d '{"fixture":"standard","truncate":true}' http://127.0.0.1:7878/api/_test/seed`
- `cd ui && BASE_URL=http://127.0.0.1:5173 npx playwright test`

Key findings:
- `cargo test --workspace -- --test-output immediate` is not a valid command for the current test binaries here; `cargo test --workspace` worked.
- Ubuntu needed extra native deps before the Rust workspace would build cleanly, including `libcurl4-openssl-dev`.
- System Python was PEP 668 managed, so repo-local testing required a virtualenv instead of `pip3 install` into the system interpreter.
- Layer 2 should run against `rustybmp` with a small test config, not `rbmp-collector`.
- Layer 7 should also run against `rustybmp`, not a nonexistent `rbmp-server` binary.
- The existing Playwright suite needed a first-run Chromium download on Ubuntu before browser tests could execute.

Repo fixes made during this session:
- Updated test seed fixtures to match the current DuckDB schema and use explicit column lists.
- Fixed layered fixture loading for `\i ...` includes in the API seed endpoint.
- Made seed fixture timestamps DuckDB-compatible for this environment.
- Made `/api/routes/changes` tolerate missing `since`.
- Restored analytics response keys expected by the tests.
- Added schema-tolerant fallbacks for RPKI analysis/coverage when `route_events.rpki_validity` is absent.
- Replaced some DuckDB query expressions that failed with `NOW() - INTERVAL ...` in this environment.
- Made the peers page render its table shell during loading so Playwright is less timing-sensitive under parallel load.
- Fixed the NL Query example-chip interaction by binding examples through native radio selection to the textarea store.
- Tightened the VRF Playwright mock route so the list endpoint is not swallowed by the routes wildcard.

Files changed in this testing pass:
- `crates/rbmp-server/src/api/routes.rs`
- `crates/rbmp-server/src/api/seed.rs`
- `crates/rbmp-server/src/api/stats.rs`
- `crates/rbmp-store/src/query.rs`
- `tests/seed.sql`
- `tests/seed_anomaly.sql`
- `tests/seed_convergence.sql`
- `tests/seed_maxprefix.sql`
- `runtime/protocol-test.toml`
- `runtime/api-test.toml`
- `ui/src/routes/peers/+page.svelte`
- `ui/src/routes/query/+page.svelte`
- `ui/tests/rustybmp.spec.ts`

Next recommended steps:
1. Start a fresh Codex session before more ContainerLab-backed scenario work, so lab logs do not bloat context.
2. Refactor `tests/scenarios/01_frr_minimal/` so FRR can target a host-run `rustybmp` process instead of requiring a `rustybmp:latest` collector node.
3. Apply the same host-process-first approach to `tests/scenarios/02_xrd_rfc9972/`.
4. After Layer 4/5 scenario behavior is stable, return to the Docker image path as a final packaging-validation pass.
5. Separate audit of store writer vs schema drift, since `writer.rs` still appears to lag the current `route_events` schema.
