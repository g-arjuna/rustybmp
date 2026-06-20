use axum::{extract::{Path, Query, State}, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::state::AppState;

pub async fn list_speakers(State(state): State<AppState>) -> Json<Value> {
    let rib = state.rib.read().await;
    let speakers: Vec<Value> = rib.speakers().iter().map(|s| {
        let addr_str = s.speaker_addr.to_string();
        let meta     = state.registry.lookup(&addr_str);
        json!({
            "addr":         addr_str,
            "hostname":     meta.as_ref().map(|m| m.hostname.as_str()).unwrap_or(""),
            "vendor":       meta.as_ref().map(|m| m.vendor.as_str()).unwrap_or(""),
            "site":         meta.as_ref().map(|m| m.site.as_str()).unwrap_or(""),
            "sys_name":     s.sys_name,
            "sys_descr":    s.sys_descr,
            "connected_at": s.connected_at.to_rfc3339(),
            "peer_count":   s.peer_count(),
            "peers_up":     s.up_peer_count(),
            "total_routes": s.total_routes(),
        })
    }).collect();
    Json(json!({ "speakers": speakers, "count": speakers.len() }))
}

pub async fn get_speaker(
    Path(addr): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rib = state.rib.read().await;
    let ip  = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    match rib.speaker(ip) {
        Some(s) => {
            let addr_str = s.speaker_addr.to_string();
            let meta     = state.registry.lookup(&addr_str);
            Ok(Json(json!({
            "addr":         addr_str,
            "hostname":     meta.as_ref().map(|m| m.hostname.as_str()).unwrap_or(""),
            "vendor":       meta.as_ref().map(|m| m.vendor.as_str()).unwrap_or(""),
            "site":         meta.as_ref().map(|m| m.site.as_str()).unwrap_or(""),
            "sys_name":     s.sys_name,
            "sys_descr":    s.sys_descr,
            "connected_at": s.connected_at.to_rfc3339(),
            "peers": s.peers.values().map(|p| json!({
                "addr":    p.peer_address.to_string(),
                "asn":     p.peer_as,
                "state":   format!("{:?}", p.state),
                "up_at":   p.up_at.map(|t| t.to_rfc3339()),
                "uptime_secs": p.uptime_secs(),
                "flaps":   p.flap_count,
            })).collect::<Vec<_>>(),
        })))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn list_peers(State(state): State<AppState>) -> Json<Value> {
    let rib = state.rib.read().await;
    let peers: Vec<Value> = rib.speakers().iter().flat_map(|s| {
        s.peers.values().map(|p| json!({
            "speaker":   s.speaker_addr.to_string(),
            "addr":      p.peer_address.to_string(),
            "asn":       p.peer_as,
            "bgp_id":    p.peer_bgp_id.to_string(),
            "state":     format!("{:?}", p.state),
            "up_at":     p.up_at.map(|t| t.to_rfc3339()),
            "uptime_secs": p.uptime_secs(),
            "hold_time": p.hold_time,
            "flaps":     p.flap_count,
            "route_counts": p.route_counts.iter().map(|(k, v)| (format!("{:?}", k), v)).collect::<std::collections::HashMap<_,_>>(),
        }))
    }).collect();
    Json(json!({ "peers": peers, "count": peers.len() }))
}

pub async fn get_peer(
    Path(addr): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rib = state.rib.read().await;
    let ip  = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    for s in rib.speakers() {
        if let Some(p) = s.peers.get(&ip) {
            return Ok(Json(json!({
                "speaker":   s.speaker_addr.to_string(),
                "addr":      p.peer_address.to_string(),
                "asn":       p.peer_as,
                "bgp_id":    p.peer_bgp_id.to_string(),
                "state":     format!("{:?}", p.state),
                "up_at":     p.up_at.map(|t| t.to_rfc3339()),
                "down_at":   p.down_at.map(|t| t.to_rfc3339()),
                "uptime_secs": p.uptime_secs(),
                "hold_time": p.hold_time,
                "flaps":     p.flap_count,
                "capabilities": p.capabilities.iter().map(|c| format!("{:?}", c)).collect::<Vec<_>>(),
                "route_counts": p.route_counts.iter().map(|(k,v)| (format!("{:?}",k), v)).collect::<std::collections::HashMap<_,_>>(),
            })));
        }
    }
    Err(StatusCode::NOT_FOUND)
}

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}
fn default_days() -> u32 { 7 }

/// GET /api/peers/{addr}/timeline?days=7
pub async fn peer_timeline(
    Path(addr):   Path<String>,
    Query(q):     Query<TimelineQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let timeline = state.queries
        .peer_session_timeline(&addr, q.days)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "peer_addr": addr, "days": q.days, "timeline": timeline })))
}

/// GET /api/peers/{addr}/capabilities
/// Returns the BGP capabilities negotiated with this peer.
pub async fn peer_capabilities(
    Path(addr):   Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rib = state.rib.read().await;
    let ip  = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    for s in rib.speakers() {
        if let Some(p) = s.peers.get(&ip) {
            let caps: Vec<Value> = p.capabilities.iter().map(|c| {
                json!({
                    "code": c.code(),
                    "name": format!("{:?}", c),
                })
            }).collect();
            return Ok(Json(json!({
                "peer_addr":    addr,
                "peer_as":      p.peer_as,
                "capabilities": caps,
                "hold_time":    p.hold_time,
                "add_path":     caps.iter().any(|c| c["name"].as_str()
                    .map(|s| s.starts_with("AddPath")).unwrap_or(false)),
                "four_byte_asn": caps.iter().any(|c| c["name"].as_str()
                    .map(|s| s.starts_with("FourByteAsn")).unwrap_or(false)),
                "llgr":         caps.iter().any(|c| c["name"].as_str()
                    .map(|s| s.starts_with("LongLivedGracefulRestart")).unwrap_or(false)),
            })));
        }
    }
    Err(StatusCode::NOT_FOUND)
}
