use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use bytes::Buf;
use crate::{Error, Result};
use super::types::{
    BgpLsNlri, BgpLsNlriType, BgpLsProtocol, BgpLsReachNlri, BgpLsUnreachNlri,
    NodeDescriptor, LinkDescriptor, Prefix, RouterId,
};
use ipnet::{Ipv4Net, Ipv6Net};

// ─── Node descriptor sub-TLV codes (RFC 7752 §3.2.1.2 Table 3) ───────────────
const ND_AS_NUMBER:     u16 = 512;
const ND_BGP_LS_ID:     u16 = 513;
const ND_OSPF_AREA:     u16 = 514;
const ND_IGP_ROUTER_ID: u16 = 515;

// ─── Link descriptor sub-TLV codes (RFC 7752 §3.2.2 Table 5) ─────────────────
const LD_LINK_LOCAL_REMOTE_ID: u16 = 258;
const LD_IPV4_IF_ADDR:         u16 = 259;
const LD_IPV4_NEIGHBOR_ADDR:   u16 = 260;
const LD_IPV6_IF_ADDR:         u16 = 261;
const LD_IPV6_NEIGHBOR_ADDR:   u16 = 262;

// ─── Prefix descriptor sub-TLV codes (RFC 7752 §3.2.3 Table 6) ───────────────
const PD_OSPF_ROUTE_TYPE: u16 = 264;
const PD_IP_REACHABILITY:  u16 = 265;

/// Decode a list of BGP-LS NLRIs from a flat byte buffer.
/// Used by both MP_REACH and MP_UNREACH parsers.
pub fn decode_bgpls_nlri(buf: &[u8]) -> Result<Vec<BgpLsNlri>> {
    let mut cur = std::io::Cursor::new(buf);
    let mut nlris = Vec::new();

    while cur.remaining() >= 4 {
        let nlri_type = cur.get_u16();
        let nlri_len  = cur.get_u16() as usize;
        if cur.remaining() < nlri_len {
            return Err(Error::UnexpectedEof { needed: nlri_len, have: cur.remaining() });
        }
        let nlri_bytes = cur.copy_to_bytes(nlri_len);
        let nlri = decode_one(nlri_type, &nlri_bytes)?;
        nlris.push(nlri);
    }
    Ok(nlris)
}

/// Decode MP_REACH BGP-LS (next_hop already consumed by caller, buf = NLRI bytes only)
pub fn decode_bgpls_reach(next_hop: Option<IpAddr>, nlri_buf: &[u8]) -> Result<BgpLsReachNlri> {
    let nlris = decode_bgpls_nlri(nlri_buf)?;
    Ok(BgpLsReachNlri { next_hop, nlris })
}

/// Decode MP_UNREACH BGP-LS
pub fn decode_bgpls_unreach(buf: &[u8]) -> Result<BgpLsUnreachNlri> {
    let nlris = decode_bgpls_nlri(buf)?;
    Ok(BgpLsUnreachNlri { nlris })
}

// ─── Internal decoder ─────────────────────────────────────────────────────────

fn decode_one(nlri_type: u16, buf: &[u8]) -> Result<BgpLsNlri> {
    // Every NLRI starts with: protocol_id(1) + identifier(8) + local_node_descriptors_tlv
    if buf.len() < 9 {
        return Ok(BgpLsNlri::Unknown { nlri_type, data: buf.to_vec() });
    }
    let protocol   = BgpLsProtocol::from(buf[0]);
    let identifier = u64::from_be_bytes([buf[1],buf[2],buf[3],buf[4],buf[5],buf[6],buf[7],buf[8]]);
    let rest       = &buf[9..];

    match BgpLsNlriType::from(nlri_type) {
        BgpLsNlriType::Node => {
            let local_node = decode_node_descriptor(rest)?;
            Ok(BgpLsNlri::Node { protocol, identifier, local_node })
        }
        BgpLsNlriType::Link => {
            decode_link_nlri(protocol, identifier, rest)
        }
        BgpLsNlriType::Ipv4Prefix | BgpLsNlriType::Ipv6Prefix => {
            let is_v6 = nlri_type == 4;
            decode_prefix_nlri(protocol, identifier, rest, is_v6)
        }
        BgpLsNlriType::Unknown(_) => {
            Ok(BgpLsNlri::Unknown { nlri_type, data: buf.to_vec() })
        }
    }
}

