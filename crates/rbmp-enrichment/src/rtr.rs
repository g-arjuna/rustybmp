use std::net::IpAddr;
use std::time::Duration;
use anyhow::{anyhow, Result};
use bytes::{Buf, BytesMut};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use crate::vrp_cache::{VrpCache, VrpEntry};

// ─── RTR PDU types (RFC 8210 §5) ─────────────────────────────────────────────

const PDU_SERIAL_NOTIFY:   u8 = 0;
const PDU_SERIAL_QUERY:    u8 = 1;
const PDU_RESET_QUERY:     u8 = 2;
const PDU_CACHE_RESPONSE:  u8 = 3;
const PDU_IPV4_PREFIX:     u8 = 4;
const PDU_IPV6_PREFIX:     u8 = 6;
const PDU_END_OF_DATA:     u8 = 7;
const PDU_CACHE_RESET:     u8 = 8;
const PDU_ROUTER_KEY:      u8 = 9;
const PDU_ERROR_REPORT:    u8 = 10;

const RTR_VERSION: u8 = 2;   // RFC 8210 uses version 2
const RTR_HDR_LEN: usize = 8; // version(1) + type(1) + session/error(2) + length(4)

/// RTR client — connects to an RPKI cache server (Routinator, rpki-client, etc.)
/// and feeds validated ROA payloads into a `VrpCache`.
pub struct RtrClient {
    addr:   String,
    cache:  VrpCache,
}

impl RtrClient {
    pub fn new(addr: impl Into<String>, cache: VrpCache) -> Self {
        Self { addr: addr.into(), cache }
    }

