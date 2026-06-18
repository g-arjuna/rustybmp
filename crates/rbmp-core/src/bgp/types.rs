use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::fmt;
use ipnet::{Ipv4Net, Ipv6Net};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

// ─── AFI / SAFI ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Afi {
    Ipv4,
    Ipv6,
    L2Vpn,
    BgpLs,
    Unknown(u16),
}

impl From<u16> for Afi {
    fn from(v: u16) -> Self {
        match v {
            1     => Self::Ipv4,
            2     => Self::Ipv6,
            25    => Self::L2Vpn,
            16388 => Self::BgpLs,
            _     => Self::Unknown(v),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Safi {
    Unicast,
    Multicast,
    LabeledUnicast,
    MplsVpn,
    Evpn,
    Flowspec,
    FlowspecVpn,
    Unknown(u8),
}

impl From<u8> for Safi {
    fn from(v: u8) -> Self {
        match v {
            1   => Self::Unicast,
            2   => Self::Multicast,
            4   => Self::LabeledUnicast,
            70  => Self::Evpn,
            128 => Self::MplsVpn,
            133 => Self::Flowspec,
            134 => Self::FlowspecVpn,
            _   => Self::Unknown(v),
        }
    }
}

/// Combined AFI/SAFI address family identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AfiSafi {
    pub afi:  Afi,
    pub safi: Safi,
}

impl AfiSafi {
    pub fn new(afi: u16, safi: u8) -> Self {
        Self { afi: Afi::from(afi), safi: Safi::from(safi) }
    }
    pub fn ipv4_unicast()  -> Self { Self::new(1, 1) }
    pub fn ipv6_unicast()  -> Self { Self::new(2, 1) }
    pub fn ipv4_vpn()      -> Self { Self::new(1, 128) }
    pub fn ipv6_vpn()      -> Self { Self::new(2, 128) }
    pub fn evpn()          -> Self { Self::new(25, 70) }
    pub fn ipv4_labeled()  -> Self { Self::new(1, 4) }
    pub fn ipv6_labeled()  -> Self { Self::new(2, 4) }
}

impl fmt::Display for AfiSafi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match (self.afi, self.safi) {
            (Afi::Ipv4, Safi::Unicast)        => "ipv4-unicast",
            (Afi::Ipv6, Safi::Unicast)        => "ipv6-unicast",
            (Afi::Ipv4, Safi::Multicast)      => "ipv4-multicast",
            (Afi::Ipv6, Safi::Multicast)      => "ipv6-multicast",
            (Afi::Ipv4, Safi::LabeledUnicast) => "ipv4-labeled-unicast",
            (Afi::Ipv6, Safi::LabeledUnicast) => "ipv6-labeled-unicast",
            (Afi::Ipv4, Safi::MplsVpn)        => "ipv4-vpn",
            (Afi::Ipv6, Safi::MplsVpn)        => "ipv6-vpn",
            (Afi::L2Vpn, Safi::Evpn)          => "l2vpn-evpn",
            (Afi::Ipv4, Safi::Flowspec)       => "ipv4-flowspec",
            (Afi::Ipv6, Safi::Flowspec)       => "ipv6-flowspec",
            _ => return write!(f, "afi{}-safi{}", self.afi.as_u16(), self.safi.as_u8()),
        };
        f.write_str(s)
    }
}

impl Afi {
    pub fn as_u16(self) -> u16 {
        match self {
            Self::Ipv4       => 1,
            Self::Ipv6       => 2,
            Self::L2Vpn      => 25,
            Self::BgpLs      => 16388,
            Self::Unknown(v) => v,
        }
    }
}

impl Safi {
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Unicast        => 1,
            Self::Multicast      => 2,
            Self::LabeledUnicast => 4,
            Self::Evpn           => 70,
            Self::MplsVpn        => 128,
            Self::Flowspec       => 133,
            Self::FlowspecVpn    => 134,
            Self::Unknown(v)     => v,
        }
    }
}

// ─── Prefix types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Prefix {
    V4(Ipv4Net),
    V6(Ipv6Net),
    /// MPLS labeled (RFC 3107): prefix + MPLS label stack
    Labeled { prefix: Box<Prefix>, labels: SmallVec<[u32; 2]> },
    /// VPN: Route Distinguisher + labeled prefix (RFC 4364)
    Vpn { rd: RouteDistinguisher, prefix: Box<Prefix>, labels: SmallVec<[u32; 2]> },
}

