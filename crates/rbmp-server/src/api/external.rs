/// External API integration endpoints (RV8-EXT5).
///
/// GET /api/external/prefix-visibility?prefix=<CIDR>
///   Compares the internal RIB view of a prefix against RIPE STAT / BGP.Tools
///   external visibility data. Returns discrepancies (e.g. prefix visible
///   internally but not externally, or origin ASN mismatch).
use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use tracing::warn;
use rbmp_core::bmp::types::RibType;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PrefixVisibilityParams {
    pub prefix: String,
}

/// GET /api/external/prefix-visibility?prefix=<CIDR>
pub async fn prefix_visibility(
    State(state): State<AppState>,
    Query(params): Query<PrefixVisibilityParams>,
) -> Json<Value> {
    let prefix = params.prefix.trim().to_string();
    if prefix.is_empty() {
        return Json(json!({ "error": "prefix parameter required" }));
    }

    // ── Internal view (from the live RIB) ────────────────────────────────────
    let rib = state.rib.read().await;
    let mut internal_origin_asns: HashSet<u32> = HashSet::new();
    let mut internal_peer_count = 0usize;
    let mut internal_next_hops: Vec<String> = Vec::new();

    for session in rib.speakers() {
        for (peer_addr, _peer) in &session.peers {
            for rib_type in [RibType::AdjRibInPrePolicy, RibType::AdjRibInPostPolicy, RibType::LocRib] {
                if let Some(peer_rib) = rib.rib_for_peer(*peer_addr) {
                    for entry in peer_rib.iter_rib(rib_type) {
                        if entry.prefix.to_string() == prefix {
                            internal_peer_count += 1;
                            if let Some(aspath) = &entry.attributes.as_path {
                                if let Some(origin_asn) = aspath.origin_asn() {
                                    internal_origin_asns.insert(origin_asn);
                                }
                            }
                            if let Some(nh) = entry.attributes.next_hop {
                                internal_next_hops.push(nh.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    drop(rib);

    // ── External view (RIPE STAT via HTTP) ───────────────────────────────────
    let external = fetch_ripe_stat_overview(&prefix).await;

    // ── Discrepancy analysis ──────────────────────────────────────────────────
    let mut discrepancies: Vec<String> = Vec::new();

    let ext_announced = external.get("announced").and_then(|v| v.as_bool()).unwrap_or(false);
    let ext_origin_asns: HashSet<u32> = external
        .get("origin_asns")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u32)).collect())
        .unwrap_or_default();

    if internal_peer_count > 0 && !ext_announced {
        discrepancies.push("Prefix visible internally but NOT announced globally (possible leak)".into());
    }
    if internal_peer_count == 0 && ext_announced {
        discrepancies.push("Prefix announced globally but NOT in local RIB (possible policy drop)".into());
    }
    if !internal_origin_asns.is_empty() && !ext_origin_asns.is_empty() {
        let internal_only: Vec<u32> = internal_origin_asns.difference(&ext_origin_asns).cloned().collect();
        let external_only: Vec<u32> = ext_origin_asns.difference(&internal_origin_asns).cloned().collect();
        if !internal_only.is_empty() {
            discrepancies.push(format!(
                "Origin ASNs seen internally but not externally: {:?} — possible route leak or test prefix",
                internal_only
            ));
        }
        if !external_only.is_empty() {
            discrepancies.push(format!(
                "Origin ASNs seen externally but not internally: {:?} — possible hijack or policy filter",
                external_only
            ));
        }
    }

    Json(json!({
        "prefix": prefix,
        "internal": {
            "peer_count":   internal_peer_count,
            "origin_asns":  internal_origin_asns.iter().cloned().collect::<Vec<_>>(),
            "next_hops":    internal_next_hops,
        },
        "external": external,
        "discrepancies": discrepancies,
        "has_discrepancies": !discrepancies.is_empty(),
    }))
}

/// Fetch prefix overview from RIPE STAT (non-blocking HTTP, fire-and-forget on error).
async fn fetch_ripe_stat_overview(prefix: &str) -> Value {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("rustybmp/0.8.0")
        .build()
        .unwrap_or_default();

    match client
        .get("https://stat.ripe.net/data/prefix-overview/data.json")
        .query(&[("resource", prefix), ("sourceapp", "rustybmp")])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<Value>().await {
                Ok(body) => {
                    let data = body.get("data").cloned().unwrap_or_default();
                    let announced   = data.get("announced").cloned().unwrap_or(json!(false));
                    let origin_asns: Vec<u32> = data
                        .get("asns").and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|a| {
                            a.get("asn").and_then(|v| v.as_u64()).map(|n| n as u32)
                        }).collect())
                        .unwrap_or_default();
                    let country  = data.get("block").and_then(|b| b.get("country")).cloned().unwrap_or(json!(""));
                    let rir      = data.get("block").and_then(|b| b.get("registry")).cloned().unwrap_or(json!(""));
                    json!({
                        "announced":    announced,
                        "origin_asns":  origin_asns,
                        "country":      country,
                        "rir":          rir,
                        "source":       "ripe_stat",
                    })
                }
                Err(e) => {
                    warn!(prefix, error = %e, "RIPE STAT JSON parse error");
                    json!({ "error": "parse_error", "source": "ripe_stat" })
                }
            }
        }
        Ok(resp) => {
            warn!(prefix, status = %resp.status(), "RIPE STAT non-2xx");
            json!({ "error": format!("http_{}", resp.status().as_u16()), "source": "ripe_stat" })
        }
        Err(e) => {
            warn!(prefix, error = %e, "RIPE STAT request failed");
            json!({ "error": "unreachable", "source": "ripe_stat" })
        }
    }
}
