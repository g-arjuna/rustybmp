use axum::{extract::{Query, State}, Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AnomaliesQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub kind:  Option<String>,
}

fn default_limit() -> usize { 100 }

/// GET /api/ml/anomalies?limit=100&kind=churn_zscore
pub async fn list_anomalies(
    Query(q):     Query<AnomaliesQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let anomalies = state.queries
        .ml_anomalies(q.limit, q.kind.as_deref())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "anomalies": anomalies, "count": anomalies.len() })))
}
