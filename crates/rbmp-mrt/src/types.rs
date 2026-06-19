/// MRT message type codes (RFC 6396 §4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MrtType {
    OspfV2           = 11,
    TableDump        = 12,
    TableDumpV2      = 13,
    Bgp4Mp           = 16,
    Bgp4MpEt         = 17, // extended timestamp
    Isis             = 32,
    OspfV3           = 48,
}

/// TABLE_DUMP_V2 sub-type codes (RFC 6396 §4.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum TableDumpV2Subtype {
    PeerIndexTable       = 1,
    RibIpv4Unicast       = 2,
    RibIpv4Multicast     = 3,
    RibIpv6Unicast       = 4,
    RibIpv6Multicast     = 5,
    RibGeneric           = 6,
    GeoPeerTable         = 7,
    RibIpv4UnicastAddPath = 8,
    RibIpv6UnicastAddPath = 9,
}

/// BGP4MP sub-type codes (RFC 6396 §4.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Bgp4MpSubtype {
    StateChange    = 0,
    Message        = 1,
    MessageAs4     = 4,
    StateChangeAs4 = 5,
    MessageLocal   = 6,
    MessageAs4Local = 7,
    MessageAddpath  = 8,
    MessageAs4Addpath = 9,
}

/// BGP FSM states (for BGP4MP_STATE_CHANGE)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BgpState {
    Idle        = 1,
    Connect     = 2,
    Active      = 3,
    OpenSent    = 4,
    OpenConfirm = 5,
    Established = 6,
}
