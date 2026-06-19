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
use rbmp_enrichment::{VrpCache, EnrichmentEngine};
use rbmp_enrichment::rtr::RtrClient;
use metrics_exporter_prometheus::PrometheusHandle;
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

    // ── Prometheus metrics recorder ───────────────────────────────────────────
    let prom_handle: PrometheusHandle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()?;

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

    // ── RPKI enrichment ───────────────────────────────────────────────────────
    let vrp_cache  = VrpCache::new();
    let enrichment = Arc::new(EnrichmentEngine::new(vrp_cache.clone()));

    // ── BMP message channel ───────────────────────────────────────────────────
    let (msg_tx, mut msg_rx) = mpsc::channel(4096);
    let cancel = CancellationToken::new();

    // Spawn pressure monitor
    governor::spawn_pressure_monitor(msg_tx.clone(), shed_signal.clone());

    // Spawn RTR client when RPKI is enabled
    if cfg.rpki.enabled {
        let rtr_addr = cfg.rpki.rtr_addr.clone();
        let cancel2  = cancel.clone();
        info!(rtr_addr = %rtr_addr, "RPKI RTR client enabled — connecting");
        tokio::spawn(async move {
            RtrClient::new(rtr_addr, vrp_cache).run(cancel2).await;
        });
    } else {
        info!("RPKI RTR client disabled (set [rpki] enabled=true to activate)");
    }

    // ── Metrics gauge updater (every 15s) ────────────────────────────────────
    {
        let rib3        = Arc::clone(&rib);
        let enrichment3 = Arc::clone(&enrichment);
        let cancel3     = cancel.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = cancel3.cancelled() => break,
                    _ = interval.tick() => {
                        let r = rib3.read().await;
                        metrics::gauge!("rustybmp_speakers").set(r.speakers().len() as f64);
                        metrics::gauge!("rustybmp_peers_up").set(r.total_peers_up() as f64);
                        metrics::gauge!("rustybmp_routes_total").set(r.total_routes() as f64);
                        drop(r);
                        metrics::gauge!("rustybmp_vrp_count").set(enrichment3.vrp_count() as f64);
                        metrics::gauge!("rustybmp_rtr_serial").set(enrichment3.rtr_serial() as f64);
                    }
                }
            }
        });
    }

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
    let registry = Arc::new(cfg.registry.clone());
    let state    = AppState {
        rib:        Arc::clone(&rib),
        store,
        queries,
        events:     event_tx,
        enrichment: Arc::clone(&enrichment),
        registry,
        prom:       prom_handle,
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
