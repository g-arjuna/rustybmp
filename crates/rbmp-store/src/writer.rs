use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{error, warn};
use metrics::counter;
use rbmp_rib::event::{RibEvent, RibEventPayload, RouteAction};
use rbmp_core::bgp::evpn::EvpnRoute;
use crate::duck::RouteStore;

const BATCH_SIZE:    usize        = 500;
const BATCH_TIMEOUT: Duration     = Duration::from_millis(50);

/// Drives a RibEvent stream into DuckDB using batched inserts.
/// Collects up to BATCH_SIZE events or flushes after BATCH_TIMEOUT — whichever first.
/// Target throughput: ≥ 1500 msg/sec.
pub async fn run_store_writer(store: Arc<std::sync::Mutex<RouteStore>>, mut rx: broadcast::Receiver<RibEvent>) {
    let mut batch: Vec<RibEvent> = Vec::with_capacity(BATCH_SIZE);

    loop {
        // Try to fill the batch up to BATCH_SIZE within BATCH_TIMEOUT
        let deadline = tokio::time::sleep(BATCH_TIMEOUT);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                biased;
                result = rx.recv() => {
                    match result {
                        Ok(ev) => {
                            batch.push(ev);
                            if batch.len() >= BATCH_SIZE { break; }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(%n, "Store writer lagged; {n} events dropped");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            // Flush remaining before exit
                            if !batch.is_empty() {
                                flush_batch(&store, &batch);
                                batch.clear();
                            }
                            return;
                        }
                    }
                }
                _ = &mut deadline => { break; }
            }
        }

        if !batch.is_empty() {
            let n = batch.len();
            flush_batch(&store, &batch);
            counter!("rib_events_written_total").increment(n as u64);
            batch.clear();
        }
    }
}

/// Flush a batch of events inside a single DuckDB transaction.
fn flush_batch(store: &std::sync::Mutex<RouteStore>, batch: &[RibEvent]) {
    let locked = match store.lock() {
        Ok(s) => s,
        Err(e) => { error!("DuckDB lock poisoned: {e}"); return; }
    };
    let conn = locked.conn();
    if let Err(e) = conn.execute_batch("BEGIN") {
        error!("DuckDB BEGIN failed: {e}"); return;
    }
    for ev in batch {
        if let Err(e) = persist_one(conn, ev) {
            error!(?e, "Failed to persist RibEvent in batch");
        }
    }
    if let Err(e) = conn.execute_batch("COMMIT") {
        error!("DuckDB COMMIT failed: {e}");
        let _ = conn.execute_batch("ROLLBACK");
    }
}

fn persist_one(conn: &duckdb::Connection, ev: &RibEvent) -> Result<()> {
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

            // Write per-route rows to evpn_events for EVPN announces
            if action == "announce" {
                if let Some(evpn) = attrs.as_ref().and_then(|a| a.evpn_reach.as_ref()) {
                    for route in &evpn.routes {
                        let (rd_s, etag, mac_s, ip_s, pfxlen, label, esi_s) = evpn_route_fields(route);
                        conn.execute(
                            "INSERT INTO evpn_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                            duckdb::params![
                                id, ts, spk,
                                rc.peer_header.peer_address.to_string(),
                                rc.peer_header.peer_as,
                                "announce",
                                route.route_type_code(),
                                route.route_type_name(),
                                rd_s, etag, mac_s, ip_s, pfxlen, label, esi_s,
                            ],
                        )?;
                    }
                }
            }
        }
        RibEventPayload::PeerUp { peer_header, local_asn, remote_asn: _, hold_time, capabilities } => {
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
            for stat in counters {
                let afi  = stat.afi_safi.map(|a| a.afi.as_u16());
                let safi = stat.afi_safi.map(|a| a.safi.as_u8());
                conn.execute(
                    "INSERT INTO stats_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        id, ts, spk,
                        peer_header.peer_address.to_string(),
                        stat.name,
                        stat.value,
                        stat.stat_type,
                        afi,
                        safi,
                    ],
                )?;
            }
        }
        RibEventPayload::EndOfRib { .. } => {
            // End-of-RIB markers are not persisted; they are ephemeral control signals.
        }
    }
    Ok(())
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join("")
}

/// Decompose an EvpnRoute into flat columns for evpn_events.
/// Returns (rd, ethernet_tag, mac, ip, prefix_len, mpls_label, esi_hex).
#[allow(clippy::type_complexity)]
fn evpn_route_fields(route: &EvpnRoute) -> (
    Option<String>, Option<u32>, Option<String>, Option<String>,
    Option<u8>, Option<u32>, Option<String>,
) {
    match route {
        EvpnRoute::EthernetAutoDiscovery { rd, esi, ethernet_tag, mpls_label } => (
            Some(hex_encode(rd)), Some(*ethernet_tag),
            None, None, None, Some(*mpls_label), Some(hex_encode(esi)),
        ),
        EvpnRoute::MacIpAdvertisement { rd, esi, ethernet_tag, mac, ip, mpls_label1, .. } => (
            Some(hex_encode(rd)), Some(*ethernet_tag),
            Some(mac.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(":")),
            ip.as_ref().map(|a| a.to_string()),
            None, Some(*mpls_label1), Some(hex_encode(esi)),
        ),
        EvpnRoute::InclusiveMulticastEthernetTag { rd, ethernet_tag, originating_router_ip } => (
            Some(hex_encode(rd)), Some(*ethernet_tag),
            None, Some(originating_router_ip.to_string()), None, None, None,
        ),
        EvpnRoute::EthernetSegment { rd, esi, originating_router_ip } => (
            Some(hex_encode(rd)), None,
            None, Some(originating_router_ip.to_string()), None, None, Some(hex_encode(esi)),
        ),
        EvpnRoute::IpPrefix { rd, esi, ethernet_tag, prefix, prefix_len, mpls_label, .. } => (
            Some(hex_encode(rd)), Some(*ethernet_tag),
            None, Some(prefix.to_string()), Some(*prefix_len), Some(*mpls_label), Some(hex_encode(esi)),
        ),
        EvpnRoute::SelectiveMulticastEthernetTag { rd, ethernet_tag, multicast_source, originating_router_ip, .. }
        | EvpnRoute::IgmpJoinSynch { rd, ethernet_tag, multicast_source, originating_router_ip, .. }
        | EvpnRoute::IgmpLeaveSynch { rd, ethernet_tag, multicast_source, originating_router_ip, .. }
        | EvpnRoute::SPmsi { rd, ethernet_tag, multicast_source, originating_router_ip, .. } => (
            Some(hex_encode(rd)), Some(*ethernet_tag),
            Some(multicast_source.to_string()),
            Some(originating_router_ip.to_string()),
            None, None, None,
        ),
        EvpnRoute::PerRegionIPmsi { rd, ethernet_tag, originating_router_ip } => (
            Some(hex_encode(rd)), Some(*ethernet_tag),
            None, Some(originating_router_ip.to_string()), None, None, None,
        ),
        EvpnRoute::LeafAD { path_id, .. } => (
            None, None, None, None, None, Some(*path_id), None,
        ),
        EvpnRoute::Unknown { .. } => (None, None, None, None, None, None, None),
    }
}
