use std::collections::HashMap;
use std::net::IpAddr;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use rbmp_core::bgp::types::{PathAttributes, Prefix};
use rbmp_core::bmp::types::RibType;

/// A route entry in the RIB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RibEntry {
    pub prefix:      Prefix,
    /// Add-Path path ID (RFC 7911); None when Add-Path not active for this AFI-SAFI
    pub path_id:     Option<u32>,
    pub attributes:  PathAttributes,
    pub received_at: DateTime<Utc>,
    pub peer_addr:   IpAddr,
    pub peer_as:     u32,
    /// True when this is the best-path among multiple paths for the same prefix
    pub is_best:     bool,
}

/// Per-peer, per-RIB-type route table.
/// Key: Prefix → RibEntry (last-received wins; true multi-path via Add-Path is a future extension).
#[derive(Debug, Default)]
pub struct RibTable {
    /// rib_type → prefix → entry
    tables: HashMap<RibType, HashMap<String, RibEntry>>,
}

fn entry_key(prefix: &Prefix, path_id: Option<u32>) -> String {
    match path_id {
        Some(id) => format!("{}@{}", prefix, id),
        None     => prefix.to_string(),
    }
}

impl RibTable {
    pub fn new() -> Self { Self::default() }

    /// Insert or replace a route. Returns true if this is a new prefix, false if update.
    pub fn insert(&mut self, rib: RibType, entry: RibEntry) -> bool {
        let key   = entry_key(&entry.prefix, entry.path_id);
        let table = self.tables.entry(rib).or_default();
        let is_new = !table.contains_key(&key);
        table.insert(key, entry);
        is_new
    }

    /// Remove a route (uses prefix + path_id for compound key). Returns the removed entry.
    pub fn remove(&mut self, rib: RibType, prefix: &Prefix) -> Option<RibEntry> {
        self.remove_with_path_id(rib, prefix, None)
    }

    pub fn remove_with_path_id(&mut self, rib: RibType, prefix: &Prefix, path_id: Option<u32>) -> Option<RibEntry> {
        let key = entry_key(prefix, path_id);
        self.tables.get_mut(&rib)?.remove(&key)
    }

    pub fn get(&self, rib: RibType, prefix: &Prefix) -> Option<&RibEntry> {
        let key = entry_key(prefix, None);
        self.tables.get(&rib)?.get(&key)
    }

    /// Recompute best-path among multiple Add-Path entries for a prefix.
    /// Tie-breakers (in order): highest LOCAL_PREF, shortest AS_PATH, lowest MED.
    pub fn recompute_best_path(&mut self, rib: RibType, prefix: &Prefix) {
        let prefix_str = prefix.to_string();
        let table = match self.tables.get_mut(&rib) { Some(t) => t, None => return };
        let path_id_prefix = format!("{}@", prefix_str);
        let matching_keys: Vec<String> = table.keys()
            .filter(|k| *k == &prefix_str || k.starts_with(&path_id_prefix))
            .cloned()
            .collect();
        if matching_keys.len() <= 1 {
            if let Some(key) = matching_keys.first() {
                if let Some(e) = table.get_mut(key) { e.is_best = true; }
            }
            return;
        }
        let best_key = matching_keys.iter().min_by_key(|k| {
            let e = &table[k.as_str()];
            let lp  = e.attributes.local_pref.map(|v| u32::MAX - v).unwrap_or(u32::MAX);
            let hop = e.attributes.as_path.as_ref().map(|p| p.hop_count() as u32).unwrap_or(0);
            let med = e.attributes.multi_exit_disc.unwrap_or(u32::MAX);
            (lp, hop, med)
        }).cloned();
        for k in &matching_keys {
            if let Some(e) = table.get_mut(k) {
                e.is_best = best_key.as_deref() == Some(k.as_str());
            }
        }
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
