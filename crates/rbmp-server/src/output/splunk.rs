/// Splunk HTTP Event Collector (HEC) output adapter (RV8-OUT3).
///
/// Ships `RibEvent`s to a Splunk HEC endpoint using the `/services/collector`
/// API. Events are batched into a single HTTP POST (JSON stream format).
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, warn};
use rbmp_rib::RibEvent;
use rbmp_rib::event::RibEventPayload;
use super::OutputAdapter;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct SplunkConfig {
    /// HEC endpoint, e.g. "https://splunk-host:8088"
    pub url:         String,
    /// HEC token (from Settings → Data Inputs → HTTP Event Collector)
    pub token:       String,
    /// Splunk index to write into (default: "main")
    #[serde(default = "default_index")]
    pub index:       String,
    /// Source type for all BGP events (default: "bgp:rustybmp")
    #[serde(default = "default_sourcetype")]
    pub sourcetype:  String,
    /// Skip TLS verification (for self-signed certs in dev environments)
    #[serde(default)]
    pub insecure_tls: bool,
}

fn default_index()      -> String { "main".into() }
fn default_sourcetype() -> String { "bgp:rustybmp".into() }

// ── Adapter ───────────────────────────────────────────────────────────────────

pub struct SplunkHecAdapter {
    cfg:         SplunkConfig,
    client:      Client,
    collector_url: String,
}

impl SplunkHecAdapter {
    pub fn new(cfg: SplunkConfig) -> Arc<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .danger_accept_invalid_certs(cfg.insecure_tls)
            .build()
            .expect("Splunk HEC HTTP client");

        let collector_url = format!(
            "{}/services/collector",
            cfg.url.trim_end_matches('/')
        );

        Arc::new(Self { cfg, client, collector_url })
    }

    /// Build a batched HEC payload (newline-delimited JSON events).
    fn build_hec_body(&self, events: &[RibEvent]) -> String {
        events.iter()
            .map(|ev| self.rib_event_to_hec(ev).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn rib_event_to_hec(&self, ev: &RibEvent) -> Value {
        let epoch = ev.occurred_at.timestamp_millis() as f64 / 1000.0;
        let event_body = rib_event_to_fields(ev);

        json!({
            "time":       epoch,
            "host":       ev.speaker.to_string(),
            "source":     "rustybmp",
            "sourcetype": self.cfg.sourcetype,
            "index":      self.cfg.index,
            "event":      event_body,
        })
    }
}

#[async_trait]
impl OutputAdapter for SplunkHecAdapter {
    fn name(&self) -> &str { "splunk_hec" }

    async fn send_batch(&self, events: &[RibEvent]) -> Result<()> {
        if events.is_empty() { return Ok(()); }

        let body = self.build_hec_body(events);
        debug!(count = events.len(), bytes = body.len(), "Splunk HEC batch send");

        let resp = self.client
            .post(&self.collector_url)
            .header("Authorization", format!("Splunk {}", self.cfg.token))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .context("Splunk HEC request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "Splunk HEC returned error");
            anyhow::bail!("Splunk HEC error: {}", status);
        }
        Ok(())
    }

    async fn is_healthy(&self) -> bool {
        let health_url = format!(
            "{}/services/collector/health",
            self.cfg.url.trim_end_matches('/')
        );
        self.client.get(&health_url)
            .header("Authorization", format!("Splunk {}", self.cfg.token))
            .send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

// ── Field extraction ──────────────────────────────────────────────────────────

fn rib_event_to_fields(ev: &RibEvent) -> Value {
    let mut fields = json!({
        "event_id":    ev.id.to_string(),
        "speaker":     ev.speaker.to_string(),
    });

    match &ev.payload {
        RibEventPayload::RouteChange(rc) => {
            fields["action"]    = json!(format!("{:?}", rc.action).to_lowercase());
            fields["prefix"]    = json!(rc.prefix.to_string());
            fields["peer_addr"] = json!(rc.peer_header.peer_address.to_string());
            fields["peer_as"]   = json!(rc.peer_header.peer_as);
            fields["rib_type"]  = json!(format!("{:?}", rc.rib_type));
            if let Some(attrs) = &rc.attributes {
                fields["as_path"]    = json!(attrs.as_path.as_ref().map(|p| p.to_string()));
                fields["next_hop"]   = json!(attrs.next_hop.map(|h| h.to_string()));
                fields["local_pref"] = json!(attrs.local_pref);
                fields["med"]        = json!(attrs.multi_exit_disc);
                fields["communities"]= json!(attrs.communities.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" "));
            }
        }
        RibEventPayload::PeerUp { peer_header, .. } => {
            fields["action"]    = json!("peer_up");
            fields["peer_addr"] = json!(peer_header.peer_address.to_string());
            fields["peer_as"]   = json!(peer_header.peer_as);
        }
        RibEventPayload::PeerDown { peer_header, reason } => {
            fields["action"]    = json!("peer_down");
            fields["peer_addr"] = json!(peer_header.peer_address.to_string());
            fields["peer_as"]   = json!(peer_header.peer_as);
            fields["reason"]    = json!(reason);
        }
        RibEventPayload::SpeakerUp { sys_name, .. } => {
            fields["action"]   = json!("speaker_up");
            fields["sys_name"] = json!(sys_name);
        }
        RibEventPayload::SpeakerDown { reason } => {
            fields["action"] = json!("speaker_down");
            fields["reason"] = json!(reason);
        }
        _ => { fields["action"] = json!("other"); }
    }

    fields
}
