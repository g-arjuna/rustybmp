/// Elasticsearch output adapter — ECS-formatted BGP route events (RV8-OUT2).
///
/// Ships `RibEvent`s to an Elasticsearch index using the `_bulk` API.
/// Event format follows Elastic Common Schema (ECS) with a custom
/// `bgp.*` field group.
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, warn};
use rbmp_rib::RibEvent;
use rbmp_rib::event::RibEventPayload;
use super::OutputAdapter;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ElasticsearchConfig {
    /// Elasticsearch base URL, e.g. "http://localhost:9200"
    pub url:       String,
    /// Index name (supports date math), e.g. "bgp-events-{now/d}"
    pub index:     String,
    /// Optional Basic Auth
    pub username:  Option<String>,
    pub password:  Option<String>,
    /// Optional API key (Base64-encoded id:api_key)
    pub api_key:   Option<String>,
    /// Max bulk body size in bytes before flushing (default 5 MB)
    #[serde(default = "default_max_body")]
    pub max_body_bytes: usize,
}

fn default_max_body() -> usize { 5 * 1024 * 1024 }

// ── Adapter ───────────────────────────────────────────────────────────────────

pub struct ElasticsearchAdapter {
    cfg:    ElasticsearchConfig,
    client: Client,
}

impl ElasticsearchAdapter {
    pub fn new(cfg: ElasticsearchConfig) -> Arc<Self> {
        Arc::new(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("ES HTTP client"),
            cfg,
        })
    }

    fn bulk_url(&self) -> String {
        format!("{}/_bulk", self.cfg.url.trim_end_matches('/'))
    }

    /// Serialize a batch of `RibEvent`s into an NDJSON `_bulk` body.
    fn build_bulk_body(&self, events: &[RibEvent]) -> String {
        let mut body = String::new();
        for ev in events {
            let doc = rib_event_to_ecs(ev);
            let meta = json!({ "index": { "_index": self.cfg.index } });
            body.push_str(&meta.to_string());
            body.push('\n');
            body.push_str(&doc.to_string());
            body.push('\n');
        }
        body
    }
}

#[async_trait]
impl OutputAdapter for ElasticsearchAdapter {
    fn name(&self) -> &str { "elasticsearch" }

    async fn send_batch(&self, events: &[RibEvent]) -> Result<()> {
        if events.is_empty() { return Ok(()); }

        let body = self.build_bulk_body(events);
        debug!(count = events.len(), bytes = body.len(), "ES bulk send");

        let mut req = self.client
            .post(self.bulk_url())
            .header("Content-Type", "application/x-ndjson")
            .body(body);

        if let Some(key) = &self.cfg.api_key {
            req = req.header("Authorization", format!("ApiKey {key}"));
        } else if let (Some(user), Some(pass)) = (&self.cfg.username, &self.cfg.password) {
            req = req.basic_auth(user, Some(pass));
        }

        let resp = req.send().await.context("ES bulk request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "ES bulk returned error");
            anyhow::bail!("ES bulk error: {}", status);
        }
        Ok(())
    }

    async fn is_healthy(&self) -> bool {
        self.client.get(format!("{}/_cluster/health", self.cfg.url))
            .send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

// ── ECS mapping ───────────────────────────────────────────────────────────────

/// Convert a `RibEvent` to an ECS-compliant JSON document.
fn rib_event_to_ecs(ev: &RibEvent) -> Value {
    let base = json!({
        "@timestamp": ev.occurred_at.to_rfc3339(),
        "event.id":      ev.id.to_string(),
        "event.module":  "bgp",
        "event.dataset": "bgp.route",
        "observer.ip":   ev.speaker.to_string(),
    });

    let mut doc = base;

    match &ev.payload {
        RibEventPayload::RouteChange(rc) => {
            doc["event.action"]       = json!(format!("{:?}", rc.action).to_lowercase());
            doc["event.category"]     = json!(["network"]);
            doc["event.type"]         = json!(["change"]);
            doc["bgp.prefix"]         = json!(rc.prefix.to_string());
            doc["bgp.peer_addr"]      = json!(rc.peer_header.peer_address.to_string());
            doc["bgp.peer_as"]        = json!(rc.peer_header.peer_as);
            doc["bgp.rib_type"]       = json!(format!("{:?}", rc.rib_type));
            if let Some(attrs) = &rc.attributes {
                doc["bgp.as_path"]    = json!(attrs.as_path.as_ref().map(|p| p.to_string()));
                doc["bgp.next_hop"]   = json!(attrs.next_hop.map(|h| h.to_string()));
                doc["bgp.local_pref"] = json!(attrs.local_pref);
                doc["bgp.med"]        = json!(attrs.multi_exit_disc);
                doc["bgp.communities"]= json!(attrs.communities.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" "));
                doc["bgp.origin"]     = json!(attrs.origin.as_ref().map(|o| format!("{:?}", o)));
            }
        }
        RibEventPayload::PeerUp { peer_header, .. } => {
            doc["event.action"]   = json!("peer_up");
            doc["event.category"] = json!(["network"]);
            doc["event.type"]     = json!(["connection"]);
            doc["bgp.peer_addr"]  = json!(peer_header.peer_address.to_string());
            doc["bgp.peer_as"]    = json!(peer_header.peer_as);
        }
        RibEventPayload::PeerDown { peer_header, reason } => {
            doc["event.action"]   = json!("peer_down");
            doc["event.category"] = json!(["network"]);
            doc["event.type"]     = json!(["connection"]);
            doc["bgp.peer_addr"]  = json!(peer_header.peer_address.to_string());
            doc["bgp.peer_as"]    = json!(peer_header.peer_as);
            doc["bgp.reason"]     = json!(reason);
        }
        RibEventPayload::SpeakerUp { sys_name, .. } => {
            doc["event.action"]   = json!("speaker_up");
            doc["bgp.sys_name"]   = json!(sys_name);
        }
        RibEventPayload::SpeakerDown { reason } => {
            doc["event.action"]   = json!("speaker_down");
            doc["bgp.reason"]     = json!(reason);
        }
        _ => {
            doc["event.action"] = json!("other");
        }
    }

    doc
}

// ── ECS index template (for documentation/setup scripts) ─────────────────────

#[derive(Debug, Serialize)]
pub struct EcsIndexTemplate {
    pub index_patterns: Vec<String>,
    pub mappings: Value,
}

impl EcsIndexTemplate {
    pub fn bgp_events() -> Self {
        Self {
            index_patterns: vec!["bgp-events-*".into()],
            mappings: json!({
                "properties": {
                    "@timestamp":     { "type": "date" },
                    "event.id":       { "type": "keyword" },
                    "event.action":   { "type": "keyword" },
                    "event.module":   { "type": "keyword" },
                    "observer.ip":    { "type": "ip" },
                    "bgp.prefix":     { "type": "ip_range" },
                    "bgp.peer_addr":  { "type": "ip" },
                    "bgp.peer_as":    { "type": "long" },
                    "bgp.as_path":    { "type": "keyword" },
                    "bgp.next_hop":   { "type": "ip" },
                    "bgp.local_pref": { "type": "long" },
                    "bgp.med":        { "type": "long" },
                    "bgp.communities":{ "type": "keyword" },
                    "bgp.origin":     { "type": "keyword" },
                    "bgp.rib_type":   { "type": "keyword" },
                    "bgp.reason":     { "type": "keyword" }
                }
            }),
        }
    }
}
