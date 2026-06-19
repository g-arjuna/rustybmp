use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use rbmp_core::bmp::types::BmpMessage;

/// Lightweight backpressure shedding signal.
/// When set, low-value messages (StatsReport) are dropped before processing.
#[derive(Clone)]
pub struct ShedSignal(Arc<AtomicBool>);

impl ShedSignal {
    pub fn new() -> Self { Self(Arc::new(AtomicBool::new(false))) }

    pub fn set(&self, val: bool) { self.0.store(val, Ordering::Relaxed); }

    pub fn should_shed(&self) -> bool { self.0.load(Ordering::Relaxed) }
}

impl Default for ShedSignal {
    fn default() -> Self { Self::new() }
}

/// Spawn a background task that monitors the mpsc channel capacity.
/// Sets shed=true when fill > 80%; clears when < 40%.
pub fn spawn_pressure_monitor(
    msg_tx: mpsc::Sender<BmpMessage>,
    signal: ShedSignal,
) {
    tokio::spawn(async move {
        loop {
            let cap   = msg_tx.max_capacity();
            let avail = msg_tx.capacity();
            let used_pct = if cap > 0 { 100 - (avail * 100 / cap) } else { 0 };
            if used_pct > 80 {
                signal.set(true);
            } else if used_pct < 40 {
                signal.set(false);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    });
}
