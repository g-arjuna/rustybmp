//! MRT writer — serialises BMP/RIB state to RFC 6396 binary MRT format.
//!
//! Supported record types written:
//!   BGP4MP_MESSAGE_AS4  (subtype 4) — raw BGP UPDATE bytes
//!   BGP4MP_STATE_CHANGE_AS4 (subtype 5) — peer FSM transitions
//!   TABLE_DUMP_V2 PEER_INDEX_TABLE (subtype 1) — peer index
//!   TABLE_DUMP_V2 RIB_IPV4_UNICAST (subtype 2)
//!   TABLE_DUMP_V2 RIB_IPV6_UNICAST (subtype 4)

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use bytes::{BufMut, BytesMut};
use chrono::{DateTime, Utc};

use rbmp_rib::event::{RibEvent, RibEventPayload};
use crate::types::*;
use crate::error::MrtError;

pub use crate::types::BgpState;

type Result<T> = std::result::Result<T, MrtError>;

// ─── MRT common header (12 bytes) ────────────────────────────────────────────

fn write_mrt_header(buf: &mut BytesMut, ts: u32, mrt_type: u16, subtype: u16, length: u32) {
    buf.put_u32(ts);
    buf.put_u16(mrt_type);
    buf.put_u16(subtype);
    buf.put_u32(length);
}

fn epoch_secs(dt: &DateTime<Utc>) -> u32 {
    dt.timestamp().max(0) as u32
}

// ─── IP address helpers ───────────────────────────────────────────────────────

fn write_ipv4(buf: &mut BytesMut, addr: Ipv4Addr) {
    buf.put_slice(&addr.octets());
}

fn write_ipv6(buf: &mut BytesMut, addr: Ipv6Addr) {
    buf.put_slice(&addr.octets());
}

/// Write a BGP NLRI-encoded prefix (prefix-len byte + ceiling(len/8) octets).
fn write_prefix_nlri(buf: &mut BytesMut, prefix: &rbmp_core::bgp::Prefix) -> Result<()> {
    match prefix {
        rbmp_core::bgp::Prefix::V4(net) => {
            let len = net.prefix_len();
            let addr_bytes = net.addr().octets();
            let byte_len = (len as usize + 7) / 8;
            buf.put_u8(len);
            buf.put_slice(&addr_bytes[..byte_len]);
        }
        rbmp_core::bgp::Prefix::V6(net) => {
            let len = net.prefix_len();
            let addr_bytes = net.addr().octets();
            let byte_len = (len as usize + 7) / 8;
            buf.put_u8(len);
            buf.put_slice(&addr_bytes[..byte_len]);
        }
        rbmp_core::bgp::Prefix::Labeled { prefix, .. }
        | rbmp_core::bgp::Prefix::Vpn { prefix, .. } => {
            return write_prefix_nlri(buf, prefix);
        }
    }
    Ok(())
}

// ─── BGP4MP_MESSAGE_AS4 ───────────────────────────────────────────────────────

/// Write a BGP4MP_MESSAGE_AS4 record for a raw BGP UPDATE.
///
/// `bgp_bytes` should be the raw BGP PDU (including the 19-byte BGP header).
pub fn write_bgp4mp_message(
    out:          &mut impl Write,
    ts:           &DateTime<Utc>,
    peer_as:      u32,
    local_as:     u32,
    interface_idx: u16,
    peer_addr:    IpAddr,
    local_addr:   IpAddr,
    bgp_bytes:    &[u8],
) -> Result<()> {
    let is_v6 = peer_addr.is_ipv6();
    let addr_len = if is_v6 { 16usize } else { 4 };

    let body_len = 4 + 4 + 2 + 2 + addr_len + addr_len + bgp_bytes.len();
    let mut buf = BytesMut::with_capacity(12 + body_len);

    write_mrt_header(&mut buf,
        epoch_secs(ts),
        MrtType::Bgp4Mp as u16,
        Bgp4MpSubtype::MessageAs4 as u16,
        body_len as u32,
    );

    buf.put_u32(peer_as);
    buf.put_u32(local_as);
    buf.put_u16(interface_idx);
    buf.put_u16(if is_v6 { 2 } else { 1 }); // AFI

    match peer_addr {
        IpAddr::V4(a) => write_ipv4(&mut buf, a),
        IpAddr::V6(a) => write_ipv6(&mut buf, a),
    }
    match local_addr {
        IpAddr::V4(a) => write_ipv4(&mut buf, a),
        IpAddr::V6(a) => write_ipv6(&mut buf, a),
    }

    buf.put_slice(bgp_bytes);
    out.write_all(&buf)?;
    Ok(())
}

// ─── BGP4MP_STATE_CHANGE_AS4 ─────────────────────────────────────────────────

