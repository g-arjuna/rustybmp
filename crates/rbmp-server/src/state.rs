/// Shared application state (extracted to break auth ↔ api circular dep).
use std::sync::Arc;
use tokio::sync::broadcast;
use rbmp_rib::RibManager;
use rbmp_store::RouteStore;
use rbmp_store::query::QueryEngine;
use rbmp_enrichment::EnrichmentEngine;
use rbmp_rib::event::RibEvent;
use crate::config::{AuthConfig, SpeakerRegistry};
use metrics_exporter_prometheus::PrometheusHandle;

#[derive(Clone)]
pub struct AppState {
    pub rib:        Arc<tokio::sync::RwLock<RibManager>>,
    pub store:      Arc<std::sync::Mutex<RouteStore>>,
    pub queries:    Arc<QueryEngine>,
    pub events:     broadcast::Sender<RibEvent>,
    pub enrichment: Arc<EnrichmentEngine>,
    pub registry:   Arc<SpeakerRegistry>,
    pub prom:       PrometheusHandle,
    pub auth_cfg:   Arc<AuthConfig>,
}
