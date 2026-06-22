use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};
use rbmp_core::bmp::types::BmpMessage;
use crate::config::GovernorConfig;

/// Snapshot returned by GET /api/governance
#[derive(Debug, Clone, serde::Serialize)]
pub struct GovernanceSnapshot {
    pub profile:               String,
    pub memory_budget_mb:      u64,
    pub rate_budget_eps:       u64,
    pub memory_pressure_active: bool,
    pub write_pressure_active:  bool,
    pub rate_shedding_active:   bool,
    pub memory_shrink_count:    u64,
    pub write_batch_expand_count: u64,
    pub rate_shed_count:        u64,
}

/// Three-loop resource governor (RV8-GOV1).
///
/// Loop 1 — Memory pressure (5s poll): soft 80% / hard 95% of `memory_budget_bytes`.
/// Loop 2 — Write pressure (channel fill): >50% sustained 60s → expand batch.
/// Loop 3 — Rate governance: events/sec vs `rate_budget_eps` → rate shedding.
#[derive(Clone)]
pub struct ResourceGovernor {
    cfg: GovernorConfig,
    memory_pressure_active:   Arc<AtomicBool>,
    write_pressure_active:    Arc<AtomicBool>,
    rate_shedding_active:     Arc<AtomicBool>,
    memory_shrink_count:      Arc<AtomicU64>,
    write_batch_expand_count: Arc<AtomicU64>,
    rate_shed_count:          Arc<AtomicU64>,
    /// Inbound event counter — incremented per BMP message processed
    inbound_event_counter:    Arc<AtomicU64>,
}

impl ResourceGovernor {
    pub fn new(cfg: GovernorConfig) -> Self {
        Self {
            cfg,
            memory_pressure_active:   Arc::new(AtomicBool::new(false)),
            write_pressure_active:    Arc::new(AtomicBool::new(false)),
            rate_shedding_active:     Arc::new(AtomicBool::new(false)),
            memory_shrink_count:      Arc::new(AtomicU64::new(0)),
            write_batch_expand_count: Arc::new(AtomicU64::new(0)),
            rate_shed_count:          Arc::new(AtomicU64::new(0)),
            inbound_event_counter:    Arc::new(AtomicU64::new(0)),
        }
    }

