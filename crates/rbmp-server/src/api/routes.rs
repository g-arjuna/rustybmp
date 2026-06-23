use axum::{extract::{Path, Query, State}, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RibQuery {
    pub rib_type: Option<String>,
    pub prefix: Option<String>,
    pub action: Option<String>,
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
    pub since: Option<String>,
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
    // Live in-memory RIB summary across all peers.
    // `action=announce` maps to the current RIB snapshot; other actions currently return empty.
    if matches!(q.action.as_deref(), Some(action) if action != "announce") {
        return Json(json!({ "routes": Vec::<Value>::new(), "count": 0 }));
    }

    let rib = state.rib.read().await;
    let mut all_routes = Vec::new();
    for speaker in rib.speakers() {
        for (peer_addr, peer) in &speaker.peers {
            if let Some(rib_table) = rib.rib_for_peer(*peer_addr) {
                for entry in rib_table.all_prefixes() {
                    let prefix = entry.prefix.to_string();
                    if let Some(filter_prefix) = q.prefix.as_deref() {
                        if prefix != filter_prefix {
                            continue;
                        }
                    }
                    all_routes.push(json!({
                        "speaker":   speaker.speaker_addr.to_string(),
                        "peer":      peer_addr.to_string(),
                        "peer_as":   peer.peer_as,
                        "prefix":    prefix,
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
    let since = q.since.unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
    let changes = state.queries
        .route_changes(&since, q.until.as_deref(), q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "changes": changes, "count": changes.len() })))
}

// ─── Prefix Explorer endpoints (RV5-2) ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}

#[derive(Debug, Deserialize)]
pub struct ConvergenceQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_days() -> u32 { 7 }

/// GET /api/routes/prefix/{prefix}/timeline?days=7
pub async fn prefix_timeline(
    Path(prefix):   Path<String>,
    Query(q):       Query<TimelineQuery>,
    State(state):   State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let prefix_decoded = urlencoding_decode(&prefix);
    let timeline = state.queries
        .prefix_timeline(&prefix_decoded, q.days)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "prefix": prefix_decoded, "days": q.days, "timeline": timeline })))
}

/// GET /api/routes/prefix/{prefix}/peers
pub async fn prefix_peers(
    Path(prefix): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let prefix_decoded = urlencoding_decode(&prefix);
    let peers = state.queries
        .prefix_peers(&prefix_decoded)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "prefix": prefix_decoded, "peers": peers, "count": peers.len() })))
}

/// GET /api/routes/prefix/{prefix}/convergence?limit=50
pub async fn prefix_convergence(
    Path(prefix): Path<String>,
    Query(q):     Query<ConvergenceQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let prefix_decoded = urlencoding_decode(&prefix);
    let events = state.queries
        .prefix_convergence(&prefix_decoded, q.limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "prefix": prefix_decoded, "events": events })))
}

/// GET /api/rpki/analysis
pub async fn rpki_analysis(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let analysis = state.queries
        .rpki_analysis()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(analysis))
}

/// GET /api/policy?peer={addr}
pub async fn policy_delta(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state):  State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let peer_addr = params.get("peer").map(|s| s.as_str()).unwrap_or("");
    let delta = state.queries
        .policy_delta(peer_addr)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(delta))
}

/// GET /api/rpki/coverage
/// Returns RPKI coverage statistics: % of prefixes covered by ROAs, breakdown by origin AS.
pub async fn rpki_coverage(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let coverage = state.queries
        .rpki_coverage()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(coverage))
}

/// Decode percent-encoded slashes in prefix path segments (e.g. "192.0.2.0%2F24" → "192.0.2.0/24")
fn urlencoding_decode(s: &str) -> String {
    s.replace("%2F", "/").replace("%2f", "/")
}
