mod config;
mod receiver;
mod archive;
mod governor;
mod api;

use std::sync::Arc;
use std::path::Path;
use anyhow::Result;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{info, error};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use rbmp_rib::RibManager;
use rbmp_store::{RouteStore, writer::run_store_writer};
use rbmp_store::query::QueryEngine;
use api::{AppState, build_router};

#[tokio::main]
async fn main() -> Result<()> {
    // ── Config ────────────────────────────────────────────────────────────────
    let cfg_path = std::env::args().nth(1).unwrap_or_else(|| "rustybmp.toml".to_string());
    let cfg = if Path::new(&cfg_path).exists() {
        config::Config::from_file(&cfg_path)?
    } else {
        config::Config::default_config()
    };

    // ── Logging ───────────────────────────────────────────────────────────────
    let level = if cfg.log.level.is_empty() { "info" } else { &cfg.log.level };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "rustybmp starting");

    // ── Store ─────────────────────────────────────────────────────────────────
    let store = if cfg.store.in_memory {
        RouteStore::in_memory()?
    } else {
        if let Some(parent) = Path::new(&cfg.store.db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        RouteStore::open(&cfg.store.db_path)?
    };
    let store = Arc::new(std::sync::Mutex::new(store));

    // ── RIB Manager ──────────────────────────────────────────────────────────
    let (rib_mgr, rib_rx) = RibManager::new(cfg.store.event_capacity);
    // Capture the real Sender BEFORE moving rib_mgr into Arc<RwLock>
    let event_tx = rib_mgr.event_sender();
    let rib = Arc::new(RwLock::new(rib_mgr));

    // ── Archive writer ────────────────────────────────────────────────────────
    let archive = Arc::new(
        archive::BmpArchive::open(cfg.bmp.archive_path.as_deref()).await?
    );

    // ── Back-pressure governor ────────────────────────────────────────────────
    let shed_signal = governor::ShedSignal::new();

    // ── BMP message channel ───────────────────────────────────────────────────
    let (msg_tx, mut msg_rx) = mpsc::channel(4096);
    let cancel = CancellationToken::new();

    // Spawn pressure monitor
    governor::spawn_pressure_monitor(msg_tx.clone(), shed_signal.clone());

    // ── BMP Receiver task ─────────────────────────────────────────────────────
    {
        let cancel2     = cancel.clone();
        let bmp_cfg     = cfg.bmp.clone();
        let shed2       = shed_signal.clone();
        let archive2    = Arc::clone(&archive);
        let msg_tx2     = msg_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = receiver::run_bmp_receiver(bmp_cfg, cancel2, msg_tx2, shed2, archive2).await {
                error!(error = %e, "BMP receiver exited with error");
            }
        });
    }

    // ── RIB pump task (BmpMessage → RibManager → events) ─────────────────────
    {
        let rib2 = Arc::clone(&rib);
        tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                rib2.write().await.process(msg);
            }
        });
    }

    // ── Store writer task ─────────────────────────────────────────────────────
    {
        let store2 = Arc::clone(&store);
        tokio::spawn(run_store_writer(store2, rib_rx));
    }

    // ── DuckDB checkpoint task ────────────────────────────────────────────────
    if cfg.store.checkpoint_secs > 0 {
        let store3 = Arc::clone(&store);
        let secs   = cfg.store.checkpoint_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(secs)
            );
            interval.tick().await; // first tick immediate — skip it
            loop {
                interval.tick().await;
                if let Ok(s) = store3.lock() {
                    if let Err(e) = s.checkpoint() {
                        tracing::warn!(error = %e, "DuckDB checkpoint failed");
                    }
                }
            }
        });
    }

    // ── HTTP Server ───────────────────────────────────────────────────────────
    let queries = Arc::new(QueryEngine::new(Arc::clone(&store)));
    let state   = AppState {
        rib:    Arc::clone(&rib),
        store,
        queries,
        events: event_tx,
    };
    let router  = build_router(state);
    let http_addr: std::net::SocketAddr = cfg.http.listen_addr.parse()?;
    info!(addr = %http_addr, "HTTP server starting");
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { cancel.cancelled().await })
        .await?;

    info!("rustybmp shutdown complete");
    Ok(())
}