impl Prefix {
    pub fn addr_family(&self) -> Afi {
        match self {
            Self::V4(_)               => Afi::Ipv4,
            Self::V6(_)               => Afi::Ipv6,
            Self::Labeled { prefix, .. } | Self::Vpn { prefix, .. } => prefix.addr_family(),
        }
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V4(n) => write!(f, "{}", n),
            Self::V6(n) => write!(f, "{}", n),
            Self::Labeled { prefix, labels } => write!(f, "{} label={}", prefix, labels[0]),
            Self::Vpn { rd, prefix, labels }  => write!(f, "{}:{} label={}", rd, prefix, labels[0]),
        }
    }
}

// ─── Route Distinguisher (RFC 4364) ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteDistinguisher(pub [u8; 8]);

impl RouteDistinguisher {
    pub fn zero() -> Self { Self([0u8; 8]) }
    pub fn is_zero(&self) -> bool { self.0 == [0u8; 8] }

    pub fn rd_type(&self) -> u16 {
        u16::from_be_bytes([self.0[0], self.0[1]])
    }
}

impl fmt::Display for RouteDistinguisher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rd_type() {
            0 => {
                let admin = u16::from_be_bytes([self.0[2], self.0[3]]);
                let assigned = u32::from_be_bytes([self.0[4], self.0[5], self.0[6], self.0[7]]);
                write!(f, "{}:{}", admin, assigned)
            }
            1 => {
                let admin = Ipv4Addr::from([self.0[2], self.0[3], self.0[4], self.0[5]]);
                let assigned = u16::from_be_bytes([self.0[6], self.0[7]]);
                write!(f, "{}:{}", admin, assigned)
            }
            2 => {
                let admin = u32::from_be_bytes([self.0[2], self.0[3], self.0[4], self.0[5]]);
                let assigned = u16::from_be_bytes([self.0[6], self.0[7]]);
                write!(f, "{}:{}", admin, assigned)
            }
            _ => write!(f, "0x{}", hex::encode(self.0)),
        }
    }
}

// ─── AS Path ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsPathSegment {
    /// Type 1: unordered set
    Set(Vec<u32>),
    /// Type 2: ordered sequence
    Sequence(Vec<u32>),
    /// Type 3: confederation sequence
    ConfedSequence(Vec<u32>),
    /// Type 4: confederation set
    ConfedSet(Vec<u32>),
}

impl AsPathSegment {
    pub fn asns(&self) -> &[u32] {
        match self {
            Self::Set(v) | Self::Sequence(v) | Self::ConfedSequence(v) | Self::ConfedSet(v) => v.as_slice(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsPath(pub Vec<AsPathSegment>);

impl AsPath {
    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    /// Total AS hop count (counting only SEQUENCE segments)
    pub fn hop_count(&self) -> usize {
        self.0.iter().map(|seg| match seg {
            AsPathSegment::Sequence(v) => v.len(),
            _ => 0,
        }).sum()
    }

    /// Left-most (origin) ASN from the first SEQUENCE
    pub fn origin_asn(&self) -> Option<u32> {
        self.0.iter().find_map(|seg| match seg {
            AsPathSegment::Sequence(v) => v.last().copied(),
            _ => None,
        })
    }

    /// Originating ASN (right-most AS in the full AS_PATH)
    pub fn first_asn(&self) -> Option<u32> {
        self.0.iter().find_map(|seg| match seg {
            AsPathSegment::Sequence(v) => v.first().copied(),
            _ => None,
        })
    }

    /// Detect prepending: same ASN repeated consecutively
    pub fn has_prepending(&self) -> bool {
        for seg in &self.0 {
            if let AsPathSegment::Sequence(v) = seg {
                let mut prev = None;
                for &asn in v {
                    if Some(asn) == prev { return true; }
                    prev = Some(asn);
                }
            }
        }
        false
    }

    /// Check for AS path loop (ASN appearing more than once across all segments)
    pub fn has_loop(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        for seg in &self.0 {
            for &asn in seg.asns() {
                if !seen.insert(asn) { return true; }
            }
        }
        false
    }
}

impl fmt::Display for AsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(|seg| match seg {
            AsPathSegment::Sequence(v) => v.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" "),
            AsPathSegment::Set(v) => format!("{{{}}}", v.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(",")),
            AsPathSegment::ConfedSequence(v) => format!("({})", v.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ")),
            AsPathSegment::ConfedSet(v) => format!("[{}]", v.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(",")),
        }).collect();
        write!(f, "{}", parts.join(" "))
    }
}

// ─── Communities ──────────────────────────────────────────────────────────────

/// Standard BGP community (RFC 1997): encoded as 32 bits (ASN:value)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StandardCommunity(pub u32);

impl StandardCommunity {
    pub fn new(asn: u16, value: u16) -> Self {
        Self(((asn as u32) << 16) | value as u32)
    }
    pub fn asn(self) -> u16   { (self.0 >> 16) as u16 }
    pub fn value(self) -> u16 { (self.0 & 0xFFFF) as u16 }

    // Well-known communities (RFC 1997 + RFC 8642)
    pub fn no_export()              -> Self { Self(0xFFFFFF01) }
    pub fn no_advertise()           -> Self { Self(0xFFFFFF02) }
    pub fn no_export_subconfed()    -> Self { Self(0xFFFFFF03) }
    pub fn blackhole()              -> Self { Self(0xFFFF029A) }
    pub fn graceful_shutdown()      -> Self { Self(0xFFFF0000) }
    pub fn accept_own()             -> Self { Self(0xFFFF0001) }

    pub fn is_well_known(self) -> bool { self.0 >= 0xFFFF0000 }

    pub fn name(self) -> Option<&'static str> {
        match self.0 {
            0xFFFFFF01 => Some("NO_EXPORT"),
            0xFFFFFF02 => Some("NO_ADVERTISE"),
            0xFFFFFF03 => Some("NO_EXPORT_SUBCONFED"),
            0xFFFF029A => Some("BLACKHOLE"),
            0xFFFF0000 => Some("GRACEFUL_SHUTDOWN"),
            0xFFFF0001 => Some("ACCEPT_OWN"),
            _          => None,
        }
    }
}

impl fmt::Display for StandardCommunity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = self.name() {
            write!(f, "{}", name)
        } else {
            write!(f, "{}:{}", self.asn(), self.value())
        }
    }
}

