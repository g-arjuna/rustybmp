/// RV6-2: ASPA (Autonomous System Provider Authorization) validation.
///
/// Implements the path-validation algorithm from RFC 9319.
///
/// ASPA validation is a BGP path security mechanism that verifies whether
/// each AS in the AS_PATH has authorized the next AS in the path as a
/// provider (for upstream routing). This catches route leaks by identifying
/// paths where customer-to-provider direction is violated.
///
/// ## Validation algorithm (RFC 9319 §4.1)
///
/// For a received UPDATE on an eBGP session:
/// 1. Extract AS_PATH hops (de-duplicated, SET segments treated as unordered).
/// 2. For each consecutive pair (A, B) in the direction of the path:
///    - If a ASPA record exists for A: check that B is in A's provider set.
///    - If no ASPA for A: result is "unknown" for that hop.
/// 3. Overall verdict:
///    - `Valid`   — every hop that has an ASPA record was confirmed.
///    - `Invalid` — at least one hop with a record failed authorization.
///    - `Unknown` — no hop failed but some lacked ASPA coverage.
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

// ─── ASPA record types ────────────────────────────────────────────────────────

/// One ASPA record: a customer AS and its authorized provider set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspaRecord {
    /// The customer AS number
    pub customer_asn: u32,
    /// ASNs of authorized providers (allowed to appear above this AS in path)
    pub provider_asns: Vec<u32>,
}

/// In-memory ASPA table (customer_asn → provider set).
#[derive(Debug, Default, Clone)]
pub struct AspaTable {
    records: HashMap<u32, HashSet<u32>>,
}

impl AspaTable {
    pub fn new() -> Self { Self::default() }

    /// Load records from a slice (e.g. parsed from JSON or RPKI cache).
    pub fn load(&mut self, records: impl IntoIterator<Item = AspaRecord>) {
        self.records.clear();
        for r in records {
            self.records
                .entry(r.customer_asn)
                .or_insert_with(HashSet::new)
                .extend(r.provider_asns);
        }
    }

    /// Insert or merge a single ASPA record.
    pub fn upsert(&mut self, record: AspaRecord) {
        self.records
            .entry(record.customer_asn)
            .or_insert_with(HashSet::new)
            .extend(record.provider_asns);
    }

    /// Returns the provider set for a customer ASN, or None if no record exists.
    pub fn providers_of(&self, customer_asn: u32) -> Option<&HashSet<u32>> {
        self.records.get(&customer_asn)
    }

    /// Total number of ASPA records loaded.
    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}

// ─── Validation result ────────────────────────────────────────────────────────

/// ASPA validation verdict for a single BGP UPDATE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AspaValidity {
    /// Every hop confirmed by a matching ASPA record.
    Valid,
    /// At least one hop violated an existing ASPA record.
    Invalid,
    /// No violations found, but coverage was incomplete.
    Unknown,
}