// ─── Node descriptor (RFC 7752 §3.2.1) ───────────────────────────────────────

fn decode_node_descriptor(buf: &[u8]) -> Result<NodeDescriptor> {
    // buf should start with the Node Descriptor TLV:
    // type(2) + length(2) + sub-TLVs
    let mut cur = std::io::Cursor::new(buf);
    let mut nd  = NodeDescriptor::default();

    // Outer TLV wrapper — type must be 256 (local) or 257 (remote)
    if cur.remaining() < 4 { return Ok(nd); }
    let _outer_type = cur.get_u16();   // 256 = local, 257 = remote
    let outer_len   = cur.get_u16() as usize;
    if cur.remaining() < outer_len { return Ok(nd); }
    let sub_bytes = cur.copy_to_bytes(outer_len);
    decode_node_sub_tlvs(&sub_bytes, &mut nd);
    Ok(nd)
}

fn decode_node_sub_tlvs(buf: &[u8], nd: &mut NodeDescriptor) {
    let mut cur = std::io::Cursor::new(buf);
    while cur.remaining() >= 4 {
        let sub_type = cur.get_u16();
        let sub_len  = cur.get_u16() as usize;
        if cur.remaining() < sub_len { break; }
        let val = cur.copy_to_bytes(sub_len);
        match sub_type {
            ND_AS_NUMBER if val.len() >= 4 => {
                nd.asn = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
            }
            ND_BGP_LS_ID if val.len() >= 4 => {
                nd.bgp_ls_id = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
            }
            ND_OSPF_AREA if val.len() >= 4 => {
                nd.ospf_area = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
            }
            ND_IGP_ROUTER_ID => {
                nd.igp_router = Some(RouterId { bytes: val.to_vec() });
            }
            _ => {}
        }
    }
}

// ─── Link NLRI (RFC 7752 §3.2.2) ─────────────────────────────────────────────

fn decode_link_nlri(protocol: BgpLsProtocol, identifier: u64, buf: &[u8]) -> Result<BgpLsNlri> {
    let mut cur = std::io::Cursor::new(buf);
    let mut local_node  = NodeDescriptor::default();
    let mut remote_node = NodeDescriptor::default();
    let mut link        = LinkDescriptor::default();

    while cur.remaining() >= 4 {
        let tlv_type = cur.get_u16();
        let tlv_len  = cur.get_u16() as usize;
        if cur.remaining() < tlv_len { break; }
        let val = cur.copy_to_bytes(tlv_len);

        match tlv_type {
            256 => { // Local Node Descriptor
                decode_node_sub_tlvs(&val, &mut local_node);
            }
            257 => { // Remote Node Descriptor
                decode_node_sub_tlvs(&val, &mut remote_node);
            }
            LD_LINK_LOCAL_REMOTE_ID if val.len() >= 8 => {
                link.local_id  = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
                link.remote_id = Some(u32::from_be_bytes([val[4], val[5], val[6], val[7]]));
            }
            LD_IPV4_IF_ADDR if val.len() >= 4 => {
                link.local_addr = Some(IpAddr::V4(Ipv4Addr::from([val[0], val[1], val[2], val[3]])));
            }
            LD_IPV4_NEIGHBOR_ADDR if val.len() >= 4 => {
                link.remote_addr = Some(IpAddr::V4(Ipv4Addr::from([val[0], val[1], val[2], val[3]])));
            }
            LD_IPV6_IF_ADDR if val.len() >= 16 => {
                let mut a = [0u8; 16]; a.copy_from_slice(&val[..16]);
                link.local_addr = Some(IpAddr::V6(Ipv6Addr::from(a)));
            }
            LD_IPV6_NEIGHBOR_ADDR if val.len() >= 16 => {
                let mut a = [0u8; 16]; a.copy_from_slice(&val[..16]);
                link.remote_addr = Some(IpAddr::V6(Ipv6Addr::from(a)));
            }
            _ => {}
        }
    }

    Ok(BgpLsNlri::Link { protocol, identifier, local_node, remote_node, link })
}

