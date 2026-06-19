use thiserror::Error;

#[derive(Debug, Error)]
pub enum KafkaError {
    #[error("failed to create Kafka producer: {0}")]
    Create(rdkafka::error::KafkaError),
}
