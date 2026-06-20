use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc as async_mpsc, RwLock};
use tracing::{error, info, warn};
use rbmp_rib::{FilterEngine, RibManager};

/// Spawn a background task that watches `filter_path` for changes and
/// hot-reloads the filter engine into the `RibManager` on modification.
///
/// Supports both:
///   `.yaml` — YAML DSL (FilterEngine)
///   `.roto` — Roto JIT scripts (RotoFilterEngine, requires `roto-jit` feature)
///
/// The `notify` watcher runs on a dedicated OS thread (not in the async
/// runtime) to avoid `!Send` issues. Events are relayed via a tokio channel
/// into the async task that performs the actual reload.
pub fn spawn_filter_watcher(
    filter_path: PathBuf,
    rib:         Arc<RwLock<RibManager>>,
) -> tokio::task::JoinHandle<()> {
    // Async channel: OS thread → tokio task
    let (async_tx, mut async_rx) = async_mpsc::channel::<()>(8);

    // Dedicated OS thread owns the `RecommendedWatcher` (which is !Send across awaits)
    let watch_path = filter_path.clone();
    std::thread::spawn(move || {
        // std::sync::mpsc for the notify callback → OS thread loop
        let (std_tx, std_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

        let mut watcher = match RecommendedWatcher::new(
            move |res| { let _ = std_tx.send(res); },
            notify::Config::default().with_poll_interval(Duration::from_secs(2)),
        ) {
            Ok(w) => w,
            Err(e) => { error!("filter watcher init failed: {e}"); return; }
        };

        if let Err(e) = watcher.watch(&watch_path, RecursiveMode::NonRecursive) {
            error!(path = %watch_path.display(), "filter watcher watch() failed: {e}");
            return;
        }
        info!(path = %watch_path.display(), "filter hot-reload watcher started");

        // Signal the async side to do an initial load
        let _ = async_tx.blocking_send(());

        for event_res in std_rx {
            match event_res {
                Ok(event) if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) => {
                    let _ = async_tx.blocking_send(());
                }
                Ok(_) => {}
                Err(e) => warn!("filter watcher event error: {e}"),
            }
        }
    });

    // Async task: receives reload signals and applies them
    tokio::spawn(async move {
        while async_rx.recv().await.is_some() {
            let is_roto = filter_path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "roto")
                .unwrap_or(false);

            if is_roto {
                reload_roto_filter(&filter_path, &rib).await;
            } else {
                reload_yaml_filter(&filter_path, &rib).await;
            }
        }
    })
}

async fn reload_yaml_filter(path: &PathBuf, rib: &Arc<RwLock<RibManager>>) {
    // Phase 1 — synchronous: load and convert result to a Send-safe type
    // before any await point so Box<dyn Error> (!Send) never crosses an await.
    let loaded: Result<FilterEngine, String> =
        FilterEngine::load_file(path.to_str().unwrap_or(""))
            .map_err(|e| e.to_string());

    // Phase 2 — async: only FilterEngine (Send) + Arc cross this await
    match loaded {
        Ok(engine) => {
            let n = engine.len();
            rib.write().await.set_filter(engine);
            info!(path = %path.display(), filters = n, "YAML filter engine reloaded");
        }
        Err(msg) => {
            error!(path = %path.display(), err = %msg,
                "YAML filter reload failed — keeping previous filter engine");
        }
    }
}

async fn reload_roto_filter(path: &PathBuf, rib: &Arc<RwLock<RibManager>>) {
    use rbmp_rib::RotoFilterEngine;

    let path_str = path.to_str().unwrap_or("").to_string();

    // Phase 1 — synchronous compilation (CPU-intensive, keep off async executor)
    let loaded: Result<RotoFilterEngine, String> =
        RotoFilterEngine::load(&path_str).map_err(|e| e.to_string());

    match loaded {
        Ok(engine) => {
            rib.write().await.set_roto_filter(engine);
            info!(path = %path.display(), "Roto filter engine hot-reloaded");
        }
        Err(msg) => {
            error!(path = %path.display(), err = %msg,
                "Roto filter reload failed — keeping previous engine");
        }
    }
}
