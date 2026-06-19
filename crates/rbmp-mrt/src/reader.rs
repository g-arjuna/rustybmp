//! MRT reader — parses RFC 6396 binary MRT files record by record.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use bytes::Buf;
use chrono::{DateTime, TimeZone, Utc};
use crate::error::MrtError;
use crate::types::*;

type Result<T> = std::result::Result<T, MrtError>;

// ─── Parsed record ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct MrtRecord {
    pub timestamp: DateTime<Utc>,
    pub mrt_type:  u16,
    pub subtype:   u16,
    pub body:      Vec<u8>,
}

impl MrtRecord {
    pub fn is_table_dump_v2(&self) -> bool { self.mrt_type == MrtType::TableDumpV2 as u16 }
    pub fn is_bgp4mp(&self)       -> bool { self.mrt_type == MrtType::Bgp4Mp as u16 }
}

// ─── Record-level reader ──────────────────────────────────────────────────────

const MRT_HEADER_LEN: usize = 12;

/// Read the next MRT record from `r`.  Returns `None` at clean EOF.
pub fn read_record(r: &mut impl Read) -> Result<Option<MrtRecord>> {
    let mut hdr = [0u8; MRT_HEADER_LEN];
    match r.read_exact(&mut hdr) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(MrtError::Io(e)),
    }

    let ts_raw  = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let mrt_type = u16::from_be_bytes([hdr[4], hdr[5]]);
    let subtype  = u16::from_be_bytes([hdr[6], hdr[7]]);
    let length   = u32::from_be_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;

    let mut body = vec![0u8; length];
    r.read_exact(&mut body).map_err(MrtError::Io)?;

    let timestamp = Utc.timestamp_opt(ts_raw as i64, 0).single()
        .unwrap_or_else(Utc::now);

    Ok(Some(MrtRecord { timestamp, mrt_type, subtype, body }))
}

/// Iterator over all MRT records in a reader.
pub struct MrtReader<R: Read> {
    inner: R,
}

impl<R: Read> MrtReader<R> {
    pub fn new(inner: R) -> Self { Self { inner } }
}

impl<R: Read> Iterator for MrtReader<R> {
    type Item = Result<MrtRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        match read_record(&mut self.inner) {
            Ok(Some(r)) => Some(Ok(r)),
            Ok(None)    => None,
            Err(e)      => Some(Err(e)),
        }
    }
}

// ─── BGP4MP decoder helpers ───────────────────────────────────────────────────

/// Decoded BGP4MP_MESSAGE_AS4 / BGP4MP_STATE_CHANGE_AS4 fields.
#[derive(Debug)]
pub struct Bgp4MpHeader {
    pub peer_as:     u32,
    pub local_as:    u32,
    pub interface:   u16,
    pub peer_addr:   IpAddr,
    pub local_addr:  IpAddr,
}

/// Parse the common BGP4MP peer/local header from a record body.
pub fn parse_bgp4mp_header(body: &[u8]) -> Result<(Bgp4MpHeader, &[u8])> {
    if body.len() < 12 {
        return Err(MrtError::TooShort { need: 12, have: body.len() });
    }
    let mut b = body;
    let peer_as   = b.get_u32();
    let local_as  = b.get_u32();
    let interface = b.get_u16();
    let afi       = b.get_u16();

    let addr_len = if afi == 2 { 16usize } else { 4 };
    if b.remaining() < addr_len * 2 {
        return Err(MrtError::TooShort { need: addr_len * 2, have: b.remaining() });
    }

    let peer_addr = if afi == 2 {
        let mut a = [0u8; 16];
        a.copy_from_slice(&b[..16]);
        b.advance(16);
        IpAddr::V6(Ipv6Addr::from(a))
    } else {
        let a = [b[0], b[1], b[2], b[3]];
        b.advance(4);
        IpAddr::V4(Ipv4Addr::from(a))
    };

    let local_addr = if afi == 2 {
        let mut a = [0u8; 16];
        a.copy_from_slice(&b[..16]);
        b.advance(16);
        IpAddr::V6(Ipv6Addr::from(a))
    } else {
        let a = [b[0], b[1], b[2], b[3]];
        b.advance(4);
        IpAddr::V4(Ipv4Addr::from(a))
    };

    Ok((Bgp4MpHeader { peer_as, local_as, interface, peer_addr, local_addr }, b))
}

