/// HA leader election via Redis SETNX (RV4-7 T1).
///
/// Both instances collect BMP from routers.
/// Only the leader writes to DuckDB and serves the HTTP API.
/// Follower buffers in-memory and takes over within `lease_secs` on leader failure.
///
/// Algorithm:
///   1. Each instance tries `SET ha:leader <instance_id> NX EX <lease_secs>` every lease_secs/2.
///   2. If SET succeeds → this instance is the leader; renew with `EXPIRE ha:leader <lease_secs>`.
///   3. If SET fails → a peer holds the lease; this instance is follower.
///   4. On follower→leader transition, call `on_promote()` callback.
///
/// Integration: callers check `HaLeader::is_leader()` before writing to DuckDB.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{info, warn};

use crate::config::HaConfig;

const HA_KEY: &str = "ha:leader";

/// Shared leader flag — cheap to clone and check.
#[derive(Clone)]
pub struct HaState {
    is_leader: Arc<AtomicBool>,
}

impl HaState {
    pub fn new(initial: bool) -> Self {
        Self { is_leader: Arc::new(AtomicBool::new(initial)) }
    }

    /// Returns true if this instance currently holds the leader lease.
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(Ordering::Relaxed)
    }
}

/// Spawn the HA leader-election background task.
///
/// If HA is disabled (`cfg.enabled = false`), returns an always-leader HaState immediately.
pub async fn run_ha_election(cfg: Arc<HaConfig>) -> HaState {
    if !cfg.enabled {
        info!("HA disabled — this instance is always leader");
        return HaState::new(true);
    }

    // Attempt to connect to Redis
    let client = match redis::Client::open(cfg.redis_url.as_str()) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "HA: failed to connect to Redis — running as standalone leader");
            return HaState::new(true);
        }
    };

    let state    = HaState::new(false);
    let state_bg = state.clone();
    let cfg_bg   = Arc::clone(&cfg);

    tokio::spawn(async move {
        let half = Duration::from_secs(cfg_bg.lease_secs / 2);
        let mut ticker = interval(half);
        loop {
            ticker.tick().await;
            match client.get_async_connection().await {
                Ok(mut conn) => {
                    let result: redis::RedisResult<Option<String>> = redis::cmd("SET")
                        .arg(HA_KEY)
                        .arg(&cfg_bg.instance_id)
                        .arg("NX")
                        .arg("EX")
                        .arg(cfg_bg.lease_secs)
                        .query_async(&mut conn)
                        .await;

                    let acquired = result.ok().flatten().is_some();

                    if acquired {
                        // We got the lease — also try to EXPIRE (renew if already held)
                        let _: redis::RedisResult<bool> = redis::cmd("EXPIRE")
                            .arg(HA_KEY)
                            .arg(cfg_bg.lease_secs)
                            .query_async(&mut conn)
                            .await;
                        if !state_bg.is_leader.swap(true, Ordering::Relaxed) {
                            info!(instance = %cfg_bg.instance_id, "HA: promoted to leader");
                        }
                    } else {
                        // Check if we already hold it (verify value matches our instance_id)
                        let holder: redis::RedisResult<String> = redis::cmd("GET")
                            .arg(HA_KEY)
                            .query_async(&mut conn)
                            .await;
                        let we_hold = holder.ok().as_deref() == Some(&cfg_bg.instance_id);
                        if we_hold {
                            // Renew our existing lease
                            let _: redis::RedisResult<bool> = redis::cmd("EXPIRE")
                                .arg(HA_KEY)
                                .arg(cfg_bg.lease_secs)
                                .query_async(&mut conn)
                                .await;
                            state_bg.is_leader.store(true, Ordering::Relaxed);
                        } else {
                            if state_bg.is_leader.swap(false, Ordering::Relaxed) {
                                warn!(instance = %cfg_bg.instance_id, "HA: demoted to follower");
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "HA: lost Redis connection — retaining current leader state");
                }
            }
        }
    });

    state
}
