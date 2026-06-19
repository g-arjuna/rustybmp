use axum::{extract::{Query, State}, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use super::AppState;

#[derive(Deserialize)]
pub struct TopN { #[serde(default = "default_n")] pub n: usize }
fn default_n() -> usize { 20 }

pub async fn top_churn(
    Query(q): Query<TopN>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rows = state.queries.top_churning_prefixes(q.n)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "top_churn": rows.into_iter().map(|(p, c)| json!({"prefix": p, "events": c})).collect::<Vec<_>>()
    })))
}

pub async fn as_origins(
    Query(q): Query<TopN>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let rows = state.queries.as_origin_counts(q.n)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "origin_asns": rows.into_iter().map(|(asn, cnt)| json!({"asn": asn, "prefix_count": cnt})).collect::<Vec<_>>()
    })))
}

pub async fn rpki_stats(
    State(state): State<AppState>,
) -> Json<Value> {
    Json(json!({
        "vrp_count":  state.enrichment.vrp_count(),
        "rtr_serial": state.enrichment.rtr_serial(),
    }))
}