/// Parse the BGP PDU bytes from a BGP4MP_MESSAGE(_AS4) record body.
/// Returns `(header, bgp_pdu_slice)`.
pub fn parse_bgp4mp_message(body: &[u8]) -> Result<(Bgp4MpHeader, &[u8])> {
    parse_bgp4mp_header(body)
}

/// Parse old/new FSM states from a BGP4MP_STATE_CHANGE(_AS4) record body.
pub fn parse_bgp4mp_state_change(body: &[u8]) -> Result<(Bgp4MpHeader, u16, u16)> {
    let (hdr, rest) = parse_bgp4mp_header(body)?;
    if rest.len() < 4 {
        return Err(MrtError::TooShort { need: 4, have: rest.len() });
    }
    let old = u16::from_be_bytes([rest[0], rest[1]]);
    let new = u16::from_be_bytes([rest[2], rest[3]]);
    Ok((hdr, old, new))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn test_roundtrip_state_change() {
        let mut buf = Vec::new();
        writer::write_bgp4mp_state_change(
            &mut buf, &ts(),
            65001, 65000,
            "10.0.0.1".parse().unwrap(),
            "10.0.0.2".parse().unwrap(),
            writer::BgpState::Active,
            writer::BgpState::Established,
        ).unwrap();

        let mut cursor = std::io::Cursor::new(&buf);
        let record = read_record(&mut cursor).unwrap().unwrap();

        assert_eq!(record.mrt_type, MrtType::Bgp4Mp as u16);
        assert_eq!(record.subtype,  Bgp4MpSubtype::StateChangeAs4 as u16);
        assert_eq!(record.timestamp, ts());

        let (hdr, old, new) = parse_bgp4mp_state_change(&record.body).unwrap();
        assert_eq!(hdr.peer_as, 65001);
        assert_eq!(hdr.peer_addr, "10.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(old, 3); // Active
        assert_eq!(new, 6); // Established
    }

    #[test]
    fn test_roundtrip_bgp4mp_message() {
        let bgp_bytes = vec![0xFFu8; 29]; // 19-byte BGP header + 10 bytes body
        let mut buf = Vec::new();
        writer::write_bgp4mp_message(
            &mut buf, &ts(),
            65001, 65000, 0,
            "192.0.2.1".parse().unwrap(),
            "192.0.2.2".parse().unwrap(),
            &bgp_bytes,
        ).unwrap();

        let mut cursor = std::io::Cursor::new(&buf);
        let record = read_record(&mut cursor).unwrap().unwrap();

        let (hdr, pdu) = parse_bgp4mp_message(&record.body).unwrap();
        assert_eq!(hdr.peer_as, 65001);
        assert_eq!(pdu.len(), bgp_bytes.len());
        assert!(pdu.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn test_eof_returns_none() {
        let buf: &[u8] = &[];
        let mut cursor = std::io::Cursor::new(buf);
        let result = read_record(&mut cursor).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_mrt_reader_iterator() {
        let mut buf = Vec::new();
        for _ in 0..3 {
            writer::write_bgp4mp_state_change(
                &mut buf, &ts(),
                65001, 65000,
                "10.0.0.1".parse().unwrap(),
                "10.0.0.2".parse().unwrap(),
                writer::BgpState::Active,
                writer::BgpState::Established,
            ).unwrap();
        }

        let cursor = std::io::Cursor::new(&buf);
        let records: Vec<_> = MrtReader::new(cursor).collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(records.len(), 3);
    }
}
