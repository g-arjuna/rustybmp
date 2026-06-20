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

/// GET /api/ml/model/status
/// Reports the readiness of each ML model (checks for artefact files on disk).
pub async fn model_status(
    State(_state): State<AppState>,
) -> Json<Value> {
    let models = [
        ("route_anomaly",  "bmppy/ml/models/route_anomaly.joblib"),
        ("bgp_stgnn",      "bmppy/ml/models/bgp_stgnn.pt"),
    ];
    let statuses: Vec<Value> = models.iter().map(|(name, path)| {
        let ready = std::path::Path::new(path).exists();
        json!({
            "model":   name,
            "path":    path,
            "ready":   ready,
            "status":  if ready { "ready" } else { "not_trained" },
        })
    }).collect();

    Json(json!({
        "models": statuses,
        "train_commands": {
            "route_anomaly": "cd bmppy && python -m ml.train_route_anomaly",
            "bgp_stgnn":     "cd bmppy && python -m ml.train_bgp_stgnn",
        }
    }))
}
