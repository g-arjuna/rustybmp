/// POST /api/_test/seed — deterministic DuckDB seed endpoint (Bundle A4).
///
/// Loads a named SQL fixture into the in-process DuckDB so that Playwright
/// and API contract tests always start from a known state.
///
/// Fixture names:
///   "standard"    — 2 speakers, 4 peers, ~30 route events (tests/seed.sql)
///   "anomaly"     — above + 3 ML anomaly rows (tests/seed_anomaly.sql)
///   "maxprefix"   — above + max-prefix capacity rows (tests/seed_maxprefix.sql)
///   "convergence" — above + convergence events (tests/seed_convergence.sql)
///
/// Only available when the server is compiled with the `test-seed` feature
/// or when the `RUSTYBMP_TEST_MODE` environment variable is set to `1`.
///
/// Security: this endpoint MUST NOT be reachable in production.
///   The route is only mounted when `cfg!(feature = "test-seed")` or
///   `RUSTYBMP_TEST_MODE=1`, and it is always behind the JWT middleware.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use tracing::info;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SeedRequest {
    /// Which fixture to load.  Defaults to "standard".
    #[serde(default = "default_fixture")]
    pub fixture: String,
    /// If true, truncate all event tables before seeding (default: true).
    #[serde(default = "default_truncate")]
    pub truncate: bool,
}

fn default_fixture()  -> String { "standard".into() }
fn default_truncate() -> bool   { true }

#[derive(Debug, Serialize)]
pub struct SeedResponse {
    pub ok:      bool,
    pub fixture: String,
    pub rows_affected: Option<u64>,
    pub error:   Option<String>,
}

/// Resolve a fixture name to the SQL file path (relative to repo root).
fn fixture_path(name: &str) -> Option<&'static str> {
    match name {
        "standard"    => Some("tests/seed.sql"),
        "anomaly"     => Some("tests/seed_anomaly.sql"),
        "maxprefix"   => Some("tests/seed_maxprefix.sql"),
        "convergence" => Some("tests/seed_convergence.sql"),
        _             => None,
    }
}

const TRUNCATE_SQL: &str = "
DELETE FROM route_events;
DELETE FROM peer_events;
DELETE FROM speaker_events;
DELETE FROM stats_events;
DELETE FROM ml_anomalies;
DELETE FROM convergence_events;
DELETE FROM peer_max_prefix;
";

pub async fn seed_handler(
    State(state): State<AppState>,
    Json(req): Json<SeedRequest>,
) -> Json<SeedResponse> {
    // Guard: refuse unless test mode is active
    if std::env::var("RUSTYBMP_TEST_MODE").as_deref() != Ok("1") {
        return Json(SeedResponse {
            ok: false,
            fixture: req.fixture,
            rows_affected: None,
            error: Some("RUSTYBMP_TEST_MODE is not set — seed endpoint disabled".into()),
        });
    }

    let Some(path) = fixture_path(&req.fixture) else {
        return Json(SeedResponse {
            ok: false,
            fixture: req.fixture.clone(),
            rows_affected: None,
            error: Some(format!("Unknown fixture '{}'. Valid: standard, anomaly, maxprefix, convergence", req.fixture)),
        });
    };

    let sql = match std::fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => return Json(SeedResponse {
            ok: false,
            fixture: req.fixture,
            rows_affected: None,
            error: Some(format!("Cannot read fixture file '{path}': {e}")),
        }),
    };

    let store = match state.store.lock() {
        Ok(s)  => s,
        Err(e) => return Json(SeedResponse {
            ok: false,
            fixture: req.fixture,
            rows_affected: None,
            error: Some(format!("Store lock poisoned: {e}")),
        }),
    };

    if req.truncate {
        if let Err(e) = store.conn().execute_batch(TRUNCATE_SQL) {
            return Json(SeedResponse {
                ok: false,
                fixture: req.fixture,
                rows_affected: None,
                error: Some(format!("Truncate failed: {e}")),
            });
        }
    }

    match store.conn().execute_batch(&sql) {
        Ok(_) => {
            info!(fixture = %req.fixture, path, "Test seed loaded");
            Json(SeedResponse {
                ok: true,
                fixture: req.fixture,
                rows_affected: None,
                error: None,
            })
        }
        Err(e) => Json(SeedResponse {
            ok: false,
            fixture: req.fixture,
            rows_affected: None,
            error: Some(format!("Seed SQL failed: {e}")),
        }),
    }
}
