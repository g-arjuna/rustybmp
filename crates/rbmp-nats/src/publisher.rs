/// NATS publisher — wraps async-nats Client (RV4-7).
use anyhow::Result;
use async_nats::Client;
use serde::Serialize;
use tracing::warn;

pub struct NatsPublisher {
    client:         Client,
    subject_prefix: String,
}

impl NatsPublisher {
    /// Connect to NATS and return a publisher.
    pub async fn connect(server: &str, subject_prefix: &str) -> Result<Self> {
        let client = async_nats::connect(server).await?;
        Ok(Self {
            client,
            subject_prefix: subject_prefix.to_string(),
        })
    }

    /// Publish a JSON-serialised event to `<prefix>.<suffix>`.
    pub async fn publish<T: Serialize>(&self, suffix: &str, payload: &T) {
        let subject = if self.subject_prefix.is_empty() {
            suffix.to_string()
        } else {
            format!("{}.{}", self.subject_prefix, suffix)
        };

        match serde_json::to_vec(payload) {
            Ok(bytes) => {
                if let Err(e) = self.client.publish(subject.clone(), bytes.into()).await {
                    warn!(subject, error = %e, "NATS publish failed");
                }
            }
            Err(e) => warn!(suffix, error = %e, "NATS: serialization failed"),
        }
    }
}
