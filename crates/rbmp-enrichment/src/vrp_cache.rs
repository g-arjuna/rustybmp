use std::sync::{Arc, RwLock};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

/// A single Validated ROA Payload entry from the RPKI RTR feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VrpEntry {
    pub prefix:     IpNet,
    pub max_len:    u8,
    pub origin_asn: u32,
}

/// RPKI validation state for a prefix+origin pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpkiState {
    /// Prefix+origin matches a ROA and is within max-length.
    Valid,
    /// A ROA exists for the prefix (or a covering prefix) but origin or length is wrong.
    Invalid,
    /// No ROA found covering this prefix.
    NotFound,
}

impl std::fmt::Display for RpkiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid    => write!(f, "valid"),
            Self::Invalid  => write!(f, "invalid"),
            Self::NotFound => write!(f, "not-found"),
        }
    }
}

/// Thread-safe, live-updatable VRP cache.
/// Updated by the RTR client; queried by the enrichment engine per route.
#[derive(Clone, Default)]
pub struct VrpCache {
    inner:  Arc<RwLock<VrpCacheInner>>,
}

#[derive(Default)]
struct VrpCacheInner {
    entries: Vec<VrpEntry>,
    serial:  u32,
}

impl VrpCache {
    pub fn new() -> Self { Self::default() }

    /// Replace the full VRP table (called on cache-reset from RTR).
    pub fn reset(&self, entries: Vec<VrpEntry>, serial: u32) {
        let mut w = self.inner.write().unwrap();
        w.entries = entries;
        w.serial  = serial;
    }

    /// Apply incremental add/remove deltas from RTR serial-notify.
    pub fn apply_delta(&self, adds: Vec<VrpEntry>, removes: Vec<VrpEntry>, serial: u32) {
        let mut w = self.inner.write().unwrap();
        for r in &removes {
            w.entries.retain(|e| !(e.prefix == r.prefix && e.origin_asn == r.origin_asn));
        }
        w.entries.extend(adds);
        w.serial = serial;
    }

