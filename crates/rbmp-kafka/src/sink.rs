use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, debug};
use rbmp_rib::event::{RibEvent, RibEventPayload};
use crate::producer::KafkaProducer;
use crate::topics::*;

/// Consume RibEvents from the broadcast channel and publish to Kafka.
///
/// Each event type maps to a topic suffix:
/// - SpeakerUp / SpeakerDown  → `router`
/// - PeerUp / PeerDown        → `peer`
/// - RouteChange (Announce)   → `unicast_prefix`
/// - Stats                    → `bmp_stat`
/// - EndOfRib                 → `peer` (tagged as EOR)
///
/// Run this in a dedicated tokio task.  Returns when the cancellation token
/// fires or the broadcast sender is dropped.
pub async fn run_kafka_sink(
    producer: KafkaProducer,
    mut rx:   broadcast::Receiver<RibEvent>,
    cancel:   CancellationToken,
) {
    info!("Kafka sink started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Kafka sink shutting down");
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok(event) => dispatch(&producer, event).await,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(dropped = n, "Kafka sink lagged — messages dropped");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Kafka sink: broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }
}

async fn dispatch(producer: &KafkaProducer, event: RibEvent) {
    let speaker_key = event.speaker.to_string();

    match &event.payload {
        RibEventPayload::SpeakerUp { .. } | RibEventPayload::SpeakerDown { .. } => {
            producer.publish(TOPIC_ROUTER, &speaker_key, &event).await;
        }

        RibEventPayload::PeerUp { peer_header, .. }
        | RibEventPayload::PeerDown { peer_header, .. }
        | RibEventPayload::EndOfRib { peer_header, .. } => {
            let key = format!("{}:{}", speaker_key, peer_header.peer_address);
            producer.publish(TOPIC_PEER, &key, &event).await;
        }

        RibEventPayload::Stats { peer_header, .. } => {
            let key = format!("{}:{}", speaker_key, peer_header.peer_address);
            producer.publish(TOPIC_STATS, &key, &event).await;
        }

        RibEventPayload::RouteChange(rc) => {
            let key = format!("{}:{}:{}", speaker_key, rc.peer_header.peer_address, rc.prefix);

            // Route to specialised topic based on what's in the attributes
            let has_evpn = rc.attributes.as_ref()
                .map(|a| a.evpn_reach.is_some() || a.evpn_unreach.is_some())
                .unwrap_or(false);
            let has_bgpls = rc.attributes.as_ref()
                .map(|a| a.bgpls_reach.is_some() || a.bgpls_attr.is_some())
                .unwrap_or(false);

            if has_evpn {
                producer.publish(TOPIC_EVPN, &key, &event).await;
            } else if has_bgpls {
                producer.publish(TOPIC_BGPLS, &key, &event).await;
            } else {
                producer.publish(TOPIC_UNICAST, &key, &event).await;
            }

            debug!(topic = if has_evpn { TOPIC_EVPN } else if has_bgpls { TOPIC_BGPLS } else { TOPIC_UNICAST },
                   prefix = %rc.prefix, "Kafka: route change published");
        }
    }
}
