use bytes::Buf;
use crate::{Error, Result};
use super::types::{Afi, BgpUpdate};
use super::nlri::decode_nlri;
use super::attributes::parse_path_attributes;

const BGP_UPDATE_TYPE: u8 = 2;

/// Parse a BGP UPDATE message.
/// `buf` must start at the BGP common header (16-byte marker + 2-byte length + 1-byte type).
pub fn parse_bgp_update(mut buf: impl Buf) -> Result<BgpUpdate> {
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
    let withdrawn = decode_nlri(&mut withdrawn_bytes.as_ref(), Afi::Ipv4)?;

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
    let announced = decode_nlri(&mut buf, Afi::Ipv4)?;

    Ok(BgpUpdate { withdrawn, attributes, announced })
}
