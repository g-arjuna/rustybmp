use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use bytes::Buf;
use crate::{Error, Result};
use super::types::*;
use super::nlri::{decode_nlri, decode_labeled_nlri, decode_vpn_nlri, decode_next_hops};

// ─── Attribute flag bits ──────────────────────────────────────────────────────
const FLAG_OPTIONAL:   u8 = 0x80;
const FLAG_TRANSITIVE: u8 = 0x40;
const FLAG_PARTIAL:    u8 = 0x20;
const FLAG_EXT_LEN:    u8 = 0x10;

/// Parse all path attributes from buf, returning a PathAttributes struct.
/// buf must contain exactly the path attributes section (already length-delimited).
pub fn parse_path_attributes(mut buf: impl Buf) -> Result<PathAttributes> {
    let mut attrs = PathAttributes::default();
    while buf.remaining() >= 3 {
        let flags     = buf.get_u8();
        let type_code = buf.get_u8();
        let attr_len = if flags & FLAG_EXT_LEN != 0 {
            if buf.remaining() < 2 {
                return Err(Error::UnexpectedEof { needed: 2, have: buf.remaining() });
            }
            buf.get_u16() as usize
        } else {
            buf.get_u8() as usize
        };

        if buf.remaining() < attr_len {
            return Err(Error::TruncatedAttribute {
                attr_type: type_code,
                declared: attr_len,
                available: buf.remaining(),
            });
        }

        let attr_data = buf.copy_to_bytes(attr_len);
        let attr_buf  = attr_data.as_ref();

        match type_code {
            // Type 1: ORIGIN
            1 if attr_len >= 1 => {
                attrs.origin = Some(Origin::try_from(attr_buf[0])?);
            }
            // Type 2: AS_PATH
            2 => {
                attrs.as_path = Some(parse_as_path(attr_buf, false)?);
            }
            // Type 3: NEXT_HOP
            3 if attr_len == 4 => {
                attrs.next_hop = Some(IpAddr::V4(Ipv4Addr::from([attr_buf[0], attr_buf[1], attr_buf[2], attr_buf[3]])));
            }
            // Type 4: MULTI_EXIT_DISC
            4 if attr_len == 4 => {
                attrs.multi_exit_disc = Some(u32::from_be_bytes(attr_buf[..4].try_into().unwrap()));
            }
            // Type 5: LOCAL_PREF
            5 if attr_len == 4 => {
                attrs.local_pref = Some(u32::from_be_bytes(attr_buf[..4].try_into().unwrap()));
            }
            // Type 6: ATOMIC_AGGREGATE
            6 => {
                attrs.atomic_aggregate = true;
            }
            // Type 7: AGGREGATOR (2+4 or 4+4 byte forms)
            7 if attr_len == 6 => {
                let asn = u16::from_be_bytes([attr_buf[0], attr_buf[1]]) as u32;
                let id  = Ipv4Addr::from([attr_buf[2], attr_buf[3], attr_buf[4], attr_buf[5]]);
                attrs.aggregator = Some((asn, id));
            }
            7 if attr_len == 8 => {
                let asn = u32::from_be_bytes([attr_buf[0], attr_buf[1], attr_buf[2], attr_buf[3]]);
                let id  = Ipv4Addr::from([attr_buf[4], attr_buf[5], attr_buf[6], attr_buf[7]]);
                attrs.aggregator = Some((asn, id));
            }
            // Type 8: COMMUNITY (RFC 1997)
            8 => {
                let mut cb = attr_data.as_ref();
                while cb.len() >= 4 {
                    let val = u32::from_be_bytes([cb[0], cb[1], cb[2], cb[3]]);
                    attrs.communities.push(StandardCommunity(val));
                    cb = &cb[4..];
                }
            }
            // Type 9: ORIGINATOR_ID (RFC 4456)
            9 if attr_len == 4 => {
                attrs.originator_id = Some(Ipv4Addr::from([attr_buf[0], attr_buf[1], attr_buf[2], attr_buf[3]]));
            }
            // Type 10: CLUSTER_LIST (RFC 4456)
            10 => {
                let mut cb = attr_data.as_ref();
                while cb.len() >= 4 {
                    attrs.cluster_list.push(Ipv4Addr::from([cb[0], cb[1], cb[2], cb[3]]));
                    cb = &cb[4..];
                }
            }
            // Type 14: MP_REACH_NLRI (RFC 4760)
            14 if attr_len >= 4 => {
                attrs.mp_reach = Some(parse_mp_reach(attr_data.as_ref())?);
            }
            // Type 15: MP_UNREACH_NLRI (RFC 4760)
            15 if attr_len >= 3 => {
                attrs.mp_unreach = Some(parse_mp_unreach(attr_data.as_ref())?);
            }
            // Type 16: EXTENDED COMMUNITIES (RFC 4360)
            16 => {
                let mut cb = attr_data.as_ref();
                while cb.len() >= 8 {
                    let raw: [u8; 8] = cb[..8].try_into().unwrap();
                    attrs.extended_communities.push(ExtendedCommunity::from_bytes(&raw));
                    cb = &cb[8..];
                }
            }
            // Type 17: AS4_PATH (RFC 6793)
            17 => {
                attrs.as4_path = Some(parse_as_path(attr_buf, true)?);
            }
            // Type 18: AS4_AGGREGATOR (RFC 6793)
            18 if attr_len == 8 => {
                let asn = u32::from_be_bytes([attr_buf[0], attr_buf[1], attr_buf[2], attr_buf[3]]);
                let id  = Ipv4Addr::from([attr_buf[4], attr_buf[5], attr_buf[6], attr_buf[7]]);
                attrs.as4_aggregator = Some((asn, id));
            }
            // Type 32: LARGE_COMMUNITY (RFC 8092)
            32 => {
                let mut cb = attr_data.as_ref();
                while cb.len() >= 12 {
                    let ga = u32::from_be_bytes([cb[0], cb[1], cb[2], cb[3]]);
                    let ld1 = u32::from_be_bytes([cb[4], cb[5], cb[6], cb[7]]);
                    let ld2 = u32::from_be_bytes([cb[8], cb[9], cb[10], cb[11]]);
                    attrs.large_communities.push(LargeCommunity { global_admin: ga, local_data_1: ld1, local_data_2: ld2 });
                    cb = &cb[12..];
                }
            }
            // Everything else preserved as raw
            _ => {
                attrs.unknown.push(RawAttribute {
                    flags,
                    type_code,
                    value: attr_data.to_vec(),
                });
            }
        }
    }
    Ok(attrs)
}

