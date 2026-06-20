use axum::{extract::State, Json};
use crate::state::AppState;

/// GET /api/governance — Return current resource governor snapshot (RV8-GOV2).
pub async fn get_governance(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snapshot = state.governor.snapshot("default");
    Json(serde_json::to_value(snapshot).unwrap_or_default())
}