pub fn write_bgp4mp_state_change(
    out:       &mut impl Write,
    ts:        &DateTime<Utc>,
    peer_as:   u32,
    local_as:  u32,
    peer_addr: IpAddr,
    local_addr: IpAddr,
    old_state: BgpState,
    new_state: BgpState,
) -> Result<()> {
    let is_v6 = peer_addr.is_ipv6();
    let addr_len = if is_v6 { 16usize } else { 4 };
    let body_len = 4 + 4 + 2 + 2 + addr_len + addr_len + 4;

    let mut buf = BytesMut::with_capacity(12 + body_len);
    write_mrt_header(&mut buf,
        epoch_secs(ts),
        MrtType::Bgp4Mp as u16,
        Bgp4MpSubtype::StateChangeAs4 as u16,
        body_len as u32,
    );

    buf.put_u32(peer_as);
    buf.put_u32(local_as);
    buf.put_u16(0); // interface index
    buf.put_u16(if is_v6 { 2 } else { 1 });

    match peer_addr { IpAddr::V4(a) => write_ipv4(&mut buf, a), IpAddr::V6(a) => write_ipv6(&mut buf, a) }
    match local_addr { IpAddr::V4(a) => write_ipv4(&mut buf, a), IpAddr::V6(a) => write_ipv6(&mut buf, a) }

    buf.put_u16(old_state as u16);
    buf.put_u16(new_state as u16);

    out.write_all(&buf)?;
    Ok(())
}

// ─── TABLE_DUMP_V2 ───────────────────────────────────────────────────────────

/// A single peer entry for the PEER_INDEX_TABLE.
#[derive(Debug, Clone)]
pub struct MrtPeerEntry {
    pub bgp_id:  Ipv4Addr,
    pub addr:    IpAddr,
    pub peer_as: u32,
}

/// Write a TABLE_DUMP_V2 PEER_INDEX_TABLE record.
pub fn write_peer_index_table(
    out:          &mut impl Write,
    ts:           &DateTime<Utc>,
    collector_id: Ipv4Addr,
    view_name:    &str,
    peers:        &[MrtPeerEntry],
) -> Result<()> {
    let mut body = BytesMut::new();
    body.put_slice(&collector_id.octets());
    let vn = view_name.as_bytes();
    body.put_u16(vn.len() as u16);
    body.put_slice(vn);
    body.put_u16(peers.len() as u16);

    for p in peers {
        let is_v6  = p.addr.is_ipv6();
        let is_as4 = true; // we always use 4-byte AS
        let peer_type: u8 = (if is_v6 { 1 } else { 0 }) | (if is_as4 { 2 } else { 0 });
        body.put_u8(peer_type);
        body.put_slice(&p.bgp_id.octets());
        match p.addr {
            IpAddr::V4(a) => write_ipv4(&mut body, a),
            IpAddr::V6(a) => write_ipv6(&mut body, a),
        }
        body.put_u32(p.peer_as);
    }

    let mut hdr = BytesMut::with_capacity(12);
    write_mrt_header(&mut hdr,
        epoch_secs(ts),
        MrtType::TableDumpV2 as u16,
        TableDumpV2Subtype::PeerIndexTable as u16,
        body.len() as u32,
    );
    out.write_all(&hdr)?;
    out.write_all(&body)?;
    Ok(())
}

/// A single RIB entry for TABLE_DUMP_V2 RIB_IPV{4,6}_UNICAST.
#[derive(Debug, Clone)]
pub struct MrtRibEntry {
    pub peer_index:  u16,
    pub originated:  DateTime<Utc>,
    pub path_attrs:  Vec<u8>,  // raw serialised BGP path attributes
}

/// Write a TABLE_DUMP_V2 RIB record (IPv4 or IPv6 unicast).
pub fn write_rib_entry(
    out:        &mut impl Write,
    ts:         &DateTime<Utc>,
    seq:        u32,
    prefix:     &rbmp_core::bgp::Prefix,
    entries:    &[MrtRibEntry],
) -> Result<()> {
    let is_v6 = matches!(prefix, rbmp_core::bgp::Prefix::V6(_));

    let mut body = BytesMut::new();
    body.put_u32(seq);

    // Write prefix in NLRI wire format
    write_prefix_nlri(&mut body, prefix)?;

    body.put_u16(entries.len() as u16);
    for e in entries {
        body.put_u16(e.peer_index);
        body.put_u32(epoch_secs(&e.originated));
        body.put_u16(e.path_attrs.len() as u16);
        body.put_slice(&e.path_attrs);
    }

    let subtype = if is_v6 {
        TableDumpV2Subtype::RibIpv6Unicast as u16
    } else {
        TableDumpV2Subtype::RibIpv4Unicast as u16
    };

    let mut hdr = BytesMut::with_capacity(12);
    write_mrt_header(&mut hdr, epoch_secs(ts), MrtType::TableDumpV2 as u16, subtype, body.len() as u32);
    out.write_all(&hdr)?;
    out.write_all(&body)?;
    Ok(())
}

