/// DuckDB retention policy — delete events older than N days (RV4-2 T1).
///
/// Runs as a background tokio task. Triggered every `sweep_interval_secs`.
/// Deletes from route_events, peer_events, speaker_events, stats_events, evpn_events.
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use rbmp_store::RouteStore;

const SWEEP_INTERVAL_SECS: u64 = 3600; // sweep once per hour

/// Spawn the retention background task.
/// retain_days = 0 means keep forever (no-op).
pub async fn run_retention_sweep(
    store:       Arc<Mutex<RouteStore>>,
    retain_days: u32,
) {
    if retain_days == 0 {
        info!("DuckDB retention: disabled (retain_days = 0, keeping all history)");
        return;
    }

    info!(retain_days, "DuckDB retention sweep enabled");
    let mut interval = tokio::time::interval(
        tokio::time::Duration::from_secs(SWEEP_INTERVAL_SECS)
    );
    interval.tick().await; // first tick is immediate — skip to avoid sweep on startup

    loop {
        interval.tick().await;
        match store.lock() {
            Ok(s) => {
                let deleted = sweep(s.conn(), retain_days);
                match deleted {
                    Ok(n) => info!(deleted_rows = n, "DuckDB retention sweep complete"),
                    Err(e) => warn!(error = %e, "DuckDB retention sweep failed"),
                }
            }
            Err(e) => warn!(error = %e, "retention: failed to lock store"),
        }
    }
}

/// Execute the DELETE statements. Returns total rows deleted.
fn sweep(conn: &duckdb::Connection, retain_days: u32) -> Result<usize, duckdb::Error> {
    let tables = [
        "route_events",
        "peer_events",
        "speaker_events",
        "stats_events",
        "evpn_events",
    ];
    let mut total = 0usize;
    for table in &tables {
        let sql = format!(
            "DELETE FROM {table} WHERE occurred_at < CAST(NOW() AS TIMESTAMP) - INTERVAL '{retain_days}' DAY"
        );
        let n = conn.execute(&sql, [])?;
        total += n;
    }
    Ok(total)
}