impl std::fmt::Display for AspaValidity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid   => write!(f, "valid"),
            Self::Invalid => write!(f, "invalid"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

// ─── Validator ────────────────────────────────────────────────────────────────

/// Validate an AS_PATH against the ASPA table.
///
/// `as_path` must be in announcement direction (left = closest to receiver,
/// right = origin). For a received BGP UPDATE the `as_path` is already in
/// this order (leftmost = first hop from local speaker's perspective).
///
/// # Arguments
/// - `as_path` — the AS_PATH as a flat ordered list (SEQUENCE segments, de-duped)
/// - `table`   — the ASPA table to validate against
///
/// # Returns
/// `(verdict, violated_hop)` where `violated_hop` is `Some((provider, customer))`
/// on Invalid, None otherwise.
pub fn validate_as_path(
    as_path: &[u32],
    table: &AspaTable,
) -> (AspaValidity, Option<(u32, u32)>) {
    if as_path.is_empty() {
        return (AspaValidity::Unknown, None);
    }

    let mut any_unknown = false;

    // Walk path pairs: as_path[i] = provider, as_path[i+1] = customer (towards origin)
    // In the upstream direction (receiver → origin) each AS should have authorized
    // the next AS (closer to origin) as a provider.
    for pair in as_path.windows(2) {
        let closer_to_receiver = pair[0];  // "provider" in the path
        let closer_to_origin   = pair[1];  // "customer" = should list pair[0] as provider

        match table.providers_of(closer_to_origin) {
            None => {
                // No ASPA record for the customer — unknown for this hop
                any_unknown = true;
            }
            Some(providers) => {
                if !providers.contains(&closer_to_receiver) {
                    // ASPA record exists but provider not listed — INVALID
                    return (AspaValidity::Invalid, Some((closer_to_receiver, closer_to_origin)));
                }
            }
        }
    }

    if any_unknown {
        (AspaValidity::Unknown, None)
    } else {
        (AspaValidity::Valid, None)
    }
}

/// Parse a space-separated AS_PATH string into an ordered list of unique ASNs.
/// Consecutive duplicates (prepending) are collapsed as per RFC 9319 §4.
pub fn parse_as_path(as_path: &str) -> Vec<u32> {
    let mut result: Vec<u32> = Vec::new();
    for token in as_path.split_whitespace() {
        if let Ok(asn) = token.parse::<u32>() {
            // Collapse consecutive prepend duplicates
            if result.last() != Some(&asn) {
                result.push(asn);
            }
        }
    }
    result
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn table_with(records: &[(u32, &[u32])]) -> AspaTable {
        let mut t = AspaTable::new();
        t.load(records.iter().map(|(cust, provs)| AspaRecord {
            customer_asn: *cust,
            provider_asns: provs.to_vec(),
        }));
        t
    }

    #[test]
    fn test_valid_path() {
        // AS_PATH: 64500 64501 64502 (each AS authorizes the previous as provider)
        // 64502 lists 64501 as provider, 64501 lists 64500 as provider
        let table = table_with(&[
            (64502, &[64501]),
            (64501, &[64500]),
        ]);
        let path = parse_as_path("64500 64501 64502");
        let (verdict, hop) = validate_as_path(&path, &table);
        assert_eq!(verdict, AspaValidity::Valid);
        assert!(hop.is_none());
    }

    #[test]
    fn test_invalid_path_route_leak() {
        // 64502 claims 64501 is a provider, but 64501 did NOT authorize 64500
        let table = table_with(&[
            (64502, &[64501]),
            (64501, &[99999]), // 64500 is not a provider of 64501 — route leak!
        ]);
        let path = parse_as_path("64500 64501 64502");
        let (verdict, hop) = validate_as_path(&path, &table);
        assert_eq!(verdict, AspaValidity::Invalid);
        assert_eq!(hop, Some((64500, 64501)));
    }

    #[test]
    fn test_unknown_no_records() {
        let table = AspaTable::new();
        let path = parse_as_path("1 2 3 4");
        let (verdict, _) = validate_as_path(&path, &table);
        assert_eq!(verdict, AspaValidity::Unknown);
    }

    #[test]
    fn test_prepend_collapsed() {
        let parsed = parse_as_path("64500 64500 64500 64501");
        assert_eq!(parsed, vec![64500, 64501]);
    }

    #[test]
    fn test_empty_path() {
        let table = AspaTable::new();
        let (verdict, _) = validate_as_path(&[], &table);
        assert_eq!(verdict, AspaValidity::Unknown);
    }

    #[test]
    fn test_partial_coverage_unknown() {
        // 64502 has a record, 64501 does not
        let table = table_with(&[(64502, &[64501])]);
        let path = parse_as_path("64500 64501 64502");
        let (verdict, _) = validate_as_path(&path, &table);
        // 64502 valid, 64501 unknown → overall Unknown
        assert_eq!(verdict, AspaValidity::Unknown);
    }

    #[test]
    fn test_single_hop_no_record() {
        let table = AspaTable::new();
        let path = parse_as_path("64500");
        let (verdict, hop) = validate_as_path(&path, &table);
        // No pairs → no violations and any_unknown stays false → Valid
        assert_eq!(verdict, AspaValidity::Valid, "single AS path with no pairs has no violations → Valid");
        assert!(hop.is_none());
    }

    #[test]
    fn test_single_hop_with_record_no_pair() {
        // Only one AS in path — no pairs to check, verdict should be Valid (no violations)
        let table = table_with(&[(64500, &[64499])]);
        let path = parse_as_path("64500");
        let (verdict, _) = validate_as_path(&path, &table);
        assert_eq!(verdict, AspaValidity::Valid, "single AS in path with record → Valid (no pairs to violate)");
    }

    #[test]
    fn test_upsert_merges_providers() {
        let mut table = AspaTable::new();
        table.upsert(AspaRecord { customer_asn: 65001, provider_asns: vec![65000] });
        table.upsert(AspaRecord { customer_asn: 65001, provider_asns: vec![65002] });
        let providers = table.providers_of(65001).unwrap();
        assert!(providers.contains(&65000));
        assert!(providers.contains(&65002));
        assert_eq!(table.len(), 1, "upsert should not create duplicate customer records");
    }

    #[test]
    fn test_load_overwrites_existing() {
        let mut table = table_with(&[(65001, &[65000])]);
        assert_eq!(table.len(), 1);
        // Load replaces all records
        table.load(vec![
            AspaRecord { customer_asn: 65002, provider_asns: vec![65001] },
            AspaRecord { customer_asn: 65003, provider_asns: vec![65001] },
        ]);
        assert_eq!(table.len(), 2);
        assert!(table.providers_of(65001).is_none(), "old record must be gone after load");
    }

    #[test]
    fn test_is_empty_then_populated() {
        let mut table = AspaTable::new();
        assert!(table.is_empty());
        table.upsert(AspaRecord { customer_asn: 1, provider_asns: vec![2] });
        assert!(!table.is_empty());
    }

    #[test]
    fn test_aspa_validity_display() {
        assert_eq!(format!("{}", AspaValidity::Valid),   "valid");
        assert_eq!(format!("{}", AspaValidity::Invalid), "invalid");
        assert_eq!(format!("{}", AspaValidity::Unknown), "unknown");
    }

    #[test]
    fn test_parse_as_path_empty_string() {
        let path = parse_as_path("");
        assert!(path.is_empty(), "empty string must produce empty path");
    }

    #[test]
    fn test_parse_as_path_single() {
        assert_eq!(parse_as_path("65001"), vec![65001u32]);
    }

    #[test]
    fn test_parse_as_path_prepend_collapsed_middle() {
        let path = parse_as_path("64500 64501 64501 64502");
        assert_eq!(path, vec![64500, 64501, 64502]);
    }

    #[test]
    fn test_invalid_violated_hop_is_reported() {
        let table = table_with(&[(64502, &[64501]), (64501, &[64503])]);
        let path = parse_as_path("64500 64501 64502");
        let (verdict, hop) = validate_as_path(&path, &table);
        assert_eq!(verdict, AspaValidity::Invalid);
        assert_eq!(hop, Some((64500, 64501)), "violated hop must be (provider, customer)");
    }
}
