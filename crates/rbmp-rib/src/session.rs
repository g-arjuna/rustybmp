use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use rbmp_core::bgp::types::BgpCapability;
use rbmp_core::bmp::types::{RibType, PeerHeader};

/// Lifecycle state of a BGP peer as seen via BMP
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerState {
    Up,
    Down,
    /// Session unknown — BMP speaker connected but Peer Up not yet received
    Unknown,
}

/// A BGP peer session tracked by the RIB engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSession {
    pub peer_address: IpAddr,
    pub peer_as:      u32,
    pub peer_bgp_id:  Ipv4Addr,
    pub local_as:     u32,
    pub hold_time:    u16,
    pub state:        PeerState,
    pub up_at:        Option<DateTime<Utc>>,
    pub down_at:      Option<DateTime<Utc>>,
    pub capabilities: Vec<BgpCapability>,
    /// Counts of routes per RIB type currently held for this peer
    pub route_counts: HashMap<RibType, usize>,
    pub flap_count:   u32,
}

impl PeerSession {
    pub fn new(hdr: &PeerHeader) -> Self {
        Self {
            peer_address: hdr.peer_address,
            peer_as:      hdr.peer_as,
            peer_bgp_id:  hdr.peer_bgp_id,
            local_as:     0,
            hold_time:    0,
            state:        PeerState::Unknown,
            up_at:        None,
            down_at:      None,
            capabilities: Vec::new(),
            route_counts: HashMap::new(),
            flap_count:   0,
        }
    }

    pub fn on_up(&mut self, local_as: u32, hold_time: u16, caps: Vec<BgpCapability>, at: DateTime<Utc>) {
        if self.state == PeerState::Up { self.flap_count += 1; }
        self.state    = PeerState::Up;
        self.up_at    = Some(at);
        self.local_as  = local_as;
        self.hold_time = hold_time;
        self.capabilities = caps;
    }

    pub fn on_down(&mut self, at: DateTime<Utc>) {
        self.state   = PeerState::Down;
        self.down_at = Some(at);
        self.route_counts.clear();
    }

    pub fn uptime_secs(&self) -> Option<i64> {
        self.up_at.map(|t| (Utc::now() - t).num_seconds())
    }
}

/// A BMP speaker (router) that has connected to us
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmpSession {
    pub speaker_addr: IpAddr,
    pub sys_name:     Option<String>,
    pub sys_descr:    Option<String>,
    pub connected_at: DateTime<Utc>,
    pub peers:        HashMap<IpAddr, PeerSession>,
}

impl BmpSession {
    pub fn new(speaker_addr: IpAddr, at: DateTime<Utc>) -> Self {
        Self {
            speaker_addr,
            sys_name:     None,
            sys_descr:    None,
            connected_at: at,
            peers:        HashMap::new(),
        }
    }

    pub fn peer_count(&self) -> usize { self.peers.len() }

    pub fn up_peer_count(&self) -> usize {
        self.peers.values().filter(|p| p.state == PeerState::Up).count()
    }

    pub fn total_routes(&self) -> usize {
        self.peers.values().flat_map(|p| p.route_counts.values()).sum()
    }
}
