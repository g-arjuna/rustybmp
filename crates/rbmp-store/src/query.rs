use std::sync::{Arc, Mutex};
use duckdb::Row;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::duck::RouteStore;

/// A flattened route row returned by queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRow {
    pub occurred_at:  String,
    pub speaker_addr: String,
    pub peer_addr:    String,
    pub peer_as:      u32,
    pub rib_type:     String,
    pub action:       String,
    pub prefix:       String,
    pub afi:          String,
    pub origin:       Option<String>,
    pub as_path:      Option<String>,
    pub as_path_len:  Option<u16>,
    pub next_hop:     Option<String>,
    pub local_pref:   Option<u32>,
    pub med:          Option<u32>,
    pub communities:  Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEventRow {
    pub occurred_at:  String,
    pub speaker_addr: String,
    pub peer_addr:    String,
    pub peer_as:      Option<u32>,
    pub event_type:   String,
    pub hold_time:    Option<u16>,
    pub reason:       Option<String>,
}

pub struct QueryEngine {
    store: Arc<Mutex<RouteStore>>,
}

impl QueryEngine {
    pub fn new(store: Arc<Mutex<RouteStore>>) -> Self { Self { store } }

    /// Latest announced routes for a peer (current RIB snapshot)
    pub fn current_rib(&self, peer_addr: &str, rib_type: Option<&str>, limit: usize) -> Result<Vec<RouteRow>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let rib_filter = rib_type.map(|r| format!("AND rib_type = '{}'", r)).unwrap_or_default();
        let sql = format!(
            r#"SELECT occurred_at, speaker_addr, peer_addr, peer_as, rib_type, action,
                      prefix, afi, origin, as_path, as_path_len, next_hop, local_pref, med, communities
               FROM (
                 SELECT *, ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
                 FROM route_events
                 WHERE peer_addr = '{}' {}
               ) t
               WHERE rn = 1 AND action = 'announce'
               ORDER BY prefix
               LIMIT {}"#,
            peer_addr, rib_filter, limit
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_route_row)?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Route history for a specific prefix
    pub fn prefix_history(&self, prefix: &str, limit: usize) -> Result<Vec<RouteRow>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            "SELECT occurred_at, speaker_addr, peer_addr, peer_as, rib_type, action,
                    prefix, afi, origin, as_path, as_path_len, next_hop, local_pref, med, communities
             FROM route_events WHERE prefix = '{prefix}'
             ORDER BY occurred_at DESC LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_route_row)?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Route changes in a time window
    pub fn route_changes(&self, since: &str, until: Option<&str>, limit: usize) -> Result<Vec<RouteRow>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let until_clause = until.map(|u| format!("AND occurred_at <= '{u}'")).unwrap_or_default();
        let sql = format!(
            "SELECT occurred_at, speaker_addr, peer_addr, peer_as, rib_type, action,
                    prefix, afi, origin, as_path, as_path_len, next_hop, local_pref, med, communities
             FROM route_events
             WHERE occurred_at >= '{since}' {until_clause}
             ORDER BY occurred_at DESC LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_route_row)?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Top N most active prefixes by churn (announce+withdraw count)
    pub fn top_churning_prefixes(&self, limit: usize) -> Result<Vec<(String, u64)>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            "SELECT prefix, COUNT(*) AS churn FROM route_events
             GROUP BY prefix ORDER BY churn DESC LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let prefix: String = row.get(0)?;
            let churn: u64     = row.get(1)?;
            Ok((prefix, churn))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// AS path popularity (how many times each AS appears in paths)
    pub fn as_origin_counts(&self, limit: usize) -> Result<Vec<(u32, u64)>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            r#"SELECT CAST(trim(last) AS UINTEGER) AS origin_asn, COUNT(*) AS cnt
               FROM (
                 SELECT string_split(as_path, ' ')[-1] AS last
                 FROM route_events
                 WHERE action = 'announce' AND as_path IS NOT NULL AND as_path <> ''
               ) sub
               WHERE last IS NOT NULL AND last <> ''
               GROUP BY origin_asn
               ORDER BY cnt DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let asn: u32 = row.get(0)?;
            let cnt: u64 = row.get(1)?;
            Ok((asn, cnt))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Peer session events (up/down history)
    pub fn peer_history(&self, peer_addr: &str) -> Result<Vec<PeerEventRow>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            "SELECT occurred_at, speaker_addr, peer_addr, peer_as, event_type, hold_time, reason
             FROM peer_events WHERE peer_addr = '{peer_addr}'
             ORDER BY occurred_at DESC LIMIT 200"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(PeerEventRow {
                occurred_at:  row.get(0)?,
                speaker_addr: row.get(1)?,
                peer_addr:    row.get(2)?,
                peer_as:      row.get(3)?,
                event_type:   row.get(4)?,
                hold_time:    row.get(5)?,
                reason:       row.get(6)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }
}

fn map_route_row(row: &Row) -> duckdb::Result<RouteRow> {
    Ok(RouteRow {
        occurred_at:  row.get(0)?,
        speaker_addr: row.get(1)?,
        peer_addr:    row.get(2)?,
        peer_as:      row.get(3)?,
        rib_type:     row.get(4)?,
        action:       row.get(5)?,
        prefix:       row.get(6)?,
        afi:          row.get(7)?,
        origin:       row.get(8)?,
        as_path:      row.get(9)?,
        as_path_len:  row.get(10)?,
        next_hop:     row.get(11)?,
        local_pref:   row.get(12)?,
        med:          row.get(13)?,
        communities:  row.get(14)?,
    })
}
