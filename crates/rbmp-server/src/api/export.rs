/// Parquet export API (RV4-2 T2).
///
/// GET /api/export/parquet?table=route_events&since=<ISO>&until=<ISO>
///
/// DuckDB writes a zstd-compressed Parquet file to a temp path,
/// then we stream it back as application/octet-stream.
/// If since/until are omitted, defaults to the past 7 days.
use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, StatusCode},
    response::Response,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::io::Read;
use tracing::info;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct ExportParams {
    /// One of: route_events | peer_events | speaker_events | stats_events | evpn_events
    #[serde(default = "default_table")]
    pub table: String,
    /// ISO 8601 UTC start (inclusive)
    pub since: Option<DateTime<Utc>>,
    /// ISO 8601 UTC end (inclusive)
    pub until: Option<DateTime<Utc>>,
}

fn default_table() -> String { "route_events".into() }

const ALLOWED_TABLES: &[&str] = &[
    "route_events", "peer_events", "speaker_events", "stats_events", "evpn_events",
];

pub async fn export_parquet(
    State(state): State<AppState>,
    Query(params): Query<ExportParams>,
) -> Result<Response<Body>, (StatusCode, String)> {
    // Validate table name (prevent SQL injection)
    if !ALLOWED_TABLES.contains(&params.table.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("table must be one of: {}", ALLOWED_TABLES.join(", ")),
        ));
    }

    let until = params.until.unwrap_or_else(Utc::now);
    let since = params.since.unwrap_or_else(|| until - Duration::days(7));

    // Write to a temp file — DuckDB COPY requires a file path
    let tmp_path = std::env::temp_dir().join(format!(
        "rustybmp_{}_{}_{}.parquet",
        params.table,
        since.format("%Y%m%dT%H%M%S"),
        until.format("%Y%m%dT%H%M%S"),
    ));

    let sql = format!(
        "COPY (SELECT * FROM {table} WHERE occurred_at BETWEEN TIMESTAMPTZ '{since}' AND TIMESTAMPTZ '{until}') \
         TO '{path}' (FORMAT PARQUET, COMPRESSION 'zstd')",
        table = params.table,
        since = since.to_rfc3339(),
        until = until.to_rfc3339(),
        path  = tmp_path.display(),
    );

    info!(
        table = %params.table,
        since = %since.to_rfc3339(),
        until = %until.to_rfc3339(),
        path  = %tmp_path.display(),
        "Parquet export requested"
    );

    // Execute export inside blocking thread (DuckDB is sync)
    let tmp_path2 = tmp_path.clone();
    let result = {
        let store = state.store.lock().map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        store.conn().execute_batch(&sql).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        // Read file bytes
        let mut f = std::fs::File::open(&tmp_path2)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        buf
    };

    // Clean up temp file
    let _ = std::fs::remove_file(&tmp_path);

    let filename = format!(
        "{}_{}.parquet",
        params.table,
        since.format("%Y%m%d"),
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(result))
        .unwrap())
}
