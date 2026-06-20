pub mod routes;
pub mod peers;
pub mod events;
pub mod stats;
pub mod health;
pub mod export;
pub mod topology;

use std::sync::Arc;
use axum::{Router, routing::{get, post}, middleware};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use crate::auth::{auth_handler, require_auth};
use crate::state::AppState;

/// Build the Axum router with all API routes
pub fn build_router(state: AppState) -> Router {
    let auth_cfg = Arc::clone(&state.auth_cfg);

    // Protected API sub-router (JWT middleware applied when auth.enabled = true)
    let api = Router::new()
        // Speakers
        .route("/speakers",            get(peers::list_speakers))
        .route("/speakers/{addr}",     get(peers::get_speaker))
        // Peers
        .route("/peers",               get(peers::list_peers))
        .route("/peers/{addr}",        get(peers::get_peer))
        .route("/peers/{addr}/rib",    get(routes::get_peer_rib))
        // Routes
        .route("/routes",              get(routes::list_routes))
        .route("/routes/prefix",       get(routes::prefix_history))
        .route("/routes/changes",      get(routes::route_changes))
        // Analytics
        .route("/analytics/churn",     get(stats::top_churn))
        .route("/analytics/origins",   get(stats::as_origins))
        // RPKI
        .route("/rpki/stats",          get(stats::rpki_stats))
        // BGP-LS topology graph (RV4-6)
        .route("/bgpls/graph",         get(topology::bgpls_graph))
        // Parquet export (RV4-2)
        .route("/export/parquet",      get(export::export_parquet))
        // Real-time event stream (SSE)
        .route("/events",              get(events::sse_handler))
        // Apply JWT auth middleware
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&auth_cfg),
            require_auth,
        ));

    // Serve compiled Svelte UI from ui/dist if present (RV4-3 T2)
    let ui_dir = std::path::PathBuf::from("ui/dist");
    let serve_ui = ui_dir.exists();

    let mut router = Router::new()
        // Health + metrics — always public
        .route("/health",  get(health::health))
        .route("/metrics", get(health::metrics))
        // Auth token endpoint — public
        .route("/auth", post(auth_handler))
        // Mount protected API at /api
        .nest("/api", api)
        .with_state(state)
        .layer(CorsLayer::permissive());

    if serve_ui {
        router = router.nest_service(
            "/",
            ServeDir::new(&ui_dir)
                .not_found_service(ServeFile::new(ui_dir.join("index.html"))),
        );
    }
    router
}