// ─── RibEvent → BGP4MP helper ─────────────────────────────────────────────────

/// Convert a `PeerDown` RibEvent to a BGP4MP_STATE_CHANGE_AS4 MRT record.
/// `local_as` and `local_addr` default to 0/0.0.0.0 when not available.
pub fn rib_event_to_mrt(
    out:   &mut impl Write,
    event: &RibEvent,
) -> Result<()> {
    let ts = &event.occurred_at;
    match &event.payload {
        RibEventPayload::PeerUp { peer_header, .. } => {
            let peer_addr  = peer_header.peer_address;
            let local_addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
            write_bgp4mp_state_change(
                out, ts,
                peer_header.peer_as, 0,
                peer_addr, local_addr,
                BgpState::Active,
                BgpState::Established,
            )
        }
        RibEventPayload::PeerDown { peer_header, .. } => {
            let peer_addr  = peer_header.peer_address;
            let local_addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
            write_bgp4mp_state_change(
                out, ts,
                peer_header.peer_as, 0,
                peer_addr, local_addr,
                BgpState::Established,
                BgpState::Idle,
            )
        }
        _ => Ok(()),
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn test_bgp4mp_message_roundtrip_length() {
        let bgp_bytes = vec![0xFFu8; 19 + 10]; // dummy BGP header + 10 bytes
        let mut out = Vec::new();
        write_bgp4mp_message(
            &mut out, &ts(),
            65001, 65000, 0,
            "10.0.0.1".parse().unwrap(),
            "10.0.0.2".parse().unwrap(),
            &bgp_bytes,
        ).unwrap();

        // 12-byte MRT header + (4+4+2+2 peer_as/local_as/iface/afi) + (4+4 IPv4 addrs) + bgp_bytes.len()
        let expected = 12 + 4 + 4 + 2 + 2 + 4 + 4 + bgp_bytes.len();
        assert_eq!(out.len(), expected);
        // length field in header should match body
        let body_len = u32::from_be_bytes([out[8], out[9], out[10], out[11]]);
        assert_eq!(body_len as usize, out.len() - 12);
    }

    #[test]
    fn test_state_change_length() {
        let mut out = Vec::new();
        write_bgp4mp_state_change(
            &mut out, &ts(),
            65001, 65000,
            "192.0.2.1".parse().unwrap(),
            "192.0.2.2".parse().unwrap(),
            BgpState::Active, BgpState::Established,
        ).unwrap();

        // 12 hdr + (4+4+2+2 peer_as/local_as/iface/afi) + (4+4 IPv4 addrs) + (2+2 old/new state)
        assert_eq!(out.len(), 12 + 4 + 4 + 2 + 2 + 4 + 4 + 2 + 2);
    }

    #[test]
    fn test_peer_index_table_written() {
        let peers = vec![
            MrtPeerEntry {
                bgp_id:  "10.0.0.1".parse().unwrap(),
                addr:    "10.0.0.1".parse().unwrap(),
                peer_as: 65001,
            },
        ];
        let mut out = Vec::new();
        write_peer_index_table(
            &mut out, &ts(),
            "0.0.0.0".parse().unwrap(),
            "",
            &peers,
        ).unwrap();

        assert!(out.len() > 12, "output should have header + body");
        let mrt_type = u16::from_be_bytes([out[4], out[5]]);
        assert_eq!(mrt_type, MrtType::TableDumpV2 as u16);
        let subtype = u16::from_be_bytes([out[6], out[7]]);
        assert_eq!(subtype, TableDumpV2Subtype::PeerIndexTable as u16);
    }

    #[test]
    fn test_rib_ipv4_unicast_written() {
        use rbmp_core::bgp::Prefix;
        use ipnet::Ipv4Net;

        let prefix: Prefix = Prefix::V4("10.0.0.0/8".parse::<Ipv4Net>().unwrap());
        let entries = vec![
            MrtRibEntry {
                peer_index:  0,
                originated:  ts(),
                path_attrs:  vec![],
            }
        ];
        let mut out = Vec::new();
        write_rib_entry(&mut out, &ts(), 1, &prefix, &entries).unwrap();

        assert!(out.len() > 12);
        let subtype = u16::from_be_bytes([out[6], out[7]]);
        assert_eq!(subtype, TableDumpV2Subtype::RibIpv4Unicast as u16);
    }
}