// ─── Prefix NLRI (RFC 7752 §3.2.3) ───────────────────────────────────────────

fn decode_prefix_nlri(
    protocol:   BgpLsProtocol,
    identifier: u64,
    buf:        &[u8],
    is_v6:      bool,
) -> Result<BgpLsNlri> {
    let mut cur        = std::io::Cursor::new(buf);
    let mut local_node = NodeDescriptor::default();
    let mut prefix_opt: Option<Prefix> = None;

    while cur.remaining() >= 4 {
        let tlv_type = cur.get_u16();
        let tlv_len  = cur.get_u16() as usize;
        if cur.remaining() < tlv_len { break; }
        let val = cur.copy_to_bytes(tlv_len);

        match tlv_type {
            256 => {
                decode_node_sub_tlvs(&val, &mut local_node);
            }
            PD_IP_REACHABILITY if val.len() >= 1 => {
                // prefix_len(1) + prefix_bytes (ceil(len/8))
                let plen   = val[0];
                let octets = (plen as usize + 7) / 8;
                if val.len() >= 1 + octets {
                    prefix_opt = if is_v6 {
                        let mut a = [0u8; 16];
                        a[..octets].copy_from_slice(&val[1..1+octets]);
                        Ipv6Net::new(Ipv6Addr::from(a), plen).ok()
                            .map(|n| Prefix::V6(n.trunc()))
                    } else {
                        let mut a = [0u8; 4];
                        a[..octets].copy_from_slice(&val[1..1+octets]);
                        Ipv4Net::new(Ipv4Addr::from(a), plen).ok()
                            .map(|n| Prefix::V4(n.trunc()))
                    };
                }
            }
            PD_OSPF_ROUTE_TYPE | _ => {}
        }
    }

    let prefix = prefix_opt.unwrap_or(if is_v6 {
        Prefix::V6(Ipv6Net::new(Ipv6Addr::UNSPECIFIED, 0).unwrap())
    } else {
        Prefix::V4(Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0).unwrap())
    });

    Ok(BgpLsNlri::Prefix { protocol, identifier, local_node, prefix })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buf_returns_empty() {
        let result = decode_bgpls_nlri(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_unknown_nlri_type_preserved() {
        // type=99, len=4, data=0xDEADBEEF
        let buf = [0u8, 99, 0, 4, 0xDE, 0xAD, 0xBE, 0xEF];
        let result = decode_bgpls_nlri(&buf).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], BgpLsNlri::Unknown { nlri_type: 99, .. }));
    }

    #[test]
    fn test_node_nlri_protocol_identifier() {
        // Build a minimal Node NLRI: type=1, len=13 (9 hdr + 4 empty outer TLV)
        // protocol=3 (OSPFv2), identifier=42
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_be_bytes());  // type = Node
        buf.extend_from_slice(&13u16.to_be_bytes()); // len
        buf.push(3);                                  // protocol = OSPFv2
        buf.extend_from_slice(&42u64.to_be_bytes());  // identifier
        // outer node descriptor TLV type=256, len=0
        buf.extend_from_slice(&256u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        let result = decode_bgpls_nlri(&buf).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            BgpLsNlri::Node { protocol, identifier, .. } => {
                assert_eq!(*protocol, BgpLsProtocol::Ospfv2);
                assert_eq!(*identifier, 42);
            }
            _ => panic!("expected Node"),
        }
    }
}
