/// Webhook output adapter — profile-driven multi-platform notifications (RV9-E2).
///
/// Built-in profiles:
///   - `Slack`      — Blocks API payload
///   - `PagerDuty`  — Events v2 (with dedup_key)
///   - `OpsGenie`   — message + alias + priority
///   - `Teams`      — Adaptive Cards
///   - `Custom`     — raw body template + configurable headers
///
/// All profiles honor `min_severity` filter and `dedup_window_secs`.
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
pub struct WebhookConfig {
    /// Webhook URL (Slack incoming webhook, PD events endpoint, etc.)
    pub url: String,
    /// Profile: "slack", "pagerduty", "opsgenie", "teams", "custom"
    #[serde(default = "default_profile")]
    pub profile: String,
    /// Optional auth header value (e.g. "Bearer xxx" or PD routing key)
    pub auth_header: Option<String>,
    /// Minimum severity to forward
    #[serde(default = "default_min_severity")]
    pub min_severity: String,
    /// Dedup window in seconds
    #[serde(default = "default_dedup_secs")]
    pub dedup_window_secs: u64,
    /// Custom headers (for Custom profile)
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Custom body template (Handlebars-style, for Custom profile)
    pub body_template: Option<String>,
}

fn default_profile() -> String { "slack".into() }
fn default_min_severity() -> String { "warn".into() }
fn default_dedup_secs() -> u64 { 300 }

// ── Severity helpers ──────────────────────────────────────────────────────────

fn severity_rank(s: &str) -> i32 {
    match s {
        "critical" => 1,
        "high"     => 2,
        "warn"     => 3,
        "info"     => 5,
        _          => 4,
    }
}

fn passes_min(event_sev: &str, min_sev: &str) -> bool {
    severity_rank(event_sev) <= severity_rank(min_sev)
}

// ── Profile formatters ───────────────────────────────────────────────────────

fn format_slack(speaker: &str, summary: &str, severity: &str) -> Value {
    let color = match severity {
        "critical" => "#FF0000",
        "warn"     => "#FFA500",
        "info"     => "#36A64F",
        _          => "#808080",
    };
    json!({
        "attachments": [{
            "color": color,
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!("*[{}]* {} — {}", severity.to_uppercase(), speaker, summary)
                    }
                }
            ]
        }]
    })
}

fn format_pagerduty(speaker: &str, summary: &str, severity: &str, dedup_key: &str, routing_key: &str) -> Value {
    let pd_severity = match severity {
        "critical" => "critical",
        "high"     => "error",
        "warn"     => "warning",
        _          => "info",
    };
    json!({
        "routing_key": routing_key,
        "event_action": "trigger",
        "dedup_key": dedup_key,
        "payload": {
            "summary": format!("{}: {}", speaker, summary),
            "source": speaker,
            "severity": pd_severity,
            "component": "bgp",
            "group": "RustyBMP"
        }
    })
}

fn format_opsgenie(speaker: &str, summary: &str, severity: &str, dedup_key: &str) -> Value {
    let priority = match severity {
        "critical" => "P1",
        "high"     => "P2",
        "warn"     => "P3",
        _          => "P5",
    };
    json!({
        "message": format!("{}: {}", speaker, summary),
        "alias": dedup_key,
        "priority": priority,
        "source": "RustyBMP",
        "tags": ["bgp", "network"]
    })
}

fn format_teams(speaker: &str, summary: &str, severity: &str) -> Value {
    let color = match severity {
        "critical" => "attention",
        "warn"     => "warning",
        _          => "good",
    };
    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": [{
                    "type": "TextBlock",
                    "text": format!("[{}] {} — {}", severity.to_uppercase(), speaker, summary),
                    "weight": "bolder",
                    "color": color
                }]
            }
        }]
    })
}

// ── Adapter ───────────────────────────────────────────────────────────────────

pub struct WebhookAdapter {
    cfg:    WebhookConfig,
    client: Client,
    dedup:  Mutex<HashMap<String, Instant>>,
}

impl WebhookAdapter {
    pub fn new(cfg: WebhookConfig) -> Arc<Self> {
        Arc::new(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("Webhook HTTP client"),
            cfg,
            dedup: Mutex::new(HashMap::new()),
        })
    }

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

    fn build_payload(&self, speaker: &str, summary: &str, severity: &str, dedup_key: &str) -> Value {
        let routing_key = self.cfg.auth_header.as_deref().unwrap_or("");
        match self.cfg.profile.as_str() {
            "slack"     => format_slack(speaker, summary, severity),
            "pagerduty" => format_pagerduty(speaker, summary, severity, dedup_key, routing_key),
            "opsgenie"  => format_opsgenie(speaker, summary, severity, dedup_key),
            "teams"     => format_teams(speaker, summary, severity),
            "custom"    => {
                // Simple template substitution
                if let Some(tmpl) = &self.cfg.body_template {
                    let body = tmpl
                        .replace("{{speaker}}", speaker)
                        .replace("{{summary}}", summary)
                        .replace("{{severity}}", severity)
                        .replace("{{dedup_key}}", dedup_key);
                    serde_json::from_str(&body).unwrap_or_else(|_| json!({"text": body}))
                } else {
                    json!({"speaker": speaker, "summary": summary, "severity": severity})
                }
            }
            _ => format_slack(speaker, summary, severity),
        }
    }

    fn event_to_notification(&self, ev: &RibEvent) -> Option<(String, String, String)> {
        let speaker = ev.speaker.to_string();
        match &ev.payload {
            RibEventPayload::PeerDown { peer_header, reason } => {
                let peer = peer_header.peer_address.to_string();
                let key = format!("{speaker}:{peer}:peer_down");
                let summary = format!("Peer {} down: {}", peer, reason);
                Some((key, summary, "warn".into()))
            }
            RibEventPayload::PeerUp { peer_header, .. } => {
                let peer = peer_header.peer_address.to_string();
                let key = format!("{speaker}:{peer}:peer_up");
                let summary = format!("Peer {} up", peer);
                Some((key, summary, "info".into()))
            }
            RibEventPayload::SpeakerDown { reason } => {
                let key = format!("{speaker}:speaker_down");
                let summary = format!("Speaker down: {}", reason);
                Some((key, summary, "critical".into()))
            }
            _ => None,
        }
    }
}