    pub fn serial(&self) -> u32 {
        self.inner.read().unwrap().serial
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().entries.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Validate a (prefix, origin_asn) pair against the VRP table.
    ///
    /// Algorithm (RFC 6811):
    /// 1. Collect all VRPs whose prefix covers the announced prefix.
    /// 2. If none → NotFound.
    /// 3. Among covering VRPs, if any matches origin AND max_len ≥ announced prefix_len → Valid.
    /// 4. Otherwise → Invalid.
    pub fn validate(&self, announced: IpNet, origin_asn: u32) -> RpkiState {
        let r = self.inner.read().unwrap();
        let ann_len = announced.prefix_len();

        let covering: Vec<&VrpEntry> = r.entries.iter()
            .filter(|e| covers(&e.prefix, &announced))
            .collect();

        if covering.is_empty() {
            return RpkiState::NotFound;
        }

        let valid = covering.iter().any(|e| {
            e.origin_asn == origin_asn && e.max_len >= ann_len
        });

        if valid { RpkiState::Valid } else { RpkiState::Invalid }
    }
}

/// Returns true when `roa_prefix` covers `announced` (announced is same or more-specific).
fn covers(roa_prefix: &IpNet, announced: &IpNet) -> bool {
    match (roa_prefix, announced) {
        (IpNet::V4(r), IpNet::V4(a)) => r.contains(a) || r == a,
        (IpNet::V6(r), IpNet::V6(a)) => r.contains(a) || r == a,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn net(s: &str) -> IpNet { IpNet::from_str(s).unwrap() }

    #[test]
    fn test_valid_exact() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("192.0.2.0/24"), max_len: 24, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("192.0.2.0/24"), 64496), RpkiState::Valid);
    }

    #[test]
    fn test_valid_more_specific_within_maxlen() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("192.0.2.0/24"), max_len: 25, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("192.0.2.0/25"), 64496), RpkiState::Valid);
    }

    #[test]
    fn test_invalid_wrong_origin() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("192.0.2.0/24"), max_len: 24, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("192.0.2.0/24"), 64500), RpkiState::Invalid);
    }

    #[test]
    fn test_invalid_exceeds_maxlen() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("192.0.2.0/24"), max_len: 24, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("192.0.2.0/25"), 64496), RpkiState::Invalid);
    }

    #[test]
    fn test_not_found() {
        let cache = VrpCache::new();
        cache.reset(vec![], 1);
        assert_eq!(cache.validate(net("198.51.100.0/24"), 64497), RpkiState::NotFound);
    }

    #[test]
    fn test_delta_add_remove() {
        let cache = VrpCache::new();
        let vrp = VrpEntry { prefix: net("192.0.2.0/24"), max_len: 24, origin_asn: 64496 };
        cache.reset(vec![vrp.clone()], 1);
        assert_eq!(cache.len(), 1);
        cache.apply_delta(vec![], vec![vrp], 2);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.validate(net("192.0.2.0/24"), 64496), RpkiState::NotFound);
    }

    #[test]
    fn test_ipv6_valid() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("2001:db8::/32"), max_len: 48, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("2001:db8:1::/48"), 64496), RpkiState::Valid);
    }

    #[test]
    fn test_ipv6_invalid_wrong_origin() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("2001:db8::/32"), max_len: 48, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("2001:db8:1::/48"), 64497), RpkiState::Invalid);
    }

    #[test]
    fn test_ipv6_exceeds_maxlen() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("2001:db8::/32"), max_len: 40, origin_asn: 64496 }], 1);
        assert_eq!(cache.validate(net("2001:db8:1::/48"), 64496), RpkiState::Invalid);
    }

    #[test]
    fn test_delta_add() {
        let cache = VrpCache::new();
        cache.reset(vec![], 1);
        cache.apply_delta(
            vec![VrpEntry { prefix: net("203.0.113.0/24"), max_len: 24, origin_asn: 64497 }],
            vec![],
            2,
        );
        assert_eq!(cache.serial(), 2);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.validate(net("203.0.113.0/24"), 64497), RpkiState::Valid);
    }

    #[test]
    fn test_multi_roa_best_match() {
        let cache = VrpCache::new();
        cache.reset(vec![
            VrpEntry { prefix: net("10.0.0.0/8"),  max_len: 24, origin_asn: 64496 },
            VrpEntry { prefix: net("10.1.0.0/16"), max_len: 24, origin_asn: 64497 },
        ], 1);
        assert_eq!(cache.validate(net("10.1.0.0/24"), 64497), RpkiState::Valid);
        assert_eq!(cache.validate(net("10.1.0.0/24"), 64496), RpkiState::Valid);
    }

    #[test]
    fn test_serial_initial_zero() {
        let cache = VrpCache::new();
        assert_eq!(cache.serial(), 0);
    }

    #[test]
    fn test_serial_after_reset() {
        let cache = VrpCache::new();
        cache.reset(vec![], 42);
        assert_eq!(cache.serial(), 42);
    }

    #[test]
    fn test_empty_cache_not_found() {
        let cache = VrpCache::new();
        assert_eq!(cache.validate(net("8.8.8.0/24"), 15169), RpkiState::NotFound);
    }

    #[test]
    fn test_is_empty_then_reset() {
        let cache = VrpCache::new();
        assert!(cache.is_empty());
        cache.reset(vec![VrpEntry { prefix: net("192.0.2.0/24"), max_len: 24, origin_asn: 1 }], 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn test_rpki_state_display() {
        assert_eq!(format!("{}", RpkiState::Valid),    "valid");
        assert_eq!(format!("{}", RpkiState::Invalid),  "invalid");
        assert_eq!(format!("{}", RpkiState::NotFound), "not-found");
    }

    #[test]
    fn test_covering_prefix_not_covered_by_more_specific() {
        let cache = VrpCache::new();
        cache.reset(vec![VrpEntry { prefix: net("192.0.2.0/25"), max_len: 25, origin_asn: 64496 }], 1);
        // /24 is LESS specific than /25 in ROA — not covered
        assert_eq!(cache.validate(net("192.0.2.0/24"), 64496), RpkiState::NotFound);
    }
}