/// Extended community (RFC 4360): 8 bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExtendedCommunity {
    pub type_high: u8,
    pub type_low:  u8,
    pub value:     [u8; 6],
}

impl ExtendedCommunity {
    pub fn from_bytes(b: &[u8; 8]) -> Self {
        let mut value = [0u8; 6];
        value.copy_from_slice(&b[2..]);
        Self { type_high: b[0], type_low: b[1], value }
    }
    pub fn is_transitive(&self) -> bool { self.type_high & 0x40 == 0 }
}

impl fmt::Display for ExtendedCommunity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.type_high & 0x3F {
            0x00 | 0x02 => {
                // AS-specific
                let admin = u16::from_be_bytes([self.value[0], self.value[1]]);
                let assigned = u32::from_be_bytes([self.value[2], self.value[3], self.value[4], self.value[5]]);
                write!(f, "rt:{}:{}", admin, assigned)
            }
            0x01 => {
                // IPv4-specific
                let admin = Ipv4Addr::from([self.value[0], self.value[1], self.value[2], self.value[3]]);
                let assigned = u16::from_be_bytes([self.value[4], self.value[5]]);
                write!(f, "rt:{}:{}", admin, assigned)
            }
            _ => write!(f, "ext:0x{:02x}{:02x}:{}", self.type_high, self.type_low, hex::encode(self.value)),
        }
    }
}

/// Large community (RFC 8092): 12 bytes (3×u32)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LargeCommunity {
    pub global_admin:  u32,
    pub local_data_1:  u32,
    pub local_data_2:  u32,
}

impl fmt::Display for LargeCommunity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.global_admin, self.local_data_1, self.local_data_2)
    }
}

// ─── BGP ORIGIN ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Igp       = 0,
    Egp       = 1,
    Incomplete = 2,
}

impl TryFrom<u8> for Origin {
    type Error = crate::Error;
    fn try_from(v: u8) -> crate::Result<Self> {
        match v {
            0 => Ok(Self::Igp),
            1 => Ok(Self::Egp),
            2 => Ok(Self::Incomplete),
            _ => Err(crate::Error::BgpParse(format!("invalid ORIGIN value {v}"))),
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Igp       => write!(f, "IGP"),
            Self::Egp       => write!(f, "EGP"),
            Self::Incomplete => write!(f, "INCOMPLETE"),
        }
    }
}