    /// Run the RTR session loop. Reconnects with exponential backoff on failure.
    /// Cancelled via the token.
    pub async fn run(self, cancel: CancellationToken) {
        let mut backoff = Duration::from_secs(5);
        loop {
            if cancel.is_cancelled() { return; }
            match self.connect_and_sync(cancel.clone()).await {
                Ok(()) => {
                    info!(addr = %self.addr, "RTR session ended cleanly");
                    backoff = Duration::from_secs(5);
                }
                Err(e) => {
                    warn!(addr = %self.addr, error = %e, backoff_secs = backoff.as_secs(),
                          "RTR session error — reconnecting");
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(300));
        }
    }

    async fn connect_and_sync(&self, cancel: CancellationToken) -> Result<()> {
        info!(addr = %self.addr, "RTR connecting");
        let mut stream = TcpStream::connect(&self.addr).await?;
        info!(addr = %self.addr, "RTR connected");

        // Send Reset Query to get full VRP table
        send_reset_query(&mut stream).await?;

        let mut buf = BytesMut::with_capacity(65536);
        // Pending VRP entries being accumulated during a cache response
        let mut pending: Vec<VrpEntry> = Vec::new();
        let mut in_cache_response = false;
        let mut session_id: u16 = 0;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                result = stream.read_buf(&mut buf) => {
                    match result {
                        Ok(0) => return Err(anyhow!("RTR server closed connection")),
                        Ok(_) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }

            // Drain complete PDUs
            loop {
                if buf.len() < RTR_HDR_LEN { break; }
                let pdu_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
                if pdu_len < RTR_HDR_LEN {
                    return Err(anyhow!("RTR PDU length {pdu_len} < header size"));
                }
                if buf.len() < pdu_len { break; }

                let pdu = buf.copy_to_bytes(pdu_len);
                let pdu_type = pdu[1];

                match pdu_type {
                    PDU_CACHE_RESPONSE => {
                        session_id = u16::from_be_bytes([pdu[2], pdu[3]]);
                        in_cache_response = true;
                        pending.clear();
                        debug!(session_id, "RTR Cache Response — collecting VRPs");
                    }
                    PDU_IPV4_PREFIX => {
                        // flags(1) + prefix_len(1) + max_len(1) + zero(1) + prefix(4) + asn(4) = 16 body bytes
                        if pdu.len() < RTR_HDR_LEN + 12 {
                            warn!("RTR IPv4 Prefix PDU too short ({})", pdu.len()); continue;
                        }
                        let flags      = pdu[8];
                        let prefix_len = pdu[9];
                        let max_len    = pdu[10];
                        // pdu[11] = zero padding
                        let prefix_bytes: [u8; 4] = pdu[12..16].try_into()?;
                        let asn = u32::from_be_bytes([pdu[16], pdu[17], pdu[18], pdu[19]]);
                        let addr = std::net::Ipv4Addr::from(prefix_bytes);
                        let net  = Ipv4Net::new(addr, prefix_len)
                            .map_err(|e| anyhow!("Bad IPv4 prefix: {e}"))?;
                        let entry = VrpEntry { prefix: IpNet::V4(net.trunc()), max_len, origin_asn: asn };
                        if flags & 0x01 != 0 {
                            pending.push(entry); // announce
                        } else {
                            // flags==0 means withdrawal in incremental update
                            self.cache.apply_delta(vec![], vec![entry], self.cache.serial());
                        }
                    }
                    PDU_IPV6_PREFIX => {
                        if pdu.len() < RTR_HDR_LEN + 28 {
                            warn!("RTR IPv6 Prefix PDU too short ({})", pdu.len()); continue;
                        }
                        let flags      = pdu[8];
                        let prefix_len = pdu[9];
                        let max_len    = pdu[10];
                        let prefix_bytes: [u8; 16] = pdu[12..28].try_into()?;
                        let asn = u32::from_be_bytes([pdu[28], pdu[29], pdu[30], pdu[31]]);
                        let addr = std::net::Ipv6Addr::from(prefix_bytes);
                        let net  = Ipv6Net::new(addr, prefix_len)
                            .map_err(|e| anyhow!("Bad IPv6 prefix: {e}"))?;
                        let entry = VrpEntry { prefix: IpNet::V6(net.trunc()), max_len, origin_asn: asn };
                        if flags & 0x01 != 0 {
                            pending.push(entry);
                        } else {
                            self.cache.apply_delta(vec![], vec![entry], self.cache.serial());
                        }
                    }
                    PDU_END_OF_DATA => {
                        let serial = u32::from_be_bytes([pdu[8], pdu[9], pdu[10], pdu[11]]);
                        if in_cache_response {
                            let n = pending.len();
                            self.cache.reset(std::mem::take(&mut pending), serial);
                            in_cache_response = false;
                            info!(vrp_count = n, serial, "RTR VRP table loaded");
                        } else {
                            // Incremental update complete — serial already applied per-prefix
                            self.cache.apply_delta(vec![], vec![], serial);
                            debug!(serial, "RTR incremental update complete");
                        }
                    }
                    PDU_CACHE_RESET => {
                        warn!("RTR Cache Reset received — sending Reset Query");
                        send_reset_query(&mut stream).await?;
                        pending.clear();
                        in_cache_response = false;
                    }
                    PDU_SERIAL_NOTIFY => {
                        let serial = u32::from_be_bytes([pdu[4], pdu[5], pdu[6], pdu[7]]);
                        debug!(serial, "RTR Serial Notify — sending Serial Query");
                        send_serial_query(&mut stream, session_id, self.cache.serial()).await?;
                    }
                    PDU_ERROR_REPORT => {
                        let err_code = u16::from_be_bytes([pdu[2], pdu[3]]);
                        warn!(err_code, "RTR Error Report received");
                        return Err(anyhow!("RTR error code {err_code}"));
                    }
                    PDU_ROUTER_KEY => {
                        // BGPsec router key — not used by BMP collector, skip
                        debug!("RTR Router Key PDU — skipped");
                    }
                    other => {
                        debug!(pdu_type = other, "RTR unknown PDU type — skipped");
                    }
                }
            }
        }
    }
}

async fn send_reset_query(stream: &mut TcpStream) -> Result<()> {
    // Reset Query PDU: version(1) + type=2(1) + zero(2) + length=8(4)
    let pdu: [u8; 8] = [RTR_VERSION, PDU_RESET_QUERY, 0, 0, 0, 0, 0, 8];
    stream.write_all(&pdu).await?;
    Ok(())
}

async fn send_serial_query(stream: &mut TcpStream, session_id: u16, serial: u32) -> Result<()> {
    // Serial Query PDU: version(1) + type=1(1) + session_id(2) + length=12(4) + serial(4)
    let mut pdu = [0u8; 12];
    pdu[0] = RTR_VERSION;
    pdu[1] = PDU_SERIAL_QUERY;
    pdu[2..4].copy_from_slice(&session_id.to_be_bytes());
    pdu[4..8].copy_from_slice(&12u32.to_be_bytes());
    pdu[8..12].copy_from_slice(&serial.to_be_bytes());
    stream.write_all(&pdu).await?;
    Ok(())
}
