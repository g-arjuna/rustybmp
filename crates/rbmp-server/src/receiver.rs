use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug, instrument};
use metrics::counter;
use chrono::Utc;
use rbmp_core::bmp::parser::parse_bmp_message;
use rbmp_core::bmp::types::{BmpMessage, BmpPayload};
use crate::archive::BmpArchive;
use crate::config::BmpConfig;
use crate::dns::DnsCache;
use crate::governor::ShedSignal;

const BMP_HEADER_LEN: usize = 6;

/// Start the BMP TCP receiver.
pub async fn run_bmp_receiver(
    cfg:     BmpConfig,
    cancel:  CancellationToken,
    tx:      mpsc::Sender<BmpMessage>,
    shed:    ShedSignal,
    archive: Arc<BmpArchive>,
    dns:     Option<DnsCache>,
) -> Result<()> {
    let listener = TcpListener::bind(&cfg.listen_addr).await?;
    info!(addr = %cfg.listen_addr, "BMP receiver listening");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("BMP receiver shutting down");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let tx2      = tx.clone();
                        let cfg2     = cfg.clone();
                        let cancel2  = cancel.clone();
                        let shed2    = shed.clone();
                        let archive2 = Arc::clone(&archive);
                        let dns2     = dns.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(
                                stream, peer, cfg2, cancel2, tx2, shed2, archive2, dns2,
                            ).await {
                                warn!(%peer, error = %e, "BMP connection error");
                            }
                        });
                    }
                    Err(e) => {
                        error!(error = %e, "BMP accept error");
                    }
                }
            }
        }
    }
    Ok(())
}

#[instrument(skip(stream, cfg, cancel, tx, shed, archive, dns), fields(%peer))]
async fn handle_connection(
    mut stream: TcpStream,
    peer:       SocketAddr,
    cfg:        BmpConfig,
    cancel:     CancellationToken,
    tx:         mpsc::Sender<BmpMessage>,
    shed:       ShedSignal,
    archive:    Arc<BmpArchive>,
    dns:        Option<DnsCache>,
) -> Result<()> {
    let speaker_addr = peer.ip();

    // DNS PTR enrichment: resolve hostname for the connecting speaker
    let hostname = if let Some(ref cache) = dns {
        cache.lookup(speaker_addr).await
    } else {
        None
    };
    match &hostname {
        Some(name) => info!(hostname = %name, "BMP speaker connected"),
        None       => info!("BMP speaker connected"),
    }
    let mut buf = BytesMut::with_capacity(65536);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            result = stream.read_buf(&mut buf) => {
                match result {
                    Ok(0) => {
                        info!("BMP speaker disconnected (EOF)");
                        break;
                    }
                    Ok(n) => {
                        debug!(%n, "Read bytes from BMP speaker");
                    }
                    Err(e) => {
                        warn!(error = %e, "BMP read error");
                        break;
                    }
                }
            }
        }

        // Drain complete frames from the buffer
        loop {
            if buf.len() < BMP_HEADER_LEN { break; }
            let frame_len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if buf.len() < frame_len { break; }

            let frame = buf.copy_to_bytes(frame_len);
            match parse_bmp_message(&frame, speaker_addr.into(), cfg.max_frame_bytes) {
                Ok(payload) => {
                    // Under pressure, drop low-value stats messages
                    if cfg.shed_stats_on_pressure
                        && shed.should_shed()
                        && matches!(payload, BmpPayload::StatsReport { .. })
                    {
                        debug!("shedding StatsReport under backpressure");
                        counter!("bmp_messages_shed_total", "speaker" => speaker_addr.to_string()).increment(1);
                        continue;
                    }

                    counter!("bmp_messages_received_total", "speaker" => speaker_addr.to_string()).increment(1);
                    let msg = BmpMessage {
                        id:           uuid::Uuid::new_v4(),
                        received_at:  Utc::now(),
                        speaker_addr: speaker_addr.into(),
                        payload,
                    };

                    // Archive before forwarding (best-effort; errors are non-fatal)
                    if let Err(e) = archive.append(&msg).await {
                        warn!(error = %e, "Archive write failed");
                    }

                    if tx.send(msg).await.is_err() {
                        // Receiver dropped — clean shutdown
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!(error = %e, "BMP parse error — skipping frame");
                    counter!("bmp_parse_errors_total", "speaker" => speaker_addr.to_string()).increment(1);
                }
            }
        }
    }

    // RV2-6: synthesize Termination after TCP drop / EOF so RibManager evicts
    // all stale routes for this speaker. Skip on clean server shutdown where
    // the RIB is being torn down anyway.
    if !cancel.is_cancelled() {
        let synth = BmpMessage {
            id:           uuid::Uuid::new_v4(),
            received_at:  Utc::now(),
            speaker_addr: speaker_addr.into(),
            payload: BmpPayload::Termination {
                reason_code: 0,
                reason_text: Some("tcp-session-dropped".to_string()),
            },
        };
        let _ = tx.send(synth).await;
        info!("Sent synthetic Termination — stale routes evicted for {speaker_addr}");
    }

    Ok(())
}
