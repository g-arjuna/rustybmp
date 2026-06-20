pub mod routes;
pub mod peers;
pub mod events;
pub mod stats;
pub mod health;
pub mod export;
pub mod topology;
pub mod ml;
pub mod onboard;
pub mod filters;
pub mod analytics;
pub mod path_status;
pub mod credentials;
pub mod policy_fetch;
pub mod capacity;
pub mod governance;
pub mod schema;
pub mod external;

use std::sync::Arc;
use axum::{Router, routing::{get, post, any}, middleware};
use tower_http::cors::CorsLayer;
use crate::api::schema::{openapi_spec, swagger_ui};
use crate::mcp_server::mcp_handler;
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
        .route("/speakers/summary",    get(peers::speakers_summary))
        .route("/speakers/{addr}",     get(peers::get_speaker))
        // Peers
        .route("/peers",               get(peers::list_peers))
        .route("/peers/{addr}",        get(peers::get_peer))
        .route("/peers/{addr}/rib",    get(routes::get_peer_rib))
        // Routes
        .route("/routes",                                   get(routes::list_routes))
        .route("/routes/prefix",                            get(routes::prefix_history))
        .route("/routes/changes",                           get(routes::route_changes))
        // Prefix Explorer (RV5-2)
        .route("/routes/prefix/{prefix}/timeline",          get(routes::prefix_timeline))
        .route("/routes/prefix/{prefix}/peers",             get(routes::prefix_peers))
        .route("/routes/prefix/{prefix}/convergence",       get(routes::prefix_convergence))
        // Analytics
        .route("/analytics/churn",     get(stats::top_churn))
        .route("/analytics/origins",   get(stats::as_origins))
        // RPKI (RV5-4)
        .route("/rpki/stats",          get(stats::rpki_stats))
        .route("/rpki/analysis",       get(routes::rpki_analysis))
        // Policy analysis (RV5-5)
        .route("/policy",              get(routes::policy_delta))
        // Peer timeline (RV5-6)
        .route("/peers/{addr}/timeline", get(peers::peer_timeline))
        // ML anomalies (RV5-9)
        .route("/ml/anomalies",           get(ml::list_anomalies))
        // Speaker onboarding (RV5-7)
        .route("/onboard/{addr}/validate", get(onboard::validate_speaker))
        .route("/onboard/{addr}/register", post(onboard::register_speaker))
        .route("/onboard/{addr}/filter",   post(onboard::apply_filter))
        .route("/onboard/{addr}/confirm",  get(onboard::confirm_speaker))
        // BGP-LS topology graph (RV4-6)
        .route("/bgpls/graph",         get(topology::bgpls_graph))
        .route("/bgpls/path",          get(topology::bgpls_path))
        // SR Policy (RV6-5)
        .route("/srpolicy",            get(analytics::srpolicy_list))
        .route("/srpolicy/{peer}",     get(analytics::srpolicy_by_peer))
        // AS Path graph (RV6-5)
        .route("/aspath/graph",        get(analytics::aspath_graph))
        // BMP stats history (RV6-5)
        .route("/bmpstats/history",    get(stats::bmp_stats_history))
        // Peer capabilities (RV6-5)
        .route("/peers/{addr}/capabilities", get(peers::peer_capabilities))
        // RPKI coverage/impact (RV6-5)
        .route("/rpki/coverage",       get(routes::rpki_coverage))
        // Path Status TLV (RV7-P3)
        .route("/path-status/matrix",  get(path_status::path_status_matrix))
        .route("/path-status/history", get(path_status::path_status_history))
        // Credential vault (RV7-V2)
        .route("/credentials",          get(credentials::list_credentials))
        .route("/credentials",          post(credentials::add_credential))
        .route("/credentials/{alias}",  axum::routing::delete(credentials::delete_credential))
        // SSH Policy fetch (RV7-V3)
        .route("/policy/fetch",          post(policy_fetch::trigger_policy_fetch))
        .route("/policy/fetch/{job_id}", get(policy_fetch::get_fetch_job))
        // Policy configs (RV7-B4)
        .route("/policy/configs",        get(capacity::list_policy_configs))
        .route("/policy/configs/{peer}", get(capacity::peer_policy_configs))
        // Max-prefix capacity (RV7-B4)
        .route("/capacity/max-prefix",   get(capacity::max_prefix_capacity))
        .route("/capacity/max-prefix",   post(capacity::upsert_max_prefix))
        // BGP convergence events (RV7-UI6)
        .route("/convergence",           get(capacity::convergence_events))
        // Filter management (RV6-1)
        .route("/filters/test",        post(filters::filter_test))
        .route("/filters/reload",      any(filters::filter_reload))
        .route("/filters/stats",       get(filters::filter_stats))
        // ML model status (RV6-5)
        .route("/ml/model/status",     get(ml::model_status))
        // Resource governor (RV8-GOV2)
        .route("/governance",          get(governance::get_governance))
        // External API integrations (RV8-EXT5)
        .route("/external/prefix-visibility", get(external::prefix_visibility))
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
        // OpenAPI spec + Swagger UI — public (RV8-OA1/OA2)
        .route("/api/openapi.json", get(openapi_spec))
        .route("/api/swagger",      get(swagger_ui))
        // MCP JSON-RPC 2.0 endpoint — public (RV8-MC1)
        .route("/mcp",              post(mcp_handler))
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
