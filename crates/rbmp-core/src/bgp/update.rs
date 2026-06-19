use bytes::Buf;
use crate::{Error, Result};
use super::types::{Afi, BgpUpdate};
use super::nlri::{decode_nlri, decode_nlri_with_path_id};
use super::attributes::parse_path_attributes;

const BGP_UPDATE_TYPE: u8 = 2;

/// Parse a BGP UPDATE message.
/// `buf` must start at the BGP common header (16-byte marker + 2-byte length + 1-byte type).
/// `add_path_ipv4` — set true when the peer negotiated Add-Path for IPv4 unicast (RFC 7911).
pub fn parse_bgp_update(mut buf: impl Buf, add_path_ipv4: bool) -> Result<BgpUpdate> {
    // Validate marker
    let marker = buf.copy_to_bytes(16);
    if marker.as_ref() != &[0xFF_u8; 16] {
        return Err(Error::InvalidBgpMarker);
    }
    let _msg_len  = buf.get_u16();
    let msg_type  = buf.get_u8();
    if msg_type != BGP_UPDATE_TYPE {
        return Err(Error::InvalidBgpMessageType(msg_type));
    }

    // Withdrawn routes (IPv4 unicast NLRI)
    if buf.remaining() < 2 {
        return Err(Error::UnexpectedEof { needed: 2, have: buf.remaining() });
    }
    let withdrawn_len = buf.get_u16() as usize;
    if buf.remaining() < withdrawn_len {
        return Err(Error::UnexpectedEof { needed: withdrawn_len, have: buf.remaining() });
    }
    let withdrawn_bytes = buf.copy_to_bytes(withdrawn_len);
    let withdrawn_with_ids = decode_nlri_with_path_id(
        &mut withdrawn_bytes.as_ref(), Afi::Ipv4, add_path_ipv4,
    )?;
    let (withdrawn, withdrawn_path_ids): (Vec<_>, Vec<_>) = withdrawn_with_ids.into_iter().unzip();

    // Path attributes
    if buf.remaining() < 2 {
        return Err(Error::UnexpectedEof { needed: 2, have: buf.remaining() });
    }
    let attr_len = buf.get_u16() as usize;
    if buf.remaining() < attr_len {
        return Err(Error::UnexpectedEof { needed: attr_len, have: buf.remaining() });
    }
    let attr_bytes = buf.copy_to_bytes(attr_len);
    let attributes = parse_path_attributes(attr_bytes.as_ref())?;

    // Announced routes (remaining NLRI = IPv4 unicast)
    let announced_with_ids = decode_nlri_with_path_id(&mut buf, Afi::Ipv4, add_path_ipv4)?;
    let (announced, announced_path_ids): (Vec<_>, Vec<_>) = announced_with_ids.into_iter().unzip();

    Ok(BgpUpdate { withdrawn, withdrawn_path_ids, attributes, announced, announced_path_ids })
}
