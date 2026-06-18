use std::collections::HashMap;
use std::net::IpAddr;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use rbmp_core::bgp::types::{PathAttributes, Prefix};
use rbmp_core::bmp::types::RibType;

/// A route entry in the RIB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RibEntry {
    pub prefix:     Prefix,
    pub attributes: PathAttributes,
    pub received_at: DateTime<Utc>,
    pub peer_addr:  IpAddr,
    pub peer_as:    u32,
}

/// Per-peer, per-RIB-type route table.
/// Key: Prefix → RibEntry (last-received wins; true multi-path via Add-Path is a future extension).
#[derive(Debug, Default)]
pub struct RibTable {
    /// rib_type → prefix → entry
    tables: HashMap<RibType, HashMap<String, RibEntry>>,
}

impl RibTable {
    pub fn new() -> Self { Self::default() }

    /// Insert or replace a route. Returns true if this is a new prefix, false if update.
    pub fn insert(&mut self, rib: RibType, entry: RibEntry) -> bool {
        let key = entry.prefix.to_string();
        let table = self.tables.entry(rib).or_default();
        let is_new = !table.contains_key(&key);
        table.insert(key, entry);
        is_new
    }

    /// Remove a route. Returns the removed entry if it existed.
    pub fn remove(&mut self, rib: RibType, prefix: &Prefix) -> Option<RibEntry> {
        let key = prefix.to_string();
        self.tables.get_mut(&rib)?.remove(&key)
    }

    pub fn get(&self, rib: RibType, prefix: &Prefix) -> Option<&RibEntry> {
        self.tables.get(&rib)?.get(&prefix.to_string())
    }

    pub fn count(&self, rib: RibType) -> usize {
        self.tables.get(&rib).map_or(0, |t| t.len())
    }

    pub fn iter_rib(&self, rib: RibType) -> impl Iterator<Item = &RibEntry> {
        self.tables.get(&rib).into_iter().flat_map(|t| t.values())
    }

    pub fn all_prefixes(&self) -> Vec<&RibEntry> {
        self.tables.values().flat_map(|t| t.values()).collect()
    }

    /// Clear all routes for a specific RIB type (e.g., on Peer Down)
    pub fn clear_rib(&mut self, rib: RibType) {
        self.tables.remove(&rib);
    }

    /// Clear all routes for this peer entirely
    pub fn clear_all(&mut self) {
        self.tables.clear();
    }

    pub fn rib_counts(&self) -> HashMap<RibType, usize> {
        self.tables.iter().map(|(k, v)| (*k, v.len())).collect()
    }
}
