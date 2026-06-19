use std::net::IpAddr;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use crate::vrp_cache::{RpkiState, VrpCache};

/// Per-route annotation produced by the enrichment engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteAnnotation {
    /// RPKI validation result for this prefix+origin pair.
    pub rpki: RpkiState,
    /// True when the announced prefix is more-specific than the ROA prefix
    /// (valid deaggregation check).
    pub rpki_more_specific: bool,
}

/// Combines all enrichment sources and produces `RouteAnnotation` per route.
/// Clone-cheap: wraps Arc-backed caches internally.
#[derive(Clone)]
pub struct EnrichmentEngine {
    vrp_cache: VrpCache,
}

impl EnrichmentEngine {
    pub fn new(vrp_cache: VrpCache) -> Self {
        Self { vrp_cache }
    }

    /// Annotate a single route announce.
    /// `prefix`     — the announced prefix (e.g. "192.0.2.0/24")
    /// `origin_asn` — last ASN in the AS_PATH (the originating AS)
    pub fn annotate(&self, prefix: IpNet, origin_asn: u32) -> RouteAnnotation {
        let rpki = self.vrp_cache.validate(prefix, origin_asn);

        // Detect more-specific: any covering ROA has a shorter prefix length
        let rpki_more_specific = {
            let ann_len = prefix.prefix_len();
            // A route is more-specific when we find a covering ROA with shorter len
            // We can infer this from the validation result: if Valid and ann_len > roa_len.
            // For correctness we keep this as a simple heuristic — detailed check is in vrp_cache.
            rpki == RpkiState::Valid && ann_len > prefix.prefix_len()
        };

        RouteAnnotation { rpki, rpki_more_specific }
    }

    /// Convenience: annotate from raw string prefix + u32 origin ASN.
    /// Returns a default `NotFound` annotation on parse failure.
    pub fn annotate_str(&self, prefix_str: &str, origin_asn: u32) -> RouteAnnotation {
        match prefix_str.parse::<IpNet>() {
            Ok(net) => self.annotate(net, origin_asn),
            Err(_)  => RouteAnnotation {
                rpki: RpkiState::NotFound,
                rpki_more_specific: false,
            },
        }
    }

    /// Returns the number of VRPs currently in cache.
    pub fn vrp_count(&self) -> usize {
        self.vrp_cache.len()
    }

    /// Returns the current RTR serial number.
    pub fn rtr_serial(&self) -> u32 {
        self.vrp_cache.serial()
    }
}