/// Parse AS_PATH or AS4_PATH attribute.
/// four_byte: true for AS4_PATH (all ASNs are 4 bytes), false for AS_PATH (2 bytes unless AS4 cap negotiated).
pub fn parse_as_path(mut buf: &[u8], four_byte: bool) -> Result<AsPath> {
    let mut segments = Vec::new();
    while buf.len() >= 2 {
        let seg_type = buf[0];
        let seg_len  = buf[1] as usize;
        buf = &buf[2..];
        let asn_size = if four_byte { 4 } else { 2 };
        let needed = seg_len * asn_size;
        if buf.len() < needed {
            return Err(Error::UnexpectedEof { needed, have: buf.len() });
        }
        let mut asns: Vec<u32> = Vec::new();
        for i in 0..seg_len {
            let asn = if four_byte {
                u32::from_be_bytes(buf[i*4..(i+1)*4].try_into().unwrap())
            } else {
                u16::from_be_bytes(buf[i*2..(i+1)*2].try_into().unwrap()) as u32
            };
            asns.push(asn);
        }
        buf = &buf[needed..];
        let seg = match seg_type {
            1 => AsPathSegment::Set(asns),
            2 => AsPathSegment::Sequence(asns),
            3 => AsPathSegment::ConfedSequence(asns),
            4 => AsPathSegment::ConfedSet(asns),
            _ => return Err(Error::BgpParse(format!("unknown AS_PATH segment type {seg_type}"))),
        };
        segments.push(seg);
    }
    Ok(AsPath(segments))
}

fn parse_mp_reach(buf: &[u8]) -> Result<MpReachNlri> {
    if buf.len() < 4 {
        return Err(Error::UnexpectedEof { needed: 4, have: buf.len() });
    }
    let afi  = u16::from_be_bytes([buf[0], buf[1]]);
    let safi = buf[2];
    let afi_safi = AfiSafi::new(afi, safi);
    let mut cur = std::io::Cursor::new(&buf[3..]);
    let next_hops = decode_next_hops(&mut cur, afi_safi.afi)?;
    // 1 byte SNPA (should be 0)
    if cur.remaining() < 1 {
        return Err(Error::UnexpectedEof { needed: 1, have: cur.remaining() });
    }
    let _snpa = cur.get_u8();
    let prefixes = dispatch_nlri_decode(&mut cur, afi_safi)?;
    Ok(MpReachNlri { afi_safi, next_hops, prefixes })
}

fn parse_mp_unreach(buf: &[u8]) -> Result<MpUnreachNlri> {
    if buf.len() < 3 {
        return Err(Error::UnexpectedEof { needed: 3, have: buf.len() });
    }
    let afi  = u16::from_be_bytes([buf[0], buf[1]]);
    let safi = buf[2];
    let afi_safi = AfiSafi::new(afi, safi);
    let mut cur = std::io::Cursor::new(&buf[3..]);
    let prefixes = dispatch_nlri_decode(&mut cur, afi_safi)?;
    Ok(MpUnreachNlri { afi_safi, prefixes })
}

fn dispatch_nlri_decode(buf: &mut impl Buf, afi_safi: AfiSafi) -> Result<Vec<Prefix>> {
    match afi_safi.safi {
        Safi::Unicast | Safi::Multicast => decode_nlri(buf, afi_safi.afi),
        Safi::LabeledUnicast            => decode_labeled_nlri(buf, afi_safi.afi),
        Safi::MplsVpn                   => decode_vpn_nlri(buf, afi_safi.afi),
        _                               => decode_nlri(buf, afi_safi.afi), // best-effort
    }
}