// ─── BGP Capabilities (RFC 5492) ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BgpCapability {
    Multiprotocol(AfiSafi),
    RouteRefresh,
    ExtendedNextHop { afi: Afi, safi: Safi, next_hop_afi: Afi },
    ExtendedMessage,
    GracefulRestart { restart_time: u16, afi_safis: Vec<(AfiSafi, u8)> },
    FourByteAsn(u32),
    AddPath(Vec<(AfiSafi, u8)>),  // u8 = send(1), recv(2), both(3)
    EnhancedRouteRefresh,
    LongLivedGracefulRestart,
    Fqdn { hostname: String, domain: String },
    Unknown { code: u8, data: Vec<u8> },
}

impl BgpCapability {
    pub fn code(&self) -> u8 {
        match self {
            Self::Multiprotocol(_)      => 1,
            Self::RouteRefresh          => 2,
            Self::ExtendedNextHop { .. }=> 5,
            Self::ExtendedMessage       => 6,
            Self::GracefulRestart { .. }=> 64,
            Self::FourByteAsn(_)        => 65,
            Self::AddPath(_)            => 69,
            Self::EnhancedRouteRefresh  => 70,
            Self::LongLivedGracefulRestart => 71,
            Self::Fqdn { .. }           => 73,
            Self::Unknown { code, .. }  => *code,
        }
    }
}

// ─── Path Attributes ──────────────────────────────────────────────────────────

/// A raw (unparsed) path attribute — preserved for unknown types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawAttribute {
    pub flags:     u8,
    pub type_code: u8,
    pub value:     Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathAttributes {
    pub origin:              Option<Origin>,
    pub as_path:             Option<AsPath>,
    pub next_hop:            Option<IpAddr>,
    pub multi_exit_disc:     Option<u32>,
    pub local_pref:          Option<u32>,
    pub atomic_aggregate:    bool,
    pub aggregator:          Option<(u32, Ipv4Addr)>,  // 4-byte ASN + BGP ID
    pub communities:         Vec<StandardCommunity>,
    pub extended_communities: Vec<ExtendedCommunity>,
    pub large_communities:   Vec<LargeCommunity>,
    pub originator_id:       Option<Ipv4Addr>,
    pub cluster_list:        Vec<Ipv4Addr>,
    pub mp_reach:            Option<MpReachNlri>,
    pub mp_unreach:          Option<MpUnreachNlri>,
    pub as4_path:            Option<AsPath>,
    pub as4_aggregator:      Option<(u32, Ipv4Addr)>,
    pub unknown:             Vec<RawAttribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpReachNlri {
    pub afi_safi:  AfiSafi,
    pub next_hops: Vec<IpAddr>,
    pub prefixes:  Vec<Prefix>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpUnreachNlri {
    pub afi_safi: AfiSafi,
    pub prefixes: Vec<Prefix>,
}

// ─── BGP UPDATE ───────────────────────────────────────────────────────────────

/// Parsed BGP UPDATE message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpUpdate {
    /// Prefixes being withdrawn (IPv4 unicast NLRI, or via MP_UNREACH)
    pub withdrawn: Vec<Prefix>,
    pub attributes: PathAttributes,
    /// Prefixes being announced (IPv4 unicast NLRI, or via MP_REACH)
    pub announced: Vec<Prefix>,
}

impl BgpUpdate {
    pub fn is_eor(&self) -> bool {
        // End-of-RIB marker: empty UPDATE or MP_UNREACH with no prefixes (RFC 4724)
        self.withdrawn.is_empty()
            && self.announced.is_empty()
            && self.attributes.mp_reach.is_none()
            && self.attributes.mp_unreach.as_ref().map_or(true, |u| u.prefixes.is_empty())
    }

    pub fn all_announced(&self) -> Vec<&Prefix> {
        let mut out: Vec<&Prefix> = self.announced.iter().collect();
        if let Some(mp) = &self.attributes.mp_reach {
            out.extend(mp.prefixes.iter());
        }
        out
    }

    pub fn all_withdrawn(&self) -> Vec<&Prefix> {
        let mut out: Vec<&Prefix> = self.withdrawn.iter().collect();
        if let Some(mp) = &self.attributes.mp_unreach {
            out.extend(mp.prefixes.iter());
        }
        out
    }
}

// ─── Need hex for Display impls ────────────────────────────────────────────────
mod hex {
    pub fn encode(b: impl AsRef<[u8]>) -> String {
        b.as_ref().iter().map(|x| format!("{:02x}", x)).collect()
    }
}
