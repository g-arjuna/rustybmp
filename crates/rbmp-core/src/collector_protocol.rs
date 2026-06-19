//! Collector ↔ Core framing protocol (RV3-10).
//!
//! Wire format: length-prefixed MessagePack over TCP, port 5001 default.
//!
//! Frame layout:
//! ```text
//! ┌──────────┬─────────────────────────────────┐
//! │ len: u32 │ MessagePack-encoded Envelope     │
//! │ (BE)     │                                  │
//! └──────────┴─────────────────────────────────┘
//! ```
//!
//! `len` is the byte-length of the MessagePack body (not including the 4-byte length field).
//! Maximum frame size: 8 MiB.

use std::net::IpAddr;
use bytes::{Buf, BufMut, BytesMut};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use crate::error::{Error, Result};

pub const COLLECTOR_PORT: u16 = 5001;
pub const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024; // 8 MiB

// ─── Envelope ─────────────────────────────────────────────────────────────────

/// A framed message sent from a `rbmp-collector` instance to the Core.
///
/// The `raw_bmp` field contains the original BMP PDU bytes exactly as received
/// from the router — the Core re-parses it so no parsed state crosses the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorEnvelope {
    /// Human-readable collector identifier (e.g. hostname or UUID).
    pub collector_id: String,
    /// Deployment site label configured in the collector (e.g. "fra01", "sin02").
    pub site: String,
    /// Source IP of the BMP speaker (as seen by the collector).
    pub speaker_addr: IpAddr,
    /// Wall-clock timestamp when the collector received this PDU.
    pub received_at: DateTime<Utc>,
    /// Raw BMP PDU bytes (one complete BMP message, including the 6-byte header).
    pub raw_bmp: Vec<u8>,
}

// ─── Codec helpers ────────────────────────────────────────────────────────────

/// Encode a `CollectorEnvelope` to MessagePack and prepend a 4-byte BE length.
pub fn encode_frame(env: &CollectorEnvelope) -> Result<BytesMut> {
    let payload = rmp_serde::to_vec(env)
        .map_err(|e| Error::BmpParse(format!("msgpack encode: {e}")))?;

    if payload.len() as u64 > MAX_FRAME_BYTES as u64 {
        return Err(Error::FrameTooLarge(payload.len() as u32, MAX_FRAME_BYTES));
    }

    let mut buf = BytesMut::with_capacity(4 + payload.len());
    buf.put_u32(payload.len() as u32);
    buf.put_slice(&payload);
    Ok(buf)
}

/// Decode a `CollectorEnvelope` from a raw MessagePack body (without length prefix).
pub fn decode_body(body: &[u8]) -> Result<CollectorEnvelope> {
    rmp_serde::from_slice(body)
        .map_err(|e| Error::BmpParse(format!("msgpack decode: {e}")))
}

// ─── Async frame I/O ──────────────────────────────────────────────────────────

/// Read one length-prefixed frame from an async reader.
/// Returns `None` on clean EOF.
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<CollectorEnvelope>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    }

    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge(len, MAX_FRAME_BYTES));
    }

    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).await.map_err(Error::Io)?;
    Ok(Some(decode_body(&body)?))
}

/// Write one length-prefixed frame to an async writer.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    env:    &CollectorEnvelope,
) -> Result<()> {
    let buf = encode_frame(env)?;
    writer.write_all(&buf).await.map_err(Error::Io)?;
    writer.flush().await.map_err(Error::Io)?;
    Ok(())
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::net::Ipv4Addr;

    fn sample_envelope() -> CollectorEnvelope {
        CollectorEnvelope {
            collector_id: "test-collector".into(),
            site:         "lab01".into(),
            speaker_addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            received_at:  Utc::now(),
            raw_bmp:      vec![0x03, 0x00, 0x00, 0x00, 0x06, 0x00], // minimal BMP header
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let env   = sample_envelope();
        let frame = encode_frame(&env).unwrap();

        // Length prefix must be 4 bytes + body
        assert!(frame.len() > 4);
        let declared_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        assert_eq!(declared_len as usize, frame.len() - 4);

        let body   = &frame[4..];
        let decoded = decode_body(body).unwrap();
        assert_eq!(decoded.collector_id, "test-collector");
        assert_eq!(decoded.site, "lab01");
        assert_eq!(decoded.raw_bmp, env.raw_bmp);
    }

    #[tokio::test]
    async fn test_async_frame_roundtrip() {
        let env = sample_envelope();

        // Use a tokio duplex pipe as the in-memory transport
        let (mut client, mut server) = tokio::io::duplex(4096);

        write_frame(&mut client, &env).await.unwrap();
        // Close the write side so the server sees EOF after the frame
        drop(client);

        let decoded = read_frame(&mut server).await.unwrap().unwrap();
        assert_eq!(decoded.collector_id, "test-collector");
        assert_eq!(decoded.site, "lab01");
        // After the single frame, next read should be EOF / None
        let eof = read_frame(&mut server).await.unwrap();
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn test_eof_returns_none() {
        let buf: &[u8] = &[];
        let mut reader = tokio::io::BufReader::new(buf);
        let result = read_frame(&mut reader).await.unwrap();
        assert!(result.is_none());
    }
}
