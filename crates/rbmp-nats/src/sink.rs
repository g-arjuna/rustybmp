/// NATS event sink — mirrors the Kafka sink's dispatch logic (RV4-7).
///
/// Subject mapping (identical taxonomy to Kafka topics):
///   SpeakerUp / SpeakerDown  → <prefix>.router
///   PeerUp / PeerDown / EOR  → <prefix>.peer
///   Stats                    → <prefix>.bmp_stat
///   RouteChange (EVPN)       → <prefix>.evpn
///   RouteChange (BGP-LS)     → <prefix>.ls_node
///   RouteChange (unicast)    → <prefix>.unicast_prefix
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, debug};
use rbmp_rib::event::{RibEvent, RibEventPayload};
use crate::publisher::NatsPublisher;

pub async fn run_nats_sink(
    publisher: NatsPublisher,
    mut rx:    broadcast::Receiver<RibEvent>,
    cancel:    CancellationToken,
) {
    info!("NATS sink started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("NATS sink shutting down");
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok(event)  => dispatch(&publisher, event).await,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(dropped = n, "NATS sink lagged — events dropped");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("NATS sink: broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }
}

async fn dispatch(p: &NatsPublisher, event: RibEvent) {
    match &event.payload {
        RibEventPayload::SpeakerUp { .. } | RibEventPayload::SpeakerDown { .. } => {
            p.publish("router", &event).await;
        }
        RibEventPayload::PeerUp { .. }
        | RibEventPayload::PeerDown { .. }
        | RibEventPayload::EndOfRib { .. } => {
            p.publish("peer", &event).await;
        }
        RibEventPayload::Stats { .. } => {
            p.publish("bmp_stat", &event).await;
        }
        RibEventPayload::RouteChange(rc) => {
            let has_evpn = rc.attributes.as_ref()
                .map(|a| a.evpn_reach.is_some() || a.evpn_unreach.is_some())
                .unwrap_or(false);
            let has_bgpls = rc.attributes.as_ref()
                .map(|a| a.bgpls_reach.is_some() || a.bgpls_attr.is_some())
                .unwrap_or(false);

            let suffix = if has_evpn {
                "evpn"
            } else if has_bgpls {
                "ls_node"
            } else {
                "unicast_prefix"
            };

            debug!(suffix, prefix = %rc.prefix, "NATS: route change published");
            p.publish(suffix, &event).await;
        }
    }
}
