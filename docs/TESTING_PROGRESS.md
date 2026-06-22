# RustyBMP Testing Progress

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
1. Start a fresh Codex session before ContainerLab-backed scenario work, so UI/debug logs do not bloat context.
2. Layer 4 FRR smoke lab on this Ubuntu host. `docker` and `containerlab` are available here.
3. Layer 5 XRd RFC 9972 lab if images/licenses are available.
4. Separate audit of store writer vs schema drift, since `writer.rs` still appears to lag the current `route_events` schema.
