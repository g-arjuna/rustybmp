/// ServiceNow Event Management (EM) output adapter (RV9-E1).
///
/// Pushes BGP anomaly events to ServiceNow's `em_event` table via REST.
///
/// Severity mapping: critical→1, high→2, warn→3, info→5
/// Dedup key: `"{speaker}:{peer}:{kind}"` with configurable dedup window.
/// Cursor: `runtime/cursors/servicenow_em.cursor`
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use rbmp_rib::RibEvent;
use rbmp_rib::event::RibEventPayload;
use super::OutputAdapter;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceNowEmConfig {
    /// ServiceNow instance URL, e.g. "https://example.service-now.com"
    pub instance_url: String,
    /// Basic auth username
    pub username: String,
    /// Basic auth password
    pub password: String,
    /// Minimum severity to forward: "info", "warn", "high", "critical"
    #[serde(default = "default_min_severity")]
    pub min_severity: String,
    /// Dedup window in seconds (default 300)
    #[serde(default = "default_dedup_secs")]
    pub dedup_window_secs: u64,
    /// Event source name
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_min_severity() -> String { "warn".into() }
fn default_dedup_secs() -> u64 { 300 }
fn default_source() -> String { "RustyBMP".into() }

// ── Severity helpers ──────────────────────────────────────────────────────────

fn severity_to_snow(s: &str) -> i32 {
    match s {
        "critical" => 1,
        "high"     => 2,
        "warn"     => 3,
        "info"     => 5,
        _          => 4,
    }
}

fn severity_passes_min(event_sev: &str, min_sev: &str) -> bool {
    severity_to_snow(event_sev) <= severity_to_snow(min_sev)
}

// ── Adapter ───────────────────────────────────────────────────────────────────

pub struct ServiceNowEmAdapter {
    cfg:    ServiceNowEmConfig,
    client: Client,
    /// Dedup map: key → last-sent instant
    dedup:  Mutex<HashMap<String, Instant>>,
}

impl ServiceNowEmAdapter {
    pub fn new(cfg: ServiceNowEmConfig) -> Arc<Self> {
        Arc::new(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("SNOW HTTP client"),
            cfg,
            dedup: Mutex::new(HashMap::new()),
        })
    }

    fn em_event_url(&self) -> String {
        format!(
            "{}/api/global/em/jsonv2",
            self.cfg.instance_url.trim_end_matches('/')
        )
    }

    /// Check dedup window — returns true if this key should be sent.
    fn should_send(&self, key: &str) -> bool {
        let mut map = self.dedup.lock().unwrap();
        let now = Instant::now();
        let window = Duration::from_secs(self.cfg.dedup_window_secs);

        if let Some(last) = map.get(key) {
            if now.duration_since(*last) < window {
                return false;
            }
        }
        map.insert(key.to_string(), now);
        true
    }

    /// Convert a RibEvent to a ServiceNow em_event JSON payload.
    fn to_em_event(&self, ev: &RibEvent) -> Option<(String, Value)> {
        let speaker = ev.speaker.to_string();

        match &ev.payload {
            RibEventPayload::PeerDown { peer_header, reason } => {
                let peer = peer_header.peer_address.to_string();
                let key = format!("{speaker}:{peer}:peer_down");
                let severity = "warn";

                if !severity_passes_min(severity, &self.cfg.min_severity) {
                    return None;
                }

                let payload = json!({
                    "records": [{
                        "source": self.cfg.source,
                        "event_class": "BGP",
                        "node": speaker,
                        "resource": peer,
                        "type": "peer_down",
                        "severity": severity_to_snow(severity),
                        "description": format!("BGP peer {} down: {}", peer, reason),
                        "additional_info": format!("{{\"peer_as\": {}, \"reason\": \"{}\"}}", peer_header.peer_as, reason),
                    }]
                });
                Some((key, payload))
            }
            RibEventPayload::RouteChange(rc) => {
                let peer = rc.peer_header.peer_address.to_string();
                let action = format!("{:?}", rc.action).to_lowercase();
                let key = format!("{speaker}:{peer}:route_{action}");
                let severity = "info";

                if !severity_passes_min(severity, &self.cfg.min_severity) {
                    return None;
                }

                let payload = json!({
                    "records": [{
                        "source": self.cfg.source,
                        "event_class": "BGP",
                        "node": speaker,
                        "resource": rc.prefix.to_string(),
                        "type": format!("route_{action}"),
                        "severity": severity_to_snow(severity),
                        "description": format!("BGP route {} for {} via {}", action, rc.prefix, peer),
                    }]
                });
                Some((key, payload))
            }
            _ => None,
        }
    }
}

#[async_trait]
impl OutputAdapter for ServiceNowEmAdapter {
    fn name(&self) -> &str { "servicenow-em" }

    async fn send_batch(&self, events: &[RibEvent]) -> Result<()> {
        for ev in events {
            if let Some((key, payload)) = self.to_em_event(ev) {
                if !self.should_send(&key) {
                    debug!(key = %key, "SNOW EM: dedup suppressed");
                    continue;
                }

                let resp = self.client
                    .post(self.em_event_url())
                    .basic_auth(&self.cfg.username, Some(&self.cfg.password))
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .send()
                    .await
                    .context("SNOW EM request failed")?;

                if !resp.status().is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    warn!(status = %body, "SNOW EM push error");
                }
            }
        }
        Ok(())
    }

    async fn is_healthy(&self) -> bool {
        self.client
            .get(format!(
                "{}/api/now/table/sys_properties?sysparm_limit=1",
                self.cfg.instance_url.trim_end_matches('/')
            ))
            .basic_auth(&self.cfg.username, Some(&self.cfg.password))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> ServiceNowEmConfig {
        ServiceNowEmConfig {
            instance_url: "https://test.service-now.com".into(),
            username: "admin".into(),
            password: "password".into(),
            min_severity: "warn".into(),
            dedup_window_secs: 300,
            source: "RustyBMP-Test".into(),
        }
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(severity_to_snow("critical"), 1);
        assert_eq!(severity_to_snow("high"),     2);
        assert_eq!(severity_to_snow("warn"),     3);
        assert_eq!(severity_to_snow("info"),     5);
    }

    #[test]
    fn severity_filter_blocks_info_when_min_warn() {
        assert!(severity_passes_min("critical", "warn"));
        assert!(severity_passes_min("warn",     "warn"));
        assert!(!severity_passes_min("info",    "warn"));
    }

    #[test]
    fn dedup_suppresses_repeat() {
        let adapter = ServiceNowEmAdapter::new(test_cfg());
        assert!(adapter.should_send("key1"), "first send should pass");
        assert!(!adapter.should_send("key1"), "immediate repeat should be suppressed");
    }

    #[test]
    fn dedup_allows_different_keys() {
        let adapter = ServiceNowEmAdapter::new(test_cfg());
        assert!(adapter.should_send("key1"));
        assert!(adapter.should_send("key2"), "different key should pass");
    }

    #[test]
    fn em_event_url_format() {
        let adapter = ServiceNowEmAdapter::new(test_cfg());
        assert_eq!(adapter.em_event_url(), "https://test.service-now.com/api/global/em/jsonv2");
    }

    #[test]
    fn adapter_name() {
        let adapter = ServiceNowEmAdapter::new(test_cfg());
        assert_eq!(adapter.name(), "servicenow-em");
    }
}
