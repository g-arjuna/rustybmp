/// RV7 Path Status API — draft-ietf-grow-bmp-path-marking-tlv-05
///
/// Exposes the path_markings DuckDB table via two endpoints:
///   GET /api/path-status/matrix   — redundancy health matrix (prefix × peer)
///   GET /api/path-status/history  — status timeline for one prefix+peer
use axum::{extract::{Query, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct MatrixParams {
    pub afi:              Option<String>,
    pub min_active_paths: Option<u32>,
    #[serde(default = "default_limit")]
    pub limit:            usize,
}
fn default_limit() -> usize { 1000 }

#[derive(Deserialize)]
pub struct HistoryParams {
    pub prefix:    String,
    pub peer_addr: String,
    #[serde(default = "default_hours")]
    pub hours:     u32,
    #[serde(default = "default_limit")]
    pub limit:     usize,
}
fn default_hours() -> u32 { 24 }

#[derive(Serialize)]
pub struct PathStatusResponse {
    pub count: usize,
    pub rows:  Vec<Value>,
}

/// GET /api/path-status/matrix
///
/// Returns the latest path status per (prefix, peer) as a flat row list.
/// UI builds the grid client-side: group by prefix, columns = peer_addrs.
pub async fn path_status_matrix(
    State(state): State<AppState>,
    Query(params): Query<MatrixParams>,
) -> Json<PathStatusResponse> {
    match state.queries.path_status_matrix(
        params.afi.as_deref(),
        params.min_active_paths,
        params.limit,
    ) {
        Ok(rows) => {
            let count = rows.len();
            let json_rows = rows.into_iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect();
            Json(PathStatusResponse { count, rows: json_rows })
        }
        Err(e) => {
            tracing::warn!(%e, "path_status_matrix query failed");
            Json(PathStatusResponse { count: 0, rows: vec![] })
        }
    }
}

/// GET /api/path-status/history?prefix=X&peer_addr=Y&hours=24
pub async fn path_status_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryParams>,
) -> Json<PathStatusResponse> {
    match state.queries.path_status_history(
        &params.prefix,
        &params.peer_addr,
        params.hours,
        params.limit,
    ) {
        Ok(rows) => {
            let count = rows.len();
            let json_rows = rows.into_iter()
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .collect();
            Json(PathStatusResponse { count, rows: json_rows })
        }
        Err(e) => {
            tracing::warn!(%e, "path_status_history query failed");
            Json(PathStatusResponse { count: 0, rows: vec![] })
        }
    }
}
