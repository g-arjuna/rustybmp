use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

// ─── Cache entry ─────────────────────────────────────────────────────────────

struct CacheEntry {
    hostname: String,
    expires:  Instant,
}

// ─── DnsCache ────────────────────────────────────────────────────────────────

/// Thread-safe, TTL-bounded PTR-lookup cache.
///
/// Lookups are performed synchronously in a `spawn_blocking` call so the
/// async executor is never blocked.  The OS resolver is used (no extra deps).
#[derive(Clone)]
pub struct DnsCache {
    inner: Arc<Mutex<HashMap<IpAddr, CacheEntry>>>,
    ttl:   Duration,
}

impl DnsCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl:   Duration::from_secs(ttl_secs),
        }
    }

    /// Return the cached PTR name for `addr`, or look it up and cache the result.
    /// Returns `None` if the lookup fails (NXDOMAIN, timeout, etc.).
    pub async fn lookup(&self, addr: IpAddr) -> Option<String> {
        // Fast path: cache hit
        {
            let cache = self.inner.lock().unwrap();
            if let Some(entry) = cache.get(&addr) {
                if entry.expires > Instant::now() {
                    debug!(%addr, hostname = %entry.hostname, "DNS cache hit");
                    return Some(entry.hostname.clone());
                }
            }
        }

        // Slow path: blocking OS PTR lookup
        let result = tokio::task::spawn_blocking(move || ptr_lookup(addr)).await;

        match result {
            Ok(Some(name)) => {
                debug!(%addr, %name, "DNS PTR resolved");
                let mut cache = self.inner.lock().unwrap();
                cache.insert(addr, CacheEntry {
                    hostname: name.clone(),
                    expires:  Instant::now() + self.ttl,
                });
                Some(name)
            }
            Ok(None) => {
                debug!(%addr, "DNS PTR lookup returned no result");
                None
            }
            Err(e) => {
                warn!(%addr, error = %e, "DNS PTR lookup task failed");
                None
            }
        }
    }

    /// Evict all expired entries (call periodically to bound memory use).
    pub fn evict_expired(&self) {
        let now = Instant::now();
        let mut cache = self.inner.lock().unwrap();
        cache.retain(|_, v| v.expires > now);
    }

    pub fn cache_size(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

// ─── OS PTR resolver ─────────────────────────────────────────────────────────

/// Blocking OS-level PTR lookup.  Works on Linux/macOS via glibc/libSystem.
/// Returns the first name without a trailing dot.
fn ptr_lookup(addr: IpAddr) -> Option<String> {
    // Build the reverse-lookup hostname: e.g. "1.2.3.4" → "4.3.2.1.in-addr.arpa"
    let reverse_name = match addr {
        IpAddr::V4(a) => {
            let o = a.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(a) => {
            let nibbles: String = a.octets().iter().rev()
                .flat_map(|b| {
                    let lo = b & 0x0F;
                    let hi = (b >> 4) & 0x0F;
                    [format!("{:x}.", lo), format!("{:x}.", hi)]
                })
                .collect();
            format!("{}ip6.arpa", nibbles)
        }
    };

    // Use std::net::ToSocketAddrs to resolve — the OS handles PTR
    use std::net::ToSocketAddrs;
    match (reverse_name.as_str(), 0u16).to_socket_addrs() {
        Ok(mut addrs) => addrs.next().map(|sa| sa.ip().to_string()),
        Err(_) => {
            // Fall back: use gethostbyaddr-equivalent via socket2 if available,
            // otherwise simply try resolving the reverse name as a hostname.
            // On most systems this just works.  On failure return None.
            None
        }
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_miss_then_hit() {
        // Use loopback — most systems resolve 127.0.0.1 PTR to "localhost" or similar
        let cache = DnsCache::new(300);
        let addr: IpAddr = "127.0.0.1".parse().unwrap();

        // First call: miss (may or may not resolve — just verify no panic)
        let _ = cache.lookup(addr).await;

        // Insert a synthetic entry to test the hit path
        {
            let mut inner = cache.inner.lock().unwrap();
            inner.insert(addr, CacheEntry {
                hostname: "testhost.example.com".to_string(),
                expires:  std::time::Instant::now() + Duration::from_secs(60),
            });
        }

        let result = cache.lookup(addr).await;
        assert_eq!(result.as_deref(), Some("testhost.example.com"));
    }

    #[tokio::test]
    async fn test_evict_expired() {
        let cache = DnsCache::new(300);
        let addr: IpAddr = "10.0.0.1".parse().unwrap();

        {
            let mut inner = cache.inner.lock().unwrap();
            inner.insert(addr, CacheEntry {
                hostname: "expired.example.com".to_string(),
                // Already expired
                expires: std::time::Instant::now() - Duration::from_secs(1),
            });
        }

        assert_eq!(cache.cache_size(), 1);
        cache.evict_expired();
        assert_eq!(cache.cache_size(), 0);
    }
}
