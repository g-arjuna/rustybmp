use std::net::IpAddr;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use rbmp_core::bgp::types::{PathAttributes, Prefix};
use rbmp_core::bmp::types::{PeerHeader, RibType, StatEntry};

/// Every state change emitted by the RIB engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RibEvent {
    pub id:          uuid::Uuid,
    pub occurred_at: DateTime<Utc>,
    pub speaker:     IpAddr,
    pub payload:     RibEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RibEventPayload {
    /// A BMP speaker connected
    SpeakerUp { sys_name: Option<String>, sys_descr: Option<String> },
    /// A BMP speaker disconnected
    SpeakerDown { reason: String },
    /// A BGP peer session came up
    PeerUp {
        peer_header:  PeerHeader,
        local_asn:    u32,
        remote_asn:   u32,
        hold_time:    u16,
        capabilities: Vec<String>,
    },
    /// A BGP peer session went down
    PeerDown {
        peer_header: PeerHeader,
        reason:      String,
    },
    /// Route change (announce or withdraw)
    RouteChange(RouteChange),
    /// Statistics snapshot (RFC 7854 + RFC 9972)
    Stats {
        peer_header: PeerHeader,
        counters:    Vec<StatEntry>,
    },
    /// End-of-RIB marker received
    EndOfRib {
        peer_header: PeerHeader,
        afi_safi:    String,
    },
}

/// A single route change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteChange {
    pub action:      RouteAction,
    pub peer_header: PeerHeader,
    pub rib_type:    RibType,
    pub prefix:      Prefix,
    /// Attributes present for Announce, None for Withdraw
    pub attributes:  Option<PathAttributes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteAction {
    Announce,
    Withdraw,
}
