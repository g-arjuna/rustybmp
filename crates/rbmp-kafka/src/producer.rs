use std::time::Duration;
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::Serialize;
use tracing::{debug, warn};
use metrics::counter;
use crate::error::KafkaError;

// ─── KafkaProducer ────────────────────────────────────────────────────────────

/// Thin async wrapper around rdkafka FutureProducer.
///
/// All `publish` calls are fire-and-forget with a short timeout (5 s).
/// Delivery failures are logged and counted but never propagate to callers
/// so they don't stall the main RIB event loop.
#[derive(Clone)]
pub struct KafkaProducer {
    inner:  FutureProducer,
    prefix: String,
}

impl KafkaProducer {
    /// Connect to Kafka brokers.
    ///
    /// `brokers` — comma-separated broker list, e.g. `"localhost:9092"`
    /// `prefix`  — topic prefix, e.g. `"rustybmp"` → topics like `rustybmp.peer`
    pub fn new(brokers: &str, prefix: &str) -> Result<Self, KafkaError> {
        let inner: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.messages", "100000")
            .set("queue.buffering.max.ms", "50")         // 50 ms linger
            .set("compression.type", "lz4")
            .create()
            .map_err(KafkaError::Create)?;

        Ok(Self { inner, prefix: prefix.to_string() })
    }

    /// Publish a JSON-serialisable value to `{prefix}.{topic_suffix}`.
    /// `key` is used as the Kafka message key (for partition affinity).
    pub async fn publish<T: Serialize>(
        &self,
        topic_suffix: &str,
        key:          &str,
        value:        &T,
    ) {
        let topic = crate::topics::topic(&self.prefix, topic_suffix);
        let payload = match serde_json::to_vec(value) {
            Ok(b)  => b,
            Err(e) => {
                warn!(topic = %topic, error = %e, "Kafka: JSON serialisation failed");
                return;
            }
        };

        let record: FutureRecord<str, [u8]> = FutureRecord::to(&topic)
            .key(key)
            .payload(&payload);

        match self.inner.send(record, Duration::from_secs(5)).await {
            Ok((partition, offset)) => {
                debug!(topic = %topic, partition, offset, "Kafka: message delivered");
                counter!("kafka_messages_sent_total", "topic" => topic).increment(1);
            }
            Err((e, _msg)) => {
                warn!(topic = %topic, error = %e, "Kafka: delivery failed");
                counter!("kafka_send_errors_total", "topic" => topic).increment(1);
            }
        }
    }

    /// Publish raw bytes (for `bmp_raw` topic — no JSON overhead).
    pub async fn publish_raw(&self, topic_suffix: &str, key: &str, payload: &[u8]) {
        let topic = crate::topics::topic(&self.prefix, topic_suffix);
        let record: FutureRecord<str, [u8]> = FutureRecord::to(&topic)
            .key(key)
            .payload(payload);

        match self.inner.send(record, Duration::from_secs(5)).await {
            Ok(_) => {
                counter!("kafka_messages_sent_total", "topic" => topic).increment(1);
            }
            Err((e, _)) => {
                warn!(topic = %topic, error = %e, "Kafka: raw delivery failed");
                counter!("kafka_send_errors_total", "topic" => topic).increment(1);
            }
        }
    }
}
