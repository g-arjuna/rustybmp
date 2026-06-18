use axum::{extract::{Path, Query, State}, Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use super::AppState;

#[derive(Debug, Deserialize)]
pub struct RibQuery {
    pub rib_type: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct PrefixQuery {
    pub prefix: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct ChangesQuery {
    pub since: String,
    pub until: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 500 }

pub async fn get_peer_rib(
    Path(addr): Path<String>,
    Query(q):   Query<RibQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let routes = state.queries
        .current_rib(&addr, q.rib_type.as_deref(), q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "peer": addr, "routes": routes, "count": routes.len() })))
}

pub async fn list_routes(
    Query(q): Query<RibQuery>,
    State(state): State<AppState>,
) -> Json<Value> {
    // Live in-memory RIB summary across all peers
    let rib = state.rib.read().await;
    let mut all_routes = Vec::new();
    for speaker in rib.speakers() {
        for (peer_addr, peer) in &speaker.peers {
            if let Some(rib_table) = rib.rib_for_peer(*peer_addr) {
                for entry in rib_table.all_prefixes() {
                    all_routes.push(json!({
                        "speaker":   speaker.speaker_addr.to_string(),
                        "peer":      peer_addr.to_string(),
                        "peer_as":   peer.peer_as,
                        "prefix":    entry.prefix.to_string(),
                        "next_hop":  entry.attributes.next_hop.map(|h| h.to_string()),
                        "as_path":   entry.attributes.as_path.as_ref().map(|p| p.to_string()),
                        "local_pref": entry.attributes.local_pref,
                        "med":       entry.attributes.multi_exit_disc,
                        "communities": entry.attributes.communities.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                        "received_at": entry.received_at.to_rfc3339(),
                    }));
                    if all_routes.len() >= q.limit { break; }
                }
            }
        }
    }
    Json(json!({ "routes": all_routes, "count": all_routes.len() }))
}

pub async fn prefix_history(
    Query(q): Query<PrefixQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let history = state.queries
        .prefix_history(&q.prefix, q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "prefix": q.prefix, "history": history, "count": history.len() })))
}

pub async fn route_changes(
    Query(q): Query<ChangesQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let changes = state.queries
        .route_changes(&q.since, q.until.as_deref(), q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "changes": changes, "count": changes.len() })))
}
