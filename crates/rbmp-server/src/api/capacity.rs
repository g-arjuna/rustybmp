/// RV7-B4: Max-prefix capacity and policy config API endpoints.
///
/// GET  /api/capacity/max-prefix               — fuel-gauge data for all peers
/// POST /api/capacity/max-prefix               — upsert a peer's configured limit
/// GET  /api/policy/configs                    — list all fetched policy configs
/// GET  /api/policy/configs/{peer}             — configs for a specific peer
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use axum::extract::Query as AxumQuery;
use crate::state::AppState;

// ─── Max-prefix capacity ─────────────────────────────────────────────────────

/// GET /api/capacity/max-prefix
pub async fn max_prefix_capacity(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let rows = state.queries.max_prefix_capacity().map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    ))?;
    Ok(Json(json!({ "count": rows.len(), "rows": rows })))
}

#[derive(Deserialize)]
pub struct UpsertMaxPrefixRequest {
    pub speaker_addr: String,
    pub peer_addr:    String,
    pub peer_as:      u32,
    pub afi_safi:     String,
    pub max_prefix:   u32,
    #[serde(default = "default_warning_pct")]
    pub warning_pct:  u16,
}
fn default_warning_pct() -> u16 { 75 }

/// POST /api/capacity/max-prefix
pub async fn upsert_max_prefix(
    State(state): State<AppState>,
    Json(body):   Json<UpsertMaxPrefixRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if body.max_prefix == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "max_prefix must be > 0" })),
        ));
    }
    state.queries.upsert_max_prefix(
        &body.speaker_addr,
        &body.peer_addr,
        body.peer_as,
        &body.afi_safi,
        body.max_prefix,
        body.warning_pct,
    ).map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    ))?;
    Ok(Json(json!({ "status": "ok" })))
}

// ─── Policy configs ───────────────────────────────────────────────────────────

/// GET /api/policy/configs
pub async fn list_policy_configs(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let rows = state.queries.policy_configs(None).map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    ))?;
    Ok(Json(json!({ "count": rows.len(), "rows": rows })))
}

// ─── Convergence events ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ConvergenceQuery {
    pub peer:  Option<String>,
    pub hours: Option<u32>,
    pub limit: Option<u32>,
}

/// GET /api/convergence
pub async fn convergence_events(
    State(state): State<AppState>,
    AxumQuery(q):  AxumQuery<ConvergenceQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let hours = q.hours.unwrap_or(24);
    let limit = q.limit.unwrap_or(200);
    let rows  = state.queries
        .convergence_events(q.peer.as_deref(), hours, limit)
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ))?;
    Ok(Json(json!({ "count": rows.len(), "rows": rows })))
}

/// GET /api/policy/configs/{peer}
pub async fn peer_policy_configs(
    State(state): State<AppState>,
    Path(peer):   Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let rows = state.queries.policy_configs(Some(&peer)).map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    ))?;
    Ok(Json(json!({ "peer": peer, "count": rows.len(), "rows": rows })))
}
