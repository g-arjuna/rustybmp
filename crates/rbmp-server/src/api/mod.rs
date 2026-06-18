pub mod routes;
pub mod peers;
pub mod events;
pub mod stats;
pub mod health;

use std::sync::Arc;
use axum::{Router, routing::get};
use tower_http::cors::CorsLayer;
use rbmp_rib::RibManager;
use rbmp_store::RouteStore;
use rbmp_store::query::QueryEngine;
use tokio::sync::broadcast;
use rbmp_rib::event::RibEvent;

/// Shared application state for all HTTP handlers
#[derive(Clone)]
pub struct AppState {
    pub rib:     Arc<tokio::sync::RwLock<RibManager>>,
    pub store:   Arc<std::sync::Mutex<RouteStore>>,
    pub queries: Arc<QueryEngine>,
    pub events:  broadcast::Sender<RibEvent>,
}

/// Build the Axum router with all API routes
pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health",              get(health::health))
        .route("/metrics",             get(health::metrics))
        // Speakers
        .route("/api/speakers",        get(peers::list_speakers))
        .route("/api/speakers/:addr",  get(peers::get_speaker))
        // Peers
        .route("/api/peers",           get(peers::list_peers))
        .route("/api/peers/:addr",     get(peers::get_peer))
        .route("/api/peers/:addr/rib", get(routes::get_peer_rib))
        // Routes
        .route("/api/routes",          get(routes::list_routes))
        .route("/api/routes/prefix",   get(routes::prefix_history))
        .route("/api/routes/changes",  get(routes::route_changes))
        // Analytics
        .route("/api/analytics/churn", get(stats::top_churn))
        .route("/api/analytics/origins", get(stats::as_origins))
        // Real-time event stream (SSE)
        .route("/api/events",          get(events::sse_handler))
        // State
        .with_state(state)
        .layer(CorsLayer::permissive())
}