    /// Call this for every BMP message entering the pipeline.
    pub fn record_event(&self) {
        self.inbound_event_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns true if this message should be dropped under rate pressure.
    pub fn should_shed(&self) -> bool {
        self.rate_shedding_active.load(Ordering::Relaxed)
    }

    /// Returns true if memory pressure is active (soft or hard threshold).
    pub fn memory_pressure(&self) -> bool {
        self.memory_pressure_active.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self, profile: &str) -> GovernanceSnapshot {
        GovernanceSnapshot {
            profile:                  profile.to_string(),
            memory_budget_mb:         self.cfg.memory_budget_mb,
            rate_budget_eps:          self.cfg.rate_budget_eps,
            memory_pressure_active:   self.memory_pressure_active.load(Ordering::Relaxed),
            write_pressure_active:    self.write_pressure_active.load(Ordering::Relaxed),
            rate_shedding_active:     self.rate_shedding_active.load(Ordering::Relaxed),
            memory_shrink_count:      self.memory_shrink_count.load(Ordering::Relaxed),
            write_batch_expand_count: self.write_batch_expand_count.load(Ordering::Relaxed),
            rate_shed_count:          self.rate_shed_count.load(Ordering::Relaxed),
        }
    }

    /// Spawn all three background loops. Call once at startup.
    pub fn spawn_loops(&self, msg_tx: mpsc::Sender<BmpMessage>) {
        self.spawn_memory_loop();
        self.spawn_write_loop(msg_tx);
        self.spawn_rate_loop();
    }

    // ── Loop 1: Memory pressure ───────────────────────────────────────────────

    fn spawn_memory_loop(&self) {
        let budget_bytes  = self.cfg.memory_budget_mb.saturating_mul(1024 * 1024);
        let active        = Arc::clone(&self.memory_pressure_active);
        let shrink_count  = Arc::clone(&self.memory_shrink_count);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let used = Self::resident_bytes();
                let pct  = if budget_bytes > 0 { (used * 100) / budget_bytes } else { 0 };

                if pct >= 95 {
                    if !active.load(Ordering::Relaxed) {
                        warn!(used_mb = used / (1024*1024), budget_mb = budget_bytes / (1024*1024),
                              "Governor: hard memory threshold (95%) — aggressive shrink");
                    }
                    active.store(true, Ordering::Relaxed);
                    shrink_count.fetch_add(1, Ordering::Relaxed);
                    metrics::counter!("rustybmp_governor_action_total",
                        "action" => "memory_shrink").increment(1);
                } else if pct >= 80 {
                    if !active.load(Ordering::Relaxed) {
                        info!(used_mb = used / (1024*1024),
                              "Governor: soft memory threshold (80%)");
                    }
                    active.store(true, Ordering::Relaxed);
                    shrink_count.fetch_add(1, Ordering::Relaxed);
                    metrics::counter!("rustybmp_governor_action_total",
                        "action" => "memory_shrink").increment(1);
                } else {
                    active.store(false, Ordering::Relaxed);
                }
            }
        });
    }

    fn resident_bytes() -> u64 {
        // Read RSS from /proc/self/status on Linux; fall back to 0 on macOS.
        #[cfg(target_os = "linux")]
        {
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        let kb: u64 = line.split_whitespace()
                            .nth(1).unwrap_or("0").parse().unwrap_or(0);
                        return kb * 1024;
                    }
                }
            }
        }
        0u64
    }

    // ── Loop 2: Write pressure ────────────────────────────────────────────────

    fn spawn_write_loop(&self, msg_tx: mpsc::Sender<BmpMessage>) {
        let active       = Arc::clone(&self.write_pressure_active);
        let expand_count = Arc::clone(&self.write_batch_expand_count);

        tokio::spawn(async move {
            let mut interval  = tokio::time::interval(tokio::time::Duration::from_secs(1));
            let mut over_half = 0u32; // seconds sustained above 50% fill

            loop {
                interval.tick().await;
                let cap  = msg_tx.max_capacity();
                let avail = msg_tx.capacity();
                let used_pct = if cap > 0 { 100usize.saturating_sub(avail * 100 / cap) } else { 0 };

                if used_pct > 50 {
                    over_half = over_half.saturating_add(1);
                    if over_half >= 60 {
                        // Sustained >50% for 60s — expand batch
                        if !active.load(Ordering::Relaxed) {
                            warn!(used_pct, "Governor: write pressure sustained 60s — expanding batch");
                            active.store(true, Ordering::Relaxed);
                            expand_count.fetch_add(1, Ordering::Relaxed);
                            metrics::counter!("rustybmp_governor_action_total",
                                "action" => "write_expand").increment(1);
                        }
                    }
                } else {
                    over_half = 0;
                    active.store(false, Ordering::Relaxed);
                }
            }
        });
    }

    // ── Loop 3: Rate governance ───────────────────────────────────────────────

    fn spawn_rate_loop(&self) {
        let budget        = self.cfg.rate_budget_eps;
        let shedding      = Arc::clone(&self.rate_shedding_active);
        let shed_count    = Arc::clone(&self.rate_shed_count);
        let event_counter = Arc::clone(&self.inbound_event_counter);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            let mut last     = 0u64;

            loop {
                interval.tick().await;
                let current = event_counter.load(Ordering::Relaxed);
                let eps     = current.saturating_sub(last);
                last        = current;

                if eps > budget {
                    if !shedding.load(Ordering::Relaxed) {
                        warn!(eps, budget, "Governor: rate budget exceeded — shedding low-priority messages");
                        shedding.store(true, Ordering::Relaxed);
                    }
                    shed_count.fetch_add(1, Ordering::Relaxed);
                    metrics::counter!("rustybmp_governor_action_total",
                        "action" => "rate_shed").increment(1);
                } else {
                    shedding.store(false, Ordering::Relaxed);
                }
            }
        });
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_governor(rate_budget_eps: u64) -> ResourceGovernor {
        ResourceGovernor::new(GovernorConfig {
            memory_budget_mb: 4096,
            rate_budget_eps,
            sheddable_types: vec![],
        })
    }

    #[test]
    fn initial_state_no_pressure() {
        let g = make_governor(500_000);
        assert!(!g.should_shed(),       "new governor should not shed");
        assert!(!g.memory_pressure(),   "new governor has no memory pressure");
        let snap = g.snapshot("default");
        assert!(!snap.memory_pressure_active);
        assert!(!snap.write_pressure_active);
        assert!(!snap.rate_shedding_active);
    }

    #[test]
    fn record_event_increments_counter() {
        let g = make_governor(500_000);
        for _ in 0..10 {
            g.record_event();
        }
        assert_eq!(
            g.inbound_event_counter.load(Ordering::Relaxed),
            10,
            "inbound_event_counter should be 10 after 10 record_event() calls"
        );
    }

    #[test]
    fn snapshot_reflects_cfg() {
        let g = make_governor(12345);
        let snap = g.snapshot("internet");
        assert_eq!(snap.profile, "internet");
        assert_eq!(snap.memory_budget_mb, 4096);
        assert_eq!(snap.rate_budget_eps, 12345);
        assert_eq!(snap.memory_shrink_count, 0);
        assert_eq!(snap.write_batch_expand_count, 0);
        assert_eq!(snap.rate_shed_count, 0);
    }

    #[test]
    fn manual_rate_shedding_flag() {
        let g = make_governor(500_000);
        assert!(!g.should_shed());
        g.rate_shedding_active.store(true, Ordering::Relaxed);
        assert!(g.should_shed(), "should_shed() must reflect the atomic flag");
        g.rate_shedding_active.store(false, Ordering::Relaxed);
        assert!(!g.should_shed());
    }

    #[test]
    fn manual_memory_pressure_flag() {
        let g = make_governor(500_000);
        assert!(!g.memory_pressure());
        g.memory_pressure_active.store(true, Ordering::Relaxed);
        assert!(g.memory_pressure());
        g.memory_pressure_active.store(false, Ordering::Relaxed);
        assert!(!g.memory_pressure());
    }

    #[test]
    fn record_many_events_counter_monotonic() {
        let g = make_governor(500_000);
        for i in 1..=100 {
            g.record_event();
            let count = g.inbound_event_counter.load(Ordering::Relaxed);
            assert_eq!(count, i, "counter must be monotonically increasing");
        }
    }

    #[test]
    fn write_pressure_flag_roundtrip() {
        let g = make_governor(500_000);
        let snap1 = g.snapshot("default");
        assert!(!snap1.write_pressure_active);
        g.write_pressure_active.store(true, Ordering::Relaxed);
        g.write_batch_expand_count.fetch_add(1, Ordering::Relaxed);
        let snap2 = g.snapshot("default");
        assert!(snap2.write_pressure_active);
        assert_eq!(snap2.write_batch_expand_count, 1);
    }

    #[test]
    fn rate_shed_count_increments_independently() {
        let g = make_governor(500_000);
        g.rate_shed_count.fetch_add(5, Ordering::Relaxed);
        let snap = g.snapshot("test");
        assert_eq!(snap.rate_shed_count, 5);
        assert_eq!(snap.memory_shrink_count, 0);
        assert_eq!(snap.write_batch_expand_count, 0);
    }

    #[test]
    fn memory_shrink_count_increments_independently() {
        let g = make_governor(500_000);
        g.memory_shrink_count.fetch_add(3, Ordering::Relaxed);
        let snap = g.snapshot("test");
        assert_eq!(snap.memory_shrink_count, 3);
        assert_eq!(snap.rate_shed_count, 0);
    }

    #[test]
    fn different_rate_budgets_reflected_in_snapshot() {
        let g1 = make_governor(100_000);
        let g2 = make_governor(999_999);
        assert_eq!(g1.snapshot("a").rate_budget_eps, 100_000);
        assert_eq!(g2.snapshot("b").rate_budget_eps, 999_999);
    }

    #[test]
    fn clone_shares_atomic_state() {
        let g1 = make_governor(500_000);
        let g2 = g1.clone();
        g1.record_event();
        g1.record_event();
        // Clone shares Arc<AtomicU64> so g2 sees same counter
        assert_eq!(g2.inbound_event_counter.load(Ordering::Relaxed), 2,
            "cloned governor must share atomic event counter via Arc");
    }
}
