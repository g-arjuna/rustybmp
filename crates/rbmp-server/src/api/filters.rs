/// RV6-1: Filter management API.
///
/// POST /api/filters/test    — evaluate the active filter against a synthetic route
/// POST /api/filters/reload  — reload filter file from disk
/// GET  /api/filters/stats   — return filter verdict counters
use std::collections::HashSet;
use std::time::Instant;
use axum::{extract::State, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::state::AppState;

const DEFAULT_FILTER_PATH: &str = "config/filters.yaml";

// ─── Filter test endpoint ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FilterTestBody {
    pub prefix:      String,
    pub peer_as:     u32,
    pub as_path:     Option<String>,
    pub rpki:        Option<String>,
    pub action:      Option<String>,
    pub communities: Option<Vec<String>>,
}

/// POST /api/filters/test
/// Evaluate the currently active filter engine against a synthetic route.
/// Returns verdict (accept/deny/default) and evaluation time in nanoseconds.
pub async fn filter_test(
    State(_state): State<AppState>,
    Json(body):   Json<FilterTestBody>,
) -> Result<Json<Value>, StatusCode> {
    use rbmp_rib::filter_expr::RouteCtx;
    use rbmp_rib::filter::FilterVerdict;
    use rbmp_core::bgp::types::{PathAttributes, Prefix};
    use ipnet::IpNet;

    let prefix_str  = body.prefix.clone();
    let peer_as     = body.peer_as;
    let rpki        = body.rpki.clone().unwrap_or_else(|| "unknown".to_string());
    let action      = body.action.clone().unwrap_or_else(|| "announce".to_string());
    let as_path_str = body.as_path.clone().unwrap_or_default();
    let asns: Vec<u32> = as_path_str.split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    let origin_asn  = asns.last().copied().unwrap_or(0);
    let has_prepend = asns.windows(2).any(|w| w[0] == w[1]);
    let community_set: HashSet<String> = body.communities
        .unwrap_or_default()
        .into_iter()
        .collect();

    let ctx = RouteCtx {
        prefix_len:    prefix_str.split('/').nth(1)
            .and_then(|s| s.parse().ok()).unwrap_or(0),
        as_path_len:   asns.len(),
        origin_asn,
        has_prepend,
        rpki:          rpki.clone(),
        action:        action.clone(),
        peer_as,
        local_pref:    None,
        med:           None,
        community_set,
    };

    let ip_net: IpNet = prefix_str.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let peer_addr: std::net::IpAddr = "0.0.0.0".parse().unwrap();
    let prefix = match ip_net {
        IpNet::V4(n) => Prefix::V4(n),
        IpNet::V6(n) => Prefix::V6(n),
    };
    let attrs = PathAttributes::default();

    let t0 = Instant::now();

    let filter_path = DEFAULT_FILTER_PATH;
    let engine_result = rbmp_rib::FilterEngine::load_file(filter_path);
    let elapsed_ns = t0.elapsed().as_nanos() as u64;

    match engine_result {
        Ok(engine) => {
            let t1 = Instant::now();
            let (verdict, filter_name) = engine.apply_with_ctx(
                &prefix, peer_as, peer_addr, &attrs, Some(&ctx),
            );
            let eval_ns = elapsed_ns + t1.elapsed().as_nanos() as u64;
            let verdict_str = match verdict {
                FilterVerdict::Accept  => "accept",
                FilterVerdict::Deny    => "deny",
                FilterVerdict::Default => "default-accept",
            };
            Ok(Json(json!({
                "verdict":        verdict_str,
                "filter_matched": filter_name,
                "evaluation_ns":  eval_ns,
                "prefix":         prefix_str,
                "peer_as":        peer_as,
                "rpki":           rpki,
                "action":         action,
            })))
        }
        Err(_) => Ok(Json(json!({
            "verdict":        "default-accept",
            "filter_matched": null,
            "evaluation_ns":  elapsed_ns,
            "note":           "No filter file — all routes accepted",
        }))),
    }
}

// ─── Filter reload endpoint ───────────────────────────────────────────────────

/// POST /api/filters/reload
/// Reload the YAML filter from disk and apply to the live RIB manager.
pub async fn filter_reload(
    State(state): State<AppState>,
) -> Result<Json<Value>, axum::http::StatusCode> {
    // Load filter on blocking thread pool (file I/O).
    // Convert the error to String inside the closure so the result is Send.
    let load_result: Result<rbmp_rib::FilterEngine, String> =
        tokio::task::spawn_blocking(|| {
            rbmp_rib::FilterEngine::load_file(DEFAULT_FILTER_PATH)
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));

    match load_result {
        Ok(engine) => {
            let n = engine.len();
            // Acquire write guard, apply, then explicitly drop before returning.
            let mut rib = state.rib.write().await;
            rib.set_filter(engine);
            drop(rib);
            Ok(Json(json!({
                "status":  "reloaded",
                "filters": n,
                "path":    DEFAULT_FILTER_PATH,
            })))
        }
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error":  e,
            "path":   DEFAULT_FILTER_PATH,
        }))),
    }
}

// ─── Filter stats endpoint ────────────────────────────────────────────────────

/// GET /api/filters/stats
/// Report current filter configuration and point to Prometheus counters.
pub async fn filter_stats(
    State(_state): State<AppState>,
) -> Result<Json<Value>, axum::http::StatusCode> {
    let filter_count = rbmp_rib::FilterEngine::load_file(DEFAULT_FILTER_PATH)
        .map(|e| e.len())
        .unwrap_or(0);

    Ok(Json(json!({
        "filter_file":  DEFAULT_FILTER_PATH,
        "filter_count": filter_count,
        "prometheus_metrics_path": "/metrics",
        "counters": {
            "bgp_routes_filtered_total":  "routes rejected by filter",
            "bgp_routes_announced_total": "routes accepted and stored",
            "bgp_routes_withdrawn_total": "withdrawal events processed",
        },
    })))
}
