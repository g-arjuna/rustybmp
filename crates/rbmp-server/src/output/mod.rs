/// Output adapter framework (RV8-OUT1).
///
/// Defines the `OutputAdapter` async trait, a cursor-based event pump,
/// and concrete adapter implementations:
///   - `ElasticsearchAdapter`   (ECS-formatted docs, RV8-OUT2)
///   - `SplunkHecAdapter`       (Splunk HTTP Event Collector, RV8-OUT3)
///
/// Usage: adapters are spawned as background tasks in `main.rs` and
/// consume `RibEvent`s from the shared broadcast channel.
pub mod elasticsearch;
pub mod splunk;
pub mod servicenow_em;
pub mod webhook;

use std::sync::Arc;
use async_trait::async_trait;
use anyhow::Result;
use rbmp_rib::event::RibEvent;
use tokio::sync::broadcast;
use tracing::{error, info};

// ── Core trait ────────────────────────────────────────────────────────────────

/// An output adapter receives RIB events and forwards them to an external sink.
#[async_trait]
pub trait OutputAdapter: Send + Sync + 'static {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Send a batch of events to the sink.
    /// Implementations should handle retries internally.
    async fn send_batch(&self, events: &[RibEvent]) -> Result<()>;

    /// Return true if the adapter is healthy (reachable, authenticated, etc.).
    async fn is_healthy(&self) -> bool { true }
}

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OutputAdaptersConfig {
    #[serde(default)]
    pub elasticsearch: Option<elasticsearch::ElasticsearchConfig>,
    #[serde(default)]
    pub splunk: Option<splunk::SplunkConfig>,
    #[serde(default)]
    pub servicenow_em: Option<servicenow_em::ServiceNowEmConfig>,
    #[serde(default)]
    pub webhook: Option<webhook::WebhookConfig>,
}

// ── Cursor pump ───────────────────────────────────────────────────────────────

/// Batch size — number of events collected before flushing to the sink.
const BATCH_SIZE: usize = 256;

/// Spawn a background task that drains the RIB event broadcast channel and
/// forwards batches to the given adapter.
pub fn spawn_adapter_pump(
    adapter: Arc<dyn OutputAdapter>,
    mut rx: broadcast::Receiver<RibEvent>,
) {
    tokio::spawn(async move {
        let mut buf: Vec<RibEvent> = Vec::with_capacity(BATCH_SIZE);
        info!(adapter = adapter.name(), "Output adapter pump started");

        loop {
            // Collect up to BATCH_SIZE events (or whatever arrived)
            match rx.recv().await {
                Ok(ev) => {
                    buf.push(ev);
                    // Drain any additional queued events without blocking
                    while buf.len() < BATCH_SIZE {
                        match rx.try_recv() {
                            Ok(e)  => buf.push(e),
                            Err(_) => break,
                        }
                    }
                    // Flush batch
                    if let Err(e) = adapter.send_batch(&buf).await {
                        error!(adapter = adapter.name(), error = %e, "Batch send failed");
                    }
                    buf.clear();
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    error!(adapter = adapter.name(), skipped = n, "Adapter lagged — events dropped");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!(adapter = adapter.name(), "RIB broadcast channel closed — adapter stopping");
                    break;
                }
            }
        }
    });
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopAdapter;

    #[async_trait]
    impl OutputAdapter for NoopAdapter {
        fn name(&self) -> &str { "noop" }
        async fn send_batch(&self, _events: &[RibEvent]) -> Result<()> { Ok(()) }
    }

    #[test]
    fn batch_size_is_256() {
        assert_eq!(BATCH_SIZE, 256, "BATCH_SIZE must be 256 per RV8-OUT1 spec");
    }

    #[test]
    fn noop_adapter_name() {
        let a = NoopAdapter;
        assert_eq!(a.name(), "noop", "adapter name() must return the configured name");
    }

    #[tokio::test]
    async fn noop_adapter_is_healthy() {
        let a = NoopAdapter;
        assert!(a.is_healthy().await, "default is_healthy() must return true");
    }

    #[tokio::test]
    async fn noop_adapter_send_batch_empty_ok() {
        let a = NoopAdapter;
        let result = a.send_batch(&[]).await;
        assert!(result.is_ok(), "send_batch with empty slice must succeed");
    }

    #[test]
    fn arc_adapter_object_safe() {
        let a: Arc<dyn OutputAdapter> = Arc::new(NoopAdapter);
        assert_eq!(a.name(), "noop", "OutputAdapter must be object-safe (Arc<dyn OutputAdapter> works)");
    }
}