#[async_trait]
impl OutputAdapter for WebhookAdapter {
    fn name(&self) -> &str { "webhook" }

    async fn send_batch(&self, events: &[RibEvent]) -> Result<()> {
        for ev in events {
            if let Some((key, summary, severity)) = self.event_to_notification(ev) {
                if !passes_min(&severity, &self.cfg.min_severity) {
                    continue;
                }
                if !self.should_send(&key) {
                    debug!(key = %key, "Webhook: dedup suppressed");
                    continue;
                }

                let payload = self.build_payload(
                    &ev.speaker.to_string(),
                    &summary,
                    &severity,
                    &key,
                );

                let mut req = self.client
                    .post(&self.cfg.url)
                    .header("Content-Type", "application/json")
                    .json(&payload);

                // Add auth header if provided and not PagerDuty (which uses routing_key in body)
                if self.cfg.profile != "pagerduty" {
                    if let Some(auth) = &self.cfg.auth_header {
                        req = req.header("Authorization", auth.as_str());
                    }
                }

                // Add custom headers
                for (k, v) in &self.cfg.headers {
                    req = req.header(k.as_str(), v.as_str());
                }

                let resp = req.send().await.context("Webhook send failed")?;
                if !resp.status().is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    warn!(
                        profile = %self.cfg.profile,
                        status = %body,
                        "Webhook push error"
                    );
                }
            }
        }
        Ok(())
    }

    async fn is_healthy(&self) -> bool {
        // For most webhooks there's no health endpoint — just check DNS/TCP
        self.client
            .head(&self.cfg.url)
            .send()
            .await
            .is_ok()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg(profile: &str) -> WebhookConfig {
        WebhookConfig {
            url: "https://hooks.example.com/test".into(),
            profile: profile.into(),
            auth_header: Some("test-key".into()),
            min_severity: "warn".into(),
            dedup_window_secs: 300,
            headers: HashMap::new(),
            body_template: None,
        }
    }

    #[test]
    fn slack_payload_has_attachments() {
        let payload = format_slack("10.0.0.1", "peer down", "warn");
        assert!(payload["attachments"].is_array());
    }

    #[test]
    fn pagerduty_payload_has_routing_key() {
        let payload = format_pagerduty("10.0.0.1", "test", "critical", "key1", "rk123");
        assert_eq!(payload["routing_key"], "rk123");
        assert_eq!(payload["payload"]["severity"], "critical");
    }

    #[test]
    fn opsgenie_payload_has_priority() {
        let payload = format_opsgenie("10.0.0.1", "test", "critical", "key1");
        assert_eq!(payload["priority"], "P1");
    }

    #[test]
    fn teams_payload_is_adaptive_card() {
        let payload = format_teams("10.0.0.1", "test", "warn");
        assert!(payload["attachments"][0]["contentType"]
            .as_str()
            .unwrap()
            .contains("adaptive"));
    }

    #[test]
    fn severity_rank_ordering() {
        assert!(severity_rank("critical") < severity_rank("warn"));
        assert!(severity_rank("warn") < severity_rank("info"));
    }

    #[test]
    fn passes_min_filter() {
        assert!(passes_min("critical", "warn"));
        assert!(passes_min("warn", "warn"));
        assert!(!passes_min("info", "warn"));
    }

    #[test]
    fn dedup_suppresses() {
        let adapter = WebhookAdapter::new(test_cfg("slack"));
        assert!(adapter.should_send("k1"));
        assert!(!adapter.should_send("k1"));
    }

    #[test]
    fn adapter_name() {
        let adapter = WebhookAdapter::new(test_cfg("slack"));
        assert_eq!(adapter.name(), "webhook");
    }

    #[test]
    fn custom_profile_with_template() {
        let mut cfg = test_cfg("custom");
        cfg.body_template = Some(r#"{"text": "{{speaker}}: {{summary}}"}"#.into());
        let adapter = WebhookAdapter::new(cfg);
        let payload = adapter.build_payload("10.0.0.1", "peer down", "warn", "key");
        assert_eq!(payload["text"], "10.0.0.1: peer down");
    }
}
