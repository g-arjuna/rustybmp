mod config;
mod receiver;
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
    let rib = Arc::new(RwLock::new(rib_mgr));
    let event_tx = {
        let r = rib.read().await;
        r.subscribe().resubscribe();  // clone the Sender side
        drop(r);
        // Re-subscribe hack: obtain a Sender by wrapping broadcast ourselves
        // We'll use the RibManager's broadcast directly via a channel wrapper
        let (tx, _) = tokio::sync::broadcast::channel::<rbmp_rib::event::RibEvent>(cfg.store.event_capacity);
        tx
    };

    // ── BMP message channel ───────────────────────────────────────────────────
    let (msg_tx, mut msg_rx) = mpsc::channel(4096);
    let cancel = CancellationToken::new();

    // ── BMP Receiver task ─────────────────────────────────────────────────────
    {
        let cancel2 = cancel.clone();
        let bmp_cfg = cfg.bmp.clone();
        tokio::spawn(async move {
            if let Err(e) = receiver::run_bmp_receiver(bmp_cfg, cancel2, msg_tx).await {
                error!(error = %e, "BMP receiver exited with error");
            }
        });
    }

    // ── RIB pump task (BmpMessage → RibManager → events) ─────────────────────
    {
        let rib2      = Arc::clone(&rib);
        let event_tx2 = event_tx.clone();
        let mut store_rx = {
            let r = rib.read().await;
            r.subscribe()
        };
        tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                rib2.write().await.process(msg);
            }
        });
    }

    // ── Store writer task ─────────────────────────────────────────────────────
    {
        let store2 = Arc::clone(&store);
        let store_rx = {
            let r = rib.read().await;
            r.subscribe()
        };
        tokio::spawn(run_store_writer(store2, store_rx));
    }

    // ── HTTP Server ───────────────────────────────────────────────────────────
    let queries = Arc::new(QueryEngine::new(Arc::clone(&store)));
    let state   = AppState {
        rib:     Arc::clone(&rib),
        store,
        queries,
        events:  {
            let r = rib.read().await;
            r.subscribe().resubscribe();
            // TODO: properly wire Sender from RibManager
            event_tx.clone()
        },
    };
    let router = build_router(state);
    let http_addr: std::net::SocketAddr = cfg.http.listen_addr.parse()?;
    info!(addr = %http_addr, "HTTP server starting");
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { cancel.cancelled().await })
        .await?;

    info!("rustybmp shutdown complete");
    Ok(())
}
