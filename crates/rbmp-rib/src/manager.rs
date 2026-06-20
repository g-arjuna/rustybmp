use std::collections::HashMap;
use std::net::IpAddr;
use chrono::Utc;
use tokio::sync::broadcast;
use tracing::{info, debug};
use metrics::counter;
use rbmp_core::bmp::types::{BmpMessage, BmpPayload};
use rbmp_core::bgp::types::BgpCapability;
use crate::event::{RibEvent, RibEventPayload, RibEventPayload::*, RouteChange, RouteAction};
use crate::session::{BmpSession, PeerSession};
use crate::table::{RibEntry, RibTable};
use crate::filter::{FilterEngine, FilterVerdict};

/// Central RIB manager: owns all per-speaker sessions and per-peer route tables.
/// Driven by BmpMessage from the receiver; emits RibEvents to subscribers.
pub struct RibManager {
    speakers: HashMap<IpAddr, BmpSession>,
    /// peer_addr → RIB table (across all speakers; a peer is globally unique by address)
    ribs: HashMap<IpAddr, RibTable>,
    event_tx: broadcast::Sender<RibEvent>,
    /// Optional programmable route filter chain (YAML DSL)
    filter: Option<FilterEngine>,
}

impl RibManager {
    pub fn new(event_capacity: usize) -> (Self, broadcast::Receiver<RibEvent>) {
        let (tx, rx) = broadcast::channel(event_capacity);
        (Self {
            speakers: HashMap::new(),
            ribs:     HashMap::new(),
            event_tx: tx,
            filter:   None,
        }, rx)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RibEvent> {
        self.event_tx.subscribe()
    }

    /// Replace the active filter engine (called on config reload).
    pub fn set_filter(&mut self, engine: FilterEngine) {
        self.filter = Some(engine);
    }

    /// Remove the filter engine (all routes are default-accepted).
    pub fn clear_filter(&mut self) {
        self.filter = None;
    }

    /// Expire LLGR-stale routes for a peer whose stale timer has elapsed.
    /// Returns count of routes deleted.
    pub fn expire_stale_for_peer(&mut self, peer_addr: IpAddr, stale_secs: u32) -> usize {
        let now = Utc::now();
        self.ribs.get_mut(&peer_addr)
            .map(|rib| rib.drain_deleted_stale(now, stale_secs))
            .unwrap_or(0)
    }

    /// Feed a parsed BmpMessage into the RIB. Emits RibEvents on state changes.
    pub fn process(&mut self, msg: BmpMessage) {
        let speaker = msg.speaker_addr;
        let now     = msg.received_at;

        match msg.payload {
            BmpPayload::Initiation { sys_name, sys_descr, .. } => {
                let session = self.speakers.entry(speaker).or_insert_with(|| {
                    info!(%speaker, "BMP speaker connected");
                    BmpSession::new(speaker, now)
                });
                session.sys_name  = sys_name.clone();
                session.sys_descr = sys_descr.clone();
                self.emit(speaker, SpeakerUp { sys_name, sys_descr });
            }
            BmpPayload::Termination { reason_code, reason_text } => {
                if let Some(sess) = self.speakers.remove(&speaker) {
                    // Clear all peer routes for this speaker
                    for peer_addr in sess.peers.keys() {
                        self.ribs.remove(peer_addr);
                    }
                    info!(%speaker, %reason_code, "BMP speaker disconnected");
                }
                let reason = reason_text.unwrap_or_else(|| format!("code={reason_code}"));
                self.emit(speaker, SpeakerDown { reason });
            }
            BmpPayload::PeerUp(pu) => {
                let peer_addr = pu.peer_header.peer_address;
                let caps: Vec<BgpCapability> = pu.recv_open.capabilities.clone();
                let cap_names: Vec<String> = caps.iter().map(|c| format!("{:?}", c.code())).collect();

                let speaker_sess = self.speakers.entry(speaker).or_insert_with(|| BmpSession::new(speaker, now));
                let peer = speaker_sess.peers.entry(peer_addr).or_insert_with(|| PeerSession::new(&pu.peer_header));
                peer.on_up(pu.sent_open.asn, pu.recv_open.hold_time, caps, now);

                info!(%speaker, %peer_addr, peer_as = pu.peer_header.peer_as, "BGP peer up");
                self.emit(speaker, PeerUp {
                    peer_header:  pu.peer_header,
                    local_asn:    pu.sent_open.asn,
                    remote_asn:   pu.recv_open.asn,
                    hold_time:    pu.recv_open.hold_time,
                    capabilities: cap_names,
                });
            }
            BmpPayload::PeerDown { peer_header, reason } => {
                let peer_addr = peer_header.peer_address;
                let reason_str = format!("{:?}", reason);

                let llgr_stale_secs = self.speakers.get(&speaker)
                    .and_then(|s| s.peers.get(&peer_addr))
                    .filter(|p| p.llgr_active)
                    .map(|p| p.llgr_stale_secs);

                if let Some(sess) = self.speakers.get_mut(&speaker) {
                    if let Some(peer) = sess.peers.get_mut(&peer_addr) {
                        peer.on_down(now);
                    }
                }

                if let Some(stale_secs) = llgr_stale_secs {
                    // LLGR active: mark routes stale instead of deleting them
                    if let Some(rib) = self.ribs.get_mut(&peer_addr) {
                        rib.mark_stale_all(now);
                    }
                    info!(%speaker, %peer_addr, %reason_str, stale_secs, "BGP peer down — LLGR stale marking");
                } else {
                    self.ribs.entry(peer_addr).or_default().clear_all();
                    info!(%speaker, %peer_addr, %reason_str, "BGP peer down");
                }
                self.emit(speaker, PeerDown { peer_header, reason: reason_str });
            }
            BmpPayload::RouteMonitoring { peer_header, update } => {
                let peer_addr = peer_header.peer_address;
                let rib_type  = peer_header.rib_type;

                // Check for End-of-RIB
                if update.is_eor() {
                    let afi_safi = update.attributes.mp_unreach
                        .as_ref().map(|u| u.afi_safi.to_string())
                        .unwrap_or_else(|| "ipv4-unicast".to_string());
                    debug!(%speaker, %peer_addr, %afi_safi, "End-of-RIB received");
                    self.emit(speaker, EndOfRib { peer_header, afi_safi });
                    return;
                }

                let rib = self.ribs.entry(peer_addr).or_default();

                // Process withdrawals — carry path_id for Add-Path compound key
                for (prefix, path_id) in update.all_withdrawn_with_path_id() {
                    rib.remove_with_path_id(rib_type, prefix, path_id);
                    counter!("bgp_routes_withdrawn_total", "speaker" => speaker.to_string()).increment(1);
                    let ev = RouteChange {
                        action:      RouteAction::Withdraw,
                        peer_header: peer_header.clone(),
                        rib_type,
                        prefix:      prefix.clone(),
                        attributes:  None,
                    };
                    self.event_tx.send(RibEvent {
                        id:          uuid::Uuid::new_v4(),
                        occurred_at: now,
                        speaker,
                        payload:     RibEventPayload::RouteChange(ev),
                    }).ok();
                }

                // Process announcements — carry path_id from NLRI (RFC 7911)
                for (prefix, path_id) in update.all_announced_with_path_id() {
                    // Apply programmable filter before installing / emitting
                    if let Some(engine) = &self.filter {
                        let (verdict, filter_name) = engine.apply(
                            &prefix,
                            peer_header.peer_as,
                            peer_addr,
                            &update.attributes,
                        );
                        if verdict == FilterVerdict::Deny {
                            debug!(%speaker, %peer_addr, %prefix,
                                filter = filter_name.unwrap_or("?"),
                                "route denied by filter");
                            counter!("bgp_routes_filtered_total", "speaker" => speaker.to_string()).increment(1);
                            continue;
                        }
                    }

                    counter!("bgp_routes_announced_total", "speaker" => speaker.to_string()).increment(1);
                    let entry = RibEntry {
                        prefix:      prefix.clone(),
                        path_id,
                        attributes:  update.attributes.clone(),
                        received_at: now,
                        peer_addr,
                        peer_as:     peer_header.peer_as,
                        is_best:     true,
                        llgr_state:  rbmp_core::bgp::types::LlgrState::Normal,
                        stale_at:    None,
                    };
                    rib.insert(rib_type, entry);
                    // Recompute best-path when Add-Path may produce multiple paths
                    if path_id.is_some() {
                        rib.recompute_best_path(rib_type, prefix);
                    }
                    let ev = RouteChange {
                        action:      RouteAction::Announce,
                        peer_header: peer_header.clone(),
                        rib_type,
                        prefix:      prefix.clone(),
                        attributes:  Some(update.attributes.clone()),
                    };
                    self.event_tx.send(RibEvent {
                        id:          uuid::Uuid::new_v4(),
                        occurred_at: now,
                        speaker,
                        payload:     RibEventPayload::RouteChange(ev),
                    }).ok();
                }
            }
            BmpPayload::StatsReport { peer_header, stats } => {
                self.emit(speaker, Stats { peer_header, counters: stats });
            }
            BmpPayload::RouteMirroring { .. } => {
                // TODO: forward mirrored PDUs to secondary parser
            }
        }
    }

    fn emit(&self, speaker: IpAddr, payload: RibEventPayload) {
        let _ = self.event_tx.send(RibEvent {
            id:          uuid::Uuid::new_v4(),
            occurred_at: Utc::now(),
            speaker,
            payload,
        });
    }

    // ─── Query surface ────────────────────────────────────────────────────────

    pub fn speakers(&self) -> Vec<&BmpSession> {
        self.speakers.values().collect()
    }

    pub fn speaker(&self, addr: IpAddr) -> Option<&BmpSession> {
        self.speakers.get(&addr)
    }

    pub fn rib_for_peer(&self, peer: IpAddr) -> Option<&RibTable> {
        self.ribs.get(&peer)
    }

    pub fn total_routes(&self) -> usize {
        self.ribs.values().flat_map(|r| r.all_prefixes()).count()
    }

    pub fn total_peers_up(&self) -> usize {
        self.speakers.values().map(|s| s.up_peer_count()).sum()
    }

    /// Return a clone of the broadcast Sender so callers can subscribe to events.
    pub fn event_sender(&self) -> broadcast::Sender<RibEvent> {
        self.event_tx.clone()
    }
}
