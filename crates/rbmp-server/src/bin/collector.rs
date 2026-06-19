//! `rbmp-collector` — edge BMP collector (RV3-10).
//!
//! Listens for BMP speakers on a configurable port (default 5000), wraps each
//! PDU in a `CollectorEnvelope`, and forwards it to a Core instance over TCP
//! (default port 5001) using the length-prefixed MessagePack protocol.
//!
//! Reconnects to Core with exponential back-off.  When Core is unreachable,
//! incoming BMP PDUs are dropped after the in-memory ring buffer is full
//! (configurable `ring_capacity`, default 10 000 messages).
//!
//! # Usage
//!
//! ```text
//! rbmp-collector \
//!     --listen  0.0.0.0:5000 \
//!     --core    core.example.com:5001 \
//!     --id      fra01-collector-1 \
//!     --site    fra01
//! ```

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn, debug};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use rbmp_core::bmp::parser::parse_bmp_message;
use rbmp_core::collector_protocol::{CollectorEnvelope, COLLECTOR_PORT, write_frame};

// ─── CLI / config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CollectorCfg {
    listen_addr:    SocketAddr,
    core_addr:      SocketAddr,
    collector_id:   String,
    site:           String,
    ring_capacity:  usize,
    max_backoff_ms: u64,
}

impl Default for CollectorCfg {
    fn default() -> Self {
        Self {
            listen_addr:    "0.0.0.0:5000".parse().unwrap(),
            core_addr:      format!("127.0.0.1:{COLLECTOR_PORT}").parse().unwrap(),
            collector_id:   hostname(),
            site:           "default".into(),
            ring_capacity:  10_000,
            max_backoff_ms: 30_000,
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| {
            std::process::Command::new("hostname")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_else(|_| "unknown-collector".into())
}

fn parse_args() -> CollectorCfg {
    let mut cfg = CollectorCfg::default();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" | "-l" => { i += 1; cfg.listen_addr = args[i].parse().expect("invalid listen addr"); }
            "--core"   | "-c" => { i += 1; cfg.core_addr   = args[i].parse().expect("invalid core addr"); }
            "--id"     | "-i" => { i += 1; cfg.collector_id = args[i].clone(); }
            "--site"   | "-s" => { i += 1; cfg.site         = args[i].clone(); }
            "--ring"          => { i += 1; cfg.ring_capacity = args[i].parse().expect("invalid ring size"); }
            _ => {}
        }
        i += 1;
    }
    cfg
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_target(true))
        .init();

    let cfg = Arc::new(parse_args());

    info!(
        listen = %cfg.listen_addr,
        core   = %cfg.core_addr,
        id     = %cfg.collector_id,
        site   = %cfg.site,
        "rbmp-collector starting"
    );

    let cancel = CancellationToken::new();

    // ── Ring buffer channel ───────────────────────────────────────────────────
    let (tx, rx) = mpsc::channel::<CollectorEnvelope>(cfg.ring_capacity);

    // ── Forwarder task (ring buffer consumer → Core TCP) ─────────────────────
    {
        let cfg2   = cfg.clone();
        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            run_forwarder(rx, cfg2, cancel2).await;
        });
    }

    // ── BMP listener ─────────────────────────────────────────────────────────
    let listener = TcpListener::bind(cfg.listen_addr).await
        .context("bind BMP listener")?;
    info!("BMP listener ready on {}", cfg.listen_addr);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let tx2    = tx.clone();
                        let cfg3   = cfg.clone();
                        let cancel3 = cancel.clone();
                        tokio::spawn(async move {
                            handle_speaker(stream, peer, tx2, cfg3, cancel3).await;
                        });
                    }
                    Err(e) => { error!(error = %e, "BMP accept error"); }
                }
            }
        }
    }
    Ok(())
}

// ─── Speaker handler ──────────────────────────────────────────────────────────

async fn handle_speaker(
    stream:  TcpStream,
    peer:    SocketAddr,
    tx:      mpsc::Sender<CollectorEnvelope>,
    cfg:     Arc<CollectorCfg>,
    cancel:  CancellationToken,
) {
    info!(speaker = %peer, "BMP speaker connected");
    let speaker_addr: IpAddr = peer.ip();
    let mut reader = tokio::io::BufReader::new(stream);

    loop {
        // Read BMP common header (6 bytes) to get total message length
        use tokio::io::AsyncReadExt;
        let mut hdr = [0u8; 6];
        tokio::select! {
            _ = cancel.cancelled() => break,
            result = reader.read_exact(&mut hdr) => {
                match result {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        info!(speaker = %peer, "BMP speaker disconnected");
                        break;
                    }
                    Err(e) => {
                        warn!(speaker = %peer, error = %e, "BMP read error");
                        break;
                    }
                }
            }
        }

        // BMP header: version(1) + length(4) + type(1)
        let total_len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
        if total_len < 6 || total_len > 8 * 1024 * 1024 {
            warn!(speaker = %peer, total_len, "BMP message length out of range");
            break;
        }

        let remaining = total_len - 6;
        let mut body = vec![0u8; remaining];
        if let Err(e) = reader.read_exact(&mut body).await {
            warn!(speaker = %peer, error = %e, "BMP body read error");
            break;
        }

        let mut raw_bmp = Vec::with_capacity(total_len);
        raw_bmp.extend_from_slice(&hdr);
        raw_bmp.extend_from_slice(&body);

        let env = CollectorEnvelope {
            collector_id: cfg.collector_id.clone(),
            site:         cfg.site.clone(),
            speaker_addr,
            received_at:  chrono::Utc::now(),
            raw_bmp,
        };

        if tx.try_send(env).is_err() {
            warn!(speaker = %peer, "ring buffer full — BMP PDU dropped");
        }
    }
}

// ─── Forwarder ────────────────────────────────────────────────────────────────

async fn run_forwarder(
    mut rx:     mpsc::Receiver<CollectorEnvelope>,
    cfg:        Arc<CollectorCfg>,
    cancel:     CancellationToken,
) {
    let mut backoff_ms: u64 = 100;

    'outer: loop {
        if cancel.is_cancelled() { break; }

        // Connect to Core
        let stream = loop {
            tokio::select! {
                _ = cancel.cancelled() => break 'outer,
                result = TcpStream::connect(cfg.core_addr) => {
                    match result {
                        Ok(s) => {
                            info!(core = %cfg.core_addr, "Connected to Core");
                            backoff_ms = 100; // reset
                            break s;
                        }
                        Err(e) => {
                            warn!(core = %cfg.core_addr, error = %e,
                                  backoff_ms, "Core connection failed, retrying");
                            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                            backoff_ms = (backoff_ms * 2).min(cfg.max_backoff_ms);
                        }
                    }
                }
            }
        };

        let mut writer = tokio::io::BufWriter::new(stream);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break 'outer,
                maybe_env = rx.recv() => {
                    let env = match maybe_env {
                        Some(e) => e,
                        None    => break 'outer, // channel closed
                    };
                    if let Err(e) = write_frame(&mut writer, &env).await {
                        warn!(error = %e, "Core write failed — reconnecting");
                        break; // reconnect
                    }
                    debug!(speaker = %env.speaker_addr, "PDU forwarded to Core");
                }
            }
        }
    }
    info!("Forwarder stopped");
}
