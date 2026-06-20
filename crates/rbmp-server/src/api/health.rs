use axum::{extract::State, Json, http::{header, HeaderValue}};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let rib = state.rib.read().await;
    let db_rows: Vec<Value> = state.store.lock()
        .map(|s| s.table_row_counts()
            .into_iter()
            .map(|(t, n)| json!({"table": t, "rows": n}))
            .collect())
        .unwrap_or_default();
    Json(json!({
        "status": "ok",
        "speakers": rib.speakers().len(),
        "peers_up": rib.total_peers_up(),
        "total_routes": rib.total_routes(),
        "version": env!("CARGO_PKG_VERSION"),
        "db_tables": db_rows,
    }))
}

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    // Collect DuckDB row counts and render as Prometheus gauges
    let mut extra = String::new();
    if let Ok(store) = state.store.lock() {
        for (table, rows) in store.table_row_counts() {
            extra.push_str(&format!(
                "# HELP rustybmp_duckdb_rows Total rows in DuckDB table\n\
                 # TYPE rustybmp_duckdb_rows gauge\n\
                 rustybmp_duckdb_rows{{table=\"{table}\"}} {rows}\n"
            ));
        }
    }
    let mut body = state.prom.render();
    body.push_str(&extra);
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static(
            "text/plain; version=0.0.4; charset=utf-8"
        ))
        .body(body)
        .unwrap()
}
