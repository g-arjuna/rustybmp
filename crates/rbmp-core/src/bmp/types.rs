use std::net::{IpAddr, Ipv4Addr};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::bgp::types::{BgpCapability, BgpUpdate};

// ─── BMP message type (RFC 7854 §4) ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum BmpMsgType {
    RouteMonitoring  = 0,
    StatsReport      = 1,
    PeerDown         = 2,
    PeerUp           = 3,
    Initiation       = 4,
    Termination      = 5,
    RouteMirroring   = 6,
}

impl TryFrom<u8> for BmpMsgType {
    type Error = crate::Error;
    fn try_from(v: u8) -> crate::Result<Self> {
        match v {
            0 => Ok(Self::RouteMonitoring),
            1 => Ok(Self::StatsReport),
            2 => Ok(Self::PeerDown),
            3 => Ok(Self::PeerUp),
            4 => Ok(Self::Initiation),
            5 => Ok(Self::Termination),
            6 => Ok(Self::RouteMirroring),
            _ => Err(crate::Error::InvalidMessageType(v)),
        }
    }
}

// ─── Peer type (RFC 7854 §4.2) ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PeerType {
    GlobalInstance = 0,  // Default — global BGP instance
    RdInstance     = 1,  // VRF-specific (RD identifies the instance)
    LocalInstance  = 2,  // Local (collector-local) BGP instance
    LocRib         = 3,  // Loc-RIB (RFC 9069)
}

impl TryFrom<u8> for PeerType {
    type Error = crate::Error;
    fn try_from(v: u8) -> crate::Result<Self> {
        match v {
            0 => Ok(Self::GlobalInstance),
            1 => Ok(Self::RdInstance),
            2 => Ok(Self::LocalInstance),
            3 => Ok(Self::LocRib),
            _ => Err(crate::Error::BmpParse(format!("unknown peer type {v}"))),
        }
    }
}

// ─── Peer flags (RFC 7854 §4.2 bits) ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PeerFlags(pub u8);

impl PeerFlags {
    pub fn ipv6(self)          -> bool { self.0 & 0x80 != 0 }
    pub fn post_policy(self)   -> bool { self.0 & 0x40 != 0 }
    pub fn as2(self)           -> bool { self.0 & 0x20 != 0 }
    pub fn adj_rib_out(self)   -> bool { self.0 & 0x10 != 0 }  // RFC 8671
    pub fn filtered(self)      -> bool { self.0 & 0x08 != 0 }  // RFC 9069 Loc-RIB filtered flag
}

// ─── RIB type derived from peer_type + peer_flags ────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RibType {
    AdjRibInPrePolicy,
    AdjRibInPostPolicy,
    AdjRibOutPrePolicy,
    AdjRibOutPostPolicy,
    LocRib,
    LocRibFiltered,
}

impl RibType {
    pub fn from_peer(peer_type: PeerType, flags: PeerFlags) -> Self {
        match peer_type {
            PeerType::LocRib => {
                if flags.filtered() { Self::LocRibFiltered } else { Self::LocRib }
            }
            PeerType::LocalInstance => {
                if flags.post_policy() { Self::AdjRibOutPostPolicy } else { Self::AdjRibOutPrePolicy }
            }
            _ => {
                if flags.adj_rib_out() {
                    if flags.post_policy() { Self::AdjRibOutPostPolicy } else { Self::AdjRibOutPrePolicy }
                } else {
                    if flags.post_policy() { Self::AdjRibInPostPolicy } else { Self::AdjRibInPrePolicy }
                }
            }
        }
    }
}

// ─── Common peer header (RFC 7854 §4.2, 42 bytes) ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerHeader {
    pub peer_type:          PeerType,
    pub peer_flags:         PeerFlags,
    /// 8-byte Route Distinguisher (zero for global instance)
    pub peer_distinguisher: [u8; 8],
    /// IPv4 or IPv6 peer address
    pub peer_address:       IpAddr,
    pub peer_as:            u32,
    pub peer_bgp_id:        Ipv4Addr,
    pub timestamp:          DateTime<Utc>,
    pub rib_type:           RibType,
}

// ─── BMP initiation TLV (RFC 7854 §4.4) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiationTlv {
    pub tlv_type: u16,
    pub value:    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InitiationInfo {
    SysDescr(String),
    SysName(String),
    AdminLabel(String),
    Unknown { tlv_type: u16, hex: String },
}

