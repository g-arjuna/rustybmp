use std::net::SocketAddr;
use anyhow::Result;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug};
use metrics::counter;
use chrono::Utc;
use rbmp_core::bmp::parser::parse_bmp_message;
use rbmp_core::bmp::types::{BmpMessage, BmpPayload};

const BMP_HEADER_LEN: usize = 6;

/// Configuration for the BMP proxy/intercept mode.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Address to listen on for incoming BMP connections
    pub listen_addr:     String,
    /// Upstream BMP collector to forward all raw bytes to
    pub upstream_addr:   String,
    /// Max BMP frame size (same limit as receiver)
    pub max_frame_bytes: u32,
}

/// Run the BMP proxy.
///
/// Behaviour:
/// - Listens on `cfg.listen_addr` for routers to connect.
/// - For each router: opens a persistent TCP connection to `cfg.upstream_addr`.
/// - Forwards every raw byte received from the router to the upstream (transparent proxy).
/// - Simultaneously tees parsed BmpMessages to the local `tx` channel so the
///   local RIB/store pipeline also processes the data.
///
/// If the upstream is unavailable the proxy still works — bytes are teed to
/// the local pipeline only, and the upstream connection is retried when the
/// next router reconnects.
pub async fn run_bmp_proxy(
    cfg:    ProxyConfig,
    cancel: CancellationToken,
    tx:     mpsc::Sender<BmpMessage>,
) -> Result<()> {
    let listener = TcpListener::bind(&cfg.listen_addr).await?;
    info!(addr = %cfg.listen_addr, upstream = %cfg.upstream_addr, "BMP proxy listening");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("BMP proxy shutting down");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let cfg2    = cfg.clone();
                        let cancel2 = cancel.clone();
                        let tx2     = tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_proxy_connection(stream, peer, cfg2, cancel2, tx2).await {
                                warn!(%peer, error = %e, "BMP proxy connection error");
                            }
                        });
                    }
                    Err(e) => error!(error = %e, "BMP proxy accept error"),
                }
            }
        }
    }
    Ok(())
}

async fn handle_proxy_connection(
    mut router_stream: TcpStream,
    peer:              SocketAddr,
    cfg:               ProxyConfig,
    cancel:            CancellationToken,
    tx:                mpsc::Sender<BmpMessage>,
) -> Result<()> {
    let speaker_addr = peer.ip();
    info!(%peer, "BMP proxy: router connected");

    // Attempt to connect to upstream (non-fatal if unavailable)
    let upstream_stream = match TcpStream::connect(&cfg.upstream_addr).await {
        Ok(s) => {
            info!(%peer, upstream = %cfg.upstream_addr, "BMP proxy: upstream connected");
            Some(s)
        }
        Err(e) => {
            warn!(%peer, upstream = %cfg.upstream_addr, error = %e,
                "BMP proxy: upstream unavailable — tee-only mode");
            None
        }
    };

    let (mut upstream_write, mut upstream_read_opt) = match upstream_stream {
        Some(s) => {
            let (r, w) = s.into_split();
            (Some(w), Some(r))
        }
        None => (None, None),
    };

    let mut buf = BytesMut::with_capacity(65536);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,

            result = router_stream.read_buf(&mut buf) => {
                match result {
                    Ok(0) => {
                        info!(%peer, "BMP proxy: router disconnected (EOF)");
                        break;
                    }
                    Ok(n) => {
                        debug!(%peer, %n, "BMP proxy: read bytes from router");
                        counter!("bmp_proxy_bytes_received_total",
                            "speaker" => speaker_addr.to_string()).increment(n as u64);

                        // Forward raw bytes to upstream
                        if let Some(ref mut w) = upstream_write {
                            if let Err(e) = w.write_all(&buf[buf.len() - n..]).await {
                                warn!(%peer, error = %e, "BMP proxy: upstream write error — disconnecting upstream");
                                upstream_write = None;
                                upstream_read_opt = None;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(%peer, error = %e, "BMP proxy: router read error");
                        break;
                    }
                }
            }
        }

        // Tee: parse complete BMP frames and send to local pipeline
        loop {
            if buf.len() < BMP_HEADER_LEN { break; }
            let frame_len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if frame_len == 0 || buf.len() < frame_len { break; }

            use bytes::Buf as _;
            let frame = buf.copy_to_bytes(frame_len);

            match parse_bmp_message(&frame, speaker_addr.into(), cfg.max_frame_bytes) {
                Ok(payload) => {
                    counter!("bmp_proxy_messages_teed_total",
                        "speaker" => speaker_addr.to_string()).increment(1);
                    let msg = BmpMessage {
                        id:           uuid::Uuid::new_v4(),
                        received_at:  Utc::now(),
                        speaker_addr: speaker_addr.into(),
                        payload,
                    };
                    if tx.send(msg).await.is_err() {
                        return Ok(()); // local pipeline shut down
                    }
                }
                Err(e) => {
                    warn!(%peer, error = %e, "BMP proxy: parse error — skipping frame");
                    counter!("bmp_proxy_parse_errors_total",
                        "speaker" => speaker_addr.to_string()).increment(1);
                }
            }
        }
    }

    // Drain any remaining upstream responses (fire-and-forget)
    drop(upstream_write);
    drop(upstream_read_opt);

    // Synthesize Termination so local RIB cleans up stale state
    let synth = BmpMessage {
        id:           uuid::Uuid::new_v4(),
        received_at:  Utc::now(),
        speaker_addr: speaker_addr.into(),
        payload:      BmpPayload::Termination {
            reason_code: 0,
            reason_text: Some("proxy-tcp-session-dropped".to_string()),
        },
    };
    let _ = tx.send(synth).await;

    Ok(())
}
