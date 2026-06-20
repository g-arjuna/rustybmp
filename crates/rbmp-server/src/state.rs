/// Shared application state (extracted to break auth ↔ api circular dep).
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::broadcast;
use rbmp_rib::RibManager;
use rbmp_store::RouteStore;
use rbmp_store::query::QueryEngine;
use rbmp_enrichment::EnrichmentEngine;
use rbmp_enrichment::CredentialVault;
use rbmp_rib::event::RibEvent;
use crate::api::policy_fetch::FetchJob;
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
    /// RV7-V1: Credential vault for SSH policy fetching
    pub vault:      Arc<CredentialVault>,
    /// RV7-V3: In-process policy fetch job registry
    pub policy_jobs: Arc<std::sync::Mutex<HashMap<String, FetchJob>>>,
}
