use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug, instrument};
use chrono::Utc;
use rbmp_core::bmp::parser::{parse_bmp_message, DEFAULT_MAX_FRAME};
use rbmp_core::bmp::types::BmpMessage;
use crate::config::BmpConfig;

const BMP_HEADER_LEN: usize = 6;

/// Start the BMP TCP receiver. Returns a channel of parsed BmpMessages.
pub async fn run_bmp_receiver(
    cfg:    BmpConfig,
    cancel: CancellationToken,
    tx:     mpsc::Sender<BmpMessage>,
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
                        let tx2    = tx.clone();
                        let cfg2   = cfg.clone();
                        let cancel2 = cancel.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, peer, cfg2, cancel2, tx2).await {
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

#[instrument(skip(stream, cfg, cancel, tx), fields(%peer))]
async fn handle_connection(
    mut stream: TcpStream,
    peer:       SocketAddr,
    cfg:        BmpConfig,
    cancel:     CancellationToken,
    tx:         mpsc::Sender<BmpMessage>,
) -> Result<()> {
    info!("BMP speaker connected");
    let speaker_addr = peer.ip();
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
                    let msg = BmpMessage {
                        id:           uuid::Uuid::new_v4(),
                        received_at:  Utc::now(),
                        speaker_addr: speaker_addr.into(),
                        payload,
                    };
                    if tx.send(msg).await.is_err() {
                        // Receiver dropped (shutdown)
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!(error = %e, "BMP parse error — skipping frame");
                }
            }
        }
    }
    Ok(())
}
