/// RV6-5: New analytics API endpoints.
///
/// GET /api/srpolicy           — list active SR Policy NLRIs
/// GET /api/srpolicy/{peer}    — SR Policies from a specific peer
/// GET /api/aspath/graph       — AS-path flow graph (Sankey input data)
use axum::{extract::{Path, Query, State}, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::state::AppState;

// ─── SR Policy ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SrPolicyQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 200 }

/// GET /api/srpolicy
/// List all active SR Policy NLRIs stored in DuckDB.
pub async fn srpolicy_list(
    Query(q):     Query<SrPolicyQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rows = state.queries
        .srpolicy_list(q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "policies": rows, "count": rows.len() })))
}

/// GET /api/srpolicy/{peer}
/// List SR Policies from a specific peer address.
pub async fn srpolicy_by_peer(
    Path(peer):   Path<String>,
    Query(q):     Query<SrPolicyQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rows = state.queries
        .srpolicy_by_peer(&peer, q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "peer": peer, "policies": rows, "count": rows.len() })))
}

// ─── AS Path graph (Sankey) ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AsPathGraphQuery {
    /// Filter to routes that contain this ASN
    pub asn:  Option<u32>,
    /// Filter to routes from this peer
    pub peer: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// GET /api/aspath/graph?asn=3356&limit=200
/// Returns nodes + links for a Sankey / flow diagram of AS paths.
/// Each link is (source_asn, target_asn, flow_count).
pub async fn aspath_graph(
    Query(q):     Query<AsPathGraphQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let graph = state.queries
        .aspath_graph(q.asn, q.peer.as_deref(), q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(graph))
}
