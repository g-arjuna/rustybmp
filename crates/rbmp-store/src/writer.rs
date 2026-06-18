use std::sync::Arc;
use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, warn};
use rbmp_rib::event::{RibEvent, RibEventPayload, RouteAction};
use crate::duck::RouteStore;

/// Drives a RibEvent stream into DuckDB. Runs as a tokio task.
pub async fn run_store_writer(store: Arc<std::sync::Mutex<RouteStore>>, mut rx: broadcast::Receiver<RibEvent>) {
    loop {
        match rx.recv().await {
            Ok(ev) => {
                if let Err(e) = persist(&store, &ev) {
                    error!(?e, "Failed to persist RibEvent to DuckDB");
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(%n, "Store writer lagged; {n} events dropped");
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

fn persist(store: &std::sync::Mutex<RouteStore>, ev: &RibEvent) -> Result<()> {
    let locked = store.lock().unwrap();
    let conn = locked.conn();
    let id  = ev.id.to_string();
    let ts  = ev.occurred_at.to_rfc3339();
    let spk = ev.speaker.to_string();

    match &ev.payload {
        RibEventPayload::RouteChange(rc) => {
            let action = match rc.action {
                RouteAction::Announce => "announce",
                RouteAction::Withdraw => "withdraw",
            };
            let attrs = &rc.attributes;
            let as_path_str = attrs.as_ref().and_then(|a| a.as_path.as_ref())
                .map(|p| p.to_string());
            let as_path_len = attrs.as_ref().and_then(|a| a.as_path.as_ref())
                .map(|p| p.hop_count() as u16);
            let next_hop   = attrs.as_ref().and_then(|a| a.next_hop).map(|h| h.to_string());
            let local_pref = attrs.as_ref().and_then(|a| a.local_pref);
            let med        = attrs.as_ref().and_then(|a| a.multi_exit_disc);
            let origin     = attrs.as_ref().and_then(|a| a.origin).map(|o| format!("{}", o));
            let communities = attrs.as_ref().map(|a| {
                a.communities.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
            });
            let ext_communities = attrs.as_ref().map(|a| {
                a.extended_communities.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
            });
            let large_communities = attrs.as_ref().map(|a| {
                a.large_communities.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
            });
            let afi = format!("{}", rc.prefix.addr_family().as_u16());

            conn.execute(
                "INSERT INTO route_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    id, ts, spk,
                    rc.peer_header.peer_address.to_string(),
                    rc.peer_header.peer_as,
                    format!("{:?}", rc.rib_type),
                    action,
                    rc.prefix.to_string(),
                    afi,
                    origin,
                    as_path_str,
                    as_path_len,
                    next_hop,
                    local_pref,
                    med,
                    communities,
                    ext_communities,
                    large_communities,
                    attrs.as_ref().and_then(|a| a.originator_id).map(|o| o.to_string()),
                    attrs.as_ref().map(|a| a.atomic_aggregate).unwrap_or(false),
                ],
            )?;
        }
        RibEventPayload::PeerUp { peer_header, local_asn, remote_asn, hold_time, capabilities } => {
            conn.execute(
                "INSERT INTO peer_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    id, ts, spk,
                    peer_header.peer_address.to_string(),
                    peer_header.peer_as,
                    "peer_up",
                    local_asn,
                    hold_time,
                    serde_json::to_string(capabilities).unwrap_or_default(),
                    duckdb::types::Null,
                ],
            )?;
        }
        RibEventPayload::PeerDown { peer_header, reason } => {
            conn.execute(
                "INSERT INTO peer_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    id, ts, spk,
                    peer_header.peer_address.to_string(),
                    peer_header.peer_as,
                    "peer_down",
                    duckdb::types::Null,
                    duckdb::types::Null,
                    duckdb::types::Null,
                    reason,
                ],
            )?;
        }
        RibEventPayload::SpeakerUp { sys_name, sys_descr } => {
            conn.execute(
                "INSERT INTO speaker_events VALUES (?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![id, ts, spk, "speaker_up", sys_name, sys_descr, duckdb::types::Null],
            )?;
        }
        RibEventPayload::SpeakerDown { reason } => {
            conn.execute(
                "INSERT INTO speaker_events VALUES (?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![id, ts, spk, "speaker_down", duckdb::types::Null, duckdb::types::Null, reason],
            )?;
        }
        RibEventPayload::Stats { peer_header, counters } => {
            for (name, value) in counters {
                conn.execute(
                    "INSERT INTO stats_events VALUES (?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        id, ts, spk,
                        peer_header.peer_address.to_string(),
                        name,
                        value,
                    ],
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}
