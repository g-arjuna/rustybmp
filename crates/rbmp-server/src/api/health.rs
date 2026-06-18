use axum::{extract::State, Json};
use serde_json::{json, Value};
use super::AppState;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let rib = state.rib.read().await;
    Json(json!({
        "status": "ok",
        "speakers": rib.speakers().len(),
        "peers_up": rib.total_peers_up(),
        "total_routes": rib.total_routes(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn metrics() -> &'static str {
    // TODO: integrate with metrics-exporter-prometheus
    "# rustybmp metrics placeholder\n"
}
