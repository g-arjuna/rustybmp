use axum::{extract::State, Json, http::{header, HeaderValue}};
use axum::response::{IntoResponse, Response};
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

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let body = state.prom.render();
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static(
            "text/plain; version=0.0.4; charset=utf-8"
        ))
        .body(body)
        .unwrap()
}