// ─── BMP termination TLV (RFC 7854 §4.5) ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum TermReason {
    AdministrativelyClosed = 0,
    Unspecified            = 1,
    OutOfResources         = 2,
    RedundantConnection    = 3,
    PermanentlyAdminClosed = 4,
}

// ─── Peer Down reason (RFC 7854 §4.9) ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerDownReason {
    LocalSystemClosed { notification: Option<Vec<u8>> },  // 1
    LocalSystemClosed2,                                    // 2 (FSM event)
    RemoteSystemClosed { notification: Option<Vec<u8>> }, // 3
    RemoteSystemClosed2,                                   // 4 (BMP message data)
    PeerDeConfigured,                                      // 5
    VrfDown,                                               // 6
    Unknown(u8),
}

// ─── BGP OPEN info (captured at Peer Up) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpOpenInfo {
    pub version:      u8,
    pub asn:          u32,
    pub hold_time:    u16,
    pub bgp_id:       Ipv4Addr,
    pub capabilities: Vec<BgpCapability>,
}

// ─── Peer Up message (RFC 7854 §4.10) ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerUpMessage {
    pub peer_header:  PeerHeader,
    pub local_addr:   IpAddr,
    pub local_port:   u16,
    pub remote_port:  u16,
    pub sent_open:    BgpOpenInfo,
    pub recv_open:    BgpOpenInfo,
}

// ─── Statistics (RFC 7854 §4.8) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatEntry {
    pub stat_type: u16,
    pub name:      String,
    pub value:     u64,
}

/// All RFC 7854 §4.8 statistics counter types
pub fn stat_name(t: u16) -> &'static str {
    match t {
        0  => "prefixes-rejected-by-inbound-policy",
        1  => "duplicate-prefix-advertisements",
        2  => "duplicate-withdrawals",
        3  => "cluster-list-loop-detections",
        4  => "as-path-loop-detections",
        5  => "originator-id-loop-detections",
        6  => "implicit-withdraw-count",
        7  => "explicit-withdraw-count",
        8  => "update-treatments-as-withdraw",
        9  => "prefixes-treated-as-withdraw",
        10 => "duplicate-update-messages",
        11 => "adj-rib-in-routes",
        12 => "loc-rib-routes",
        13 => "per-afi-safi-adj-rib-in-routes",
        14 => "per-afi-safi-loc-rib-routes",
        15 => "updates-subjected-to-treat-as-withdraw",
        16 => "prefixes-subjected-to-treat-as-withdraw",
        17 => "duplicate-update-messages-rcvd",
        18 => "adj-rib-out-pre-policy-routes",
        19 => "adj-rib-out-post-policy-routes",
        _  => "unknown",
    }
}

// ─── Top-level BMP message envelope ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BmpMessage {
    /// Unique ID for this message (assigned by receiver)
    pub id:           uuid::Uuid,
    /// Wall-clock when received (UTC)
    pub received_at:  DateTime<Utc>,
    /// IP address of the BMP speaker (router) that sent this
    pub speaker_addr: IpAddr,
    pub payload:      BmpPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BmpPayload {
    Initiation {
        sys_name:  Option<String>,
        sys_descr: Option<String>,
        labels:    Vec<String>,
    },
    Termination {
        reason_code: u16,
        reason_text: Option<String>,
    },
    PeerUp(PeerUpMessage),
    PeerDown {
        peer_header: PeerHeader,
        reason:      PeerDownReason,
    },
    RouteMonitoring {
        peer_header: PeerHeader,
        update:      BgpUpdate,
    },
    StatsReport {
        peer_header: PeerHeader,
        stats:       Vec<StatEntry>,
    },
    RouteMirroring {
        peer_header: PeerHeader,
        /// Raw mirrored BGP PDU bytes
        pdu:         Vec<u8>,
    },
}

impl BmpMessage {
    pub fn msg_type(&self) -> &'static str {
        match &self.payload {
            BmpPayload::Initiation { .. }  => "initiation",
            BmpPayload::Termination { .. } => "termination",
            BmpPayload::PeerUp(_)          => "peer_up",
            BmpPayload::PeerDown { .. }    => "peer_down",
            BmpPayload::RouteMonitoring { .. } => "route_monitoring",
            BmpPayload::StatsReport { .. } => "stats_report",
            BmpPayload::RouteMirroring { .. } => "route_mirroring",
        }
    }
}
