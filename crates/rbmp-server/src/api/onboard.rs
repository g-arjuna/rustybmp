/// Speaker onboarding API (RV5-7).
///
/// Provides a guided 4-step onboarding flow:
///   Step 1 — Validate: verify BMP connectivity to the proposed speaker IP.
///   Step 2 — Register: save metadata (hostname, vendor, site) to the speaker registry.
///   Step 3 — Filter:  optionally upload or update the YAML filter for this speaker.
///   Step 4 — Confirm: return current speaker status and peer count.
use axum::{extract::{Path, State}, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::IpAddr;
use crate::state::AppState;
use crate::config::SpeakerEntry;

// ─── Step 1 — Validate ────────────────────────────────────────────────────────

/// GET /api/onboard/{addr}/validate
/// Checks whether the speaker is already connected to the BMP receiver.
pub async fn validate_speaker(
    Path(addr):   Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let ip: IpAddr = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let rib = state.rib.read().await;
    match rib.speaker(ip) {
        Some(s) => Ok(Json(json!({
            "step":        1,
            "status":      "connected",
            "addr":        addr,
            "sys_name":    s.sys_name,
            "sys_descr":   s.sys_descr,
            "peers_up":    s.up_peer_count(),
            "peer_count":  s.peer_count(),
            "connected_at": s.connected_at.to_rfc3339(),
            "message":     "Speaker is already connected via BMP. Proceed to step 2."
        }))),
        None => Ok(Json(json!({
            "step":    1,
            "status":  "not_connected",
            "addr":    addr,
            "message": "Speaker not yet seen. Configure BMP on the router to point to this collector."
        }))),
    }
}

// ─── Step 2 — Register ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterBody {
    pub hostname: Option<String>,
    pub vendor:   Option<String>,
    pub site:     Option<String>,
    pub notes:    Option<String>,
}

/// POST /api/onboard/{addr}/register
/// Upsert speaker metadata into the in-memory registry.
pub async fn register_speaker(
    Path(addr):   Path<String>,
    State(state): State<AppState>,
    Json(body):   Json<RegisterBody>,
) -> Result<Json<Value>, StatusCode> {
    let _ip: IpAddr = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    let entry = SpeakerEntry {
        addr:     addr.clone(),
        hostname: body.hostname.unwrap_or_default(),
        vendor:   body.vendor.unwrap_or_default(),
        site:     body.site.unwrap_or_default(),
    };

    state.registry.upsert(entry.clone());

    Ok(Json(json!({
        "step":     2,
        "status":   "registered",
        "addr":     addr,
        "meta":     { "hostname": entry.hostname, "vendor": entry.vendor, "site": entry.site },
        "message":  "Speaker metadata saved. Proceed to step 3 to apply filters."
    })))
}

// ─── Step 3 — Filter ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FilterBody {
    /// YAML filter content (same format as the filter_file).
    pub filter_yaml: String,
}

/// POST /api/onboard/{addr}/filter
/// Parse and apply a YAML filter string to the global filter engine.
pub async fn apply_filter(
    Path(addr):   Path<String>,
    State(state): State<AppState>,
    Json(body):   Json<FilterBody>,
) -> Result<Json<Value>, StatusCode> {
    let _ip: IpAddr = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    use rbmp_rib::FilterEngine;
    let engine = FilterEngine::load_yaml(&body.filter_yaml)
        .map_err(|e| {
            let msg = e.to_string();
            tracing::warn!(err = %msg, "onboard filter parse failed");
            StatusCode::UNPROCESSABLE_ENTITY
        })?;

    let n = engine.len();
    state.rib.write().await.set_filter(engine);

    Ok(Json(json!({
        "step":    3,
        "status":  "filter_applied",
        "addr":    addr,
        "filters": n,
        "message": format!("{n} filter rule(s) loaded. Proceed to step 4 to confirm.")
    })))
}

// ─── Step 4 — Confirm ─────────────────────────────────────────────────────────

/// GET /api/onboard/{addr}/confirm
/// Returns full speaker status — final step of onboarding.
pub async fn confirm_speaker(
    Path(addr):   Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let ip: IpAddr = addr.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let rib  = state.rib.read().await;
    let meta = state.registry.lookup(&addr);

    match rib.speaker(ip) {
        Some(s) => {
            let peers: Vec<Value> = s.peers.values().map(|p| json!({
                "addr":         p.peer_address.to_string(),
                "asn":          p.peer_as,
                "state":        format!("{:?}", p.state),
                "uptime_secs":  p.uptime_secs(),
                "flaps":        p.flap_count,
            })).collect();

            Ok(Json(json!({
                "step":       4,
                "status":     "onboarded",
                "addr":       addr,
                "hostname":   meta.as_ref().map(|m| m.hostname.as_str()).unwrap_or(""),
                "vendor":     meta.as_ref().map(|m| m.vendor.as_str()).unwrap_or(""),
                "site":       meta.as_ref().map(|m| m.site.as_str()).unwrap_or(""),
                "peers_up":   s.up_peer_count(),
                "peers":      peers,
                "total_routes": rib.rib_for_peer(ip)
                    .map(|r| r.all_prefixes().len())
                    .unwrap_or(0),
                "message":    "Speaker successfully onboarded."
            })))
        }
        None => Ok(Json(json!({
            "step":    4,
            "status":  "not_connected",
            "addr":    addr,
            "message": "Speaker is not yet connected. Verify BMP configuration on the router."
        }))),
    }
}
