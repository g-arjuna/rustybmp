use std::sync::{Arc, Mutex};
use duckdb::Row;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

    /// Prefix timeline — announce/withdraw events bucketed by hour for the last N days
    pub fn prefix_timeline(&self, prefix: &str, days: u32) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            r#"SELECT
                time_bucket(INTERVAL '1 hour', occurred_at) AS bucket,
                action,
                COUNT(*) AS event_count
               FROM route_events
               WHERE prefix = '{prefix}'
                 AND occurred_at >= NOW() - INTERVAL '{days} days'
               GROUP BY bucket, action
               ORDER BY bucket"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let bucket: String    = row.get(0)?;
            let action: String    = row.get(1)?;
            let count:  u64       = row.get(2)?;
            Ok(serde_json::json!({ "bucket": bucket, "action": action, "count": count }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Which peers currently see a prefix and their most recent AS_PATH
    pub fn prefix_peers(&self, prefix: &str) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            r#"SELECT peer_addr, peer_as, as_path, next_hop, local_pref, communities,
                      occurred_at
               FROM (
                 SELECT *, ROW_NUMBER() OVER (PARTITION BY peer_addr ORDER BY occurred_at DESC) AS rn
                 FROM route_events
                 WHERE prefix = '{prefix}' AND action = 'announce'
               ) t WHERE rn = 1
               ORDER BY peer_addr"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let peer_addr:  String         = row.get(0)?;
            let peer_as:    u32            = row.get(1)?;
            let as_path:    Option<String> = row.get(2)?;
            let next_hop:   Option<String> = row.get(3)?;
            let local_pref: Option<u32>    = row.get(4)?;
            let communities: Option<String> = row.get(5)?;
            let occurred_at: String        = row.get(6)?;
            Ok(serde_json::json!({
                "peer_addr": peer_addr, "peer_as": peer_as,
                "as_path": as_path, "next_hop": next_hop,
                "local_pref": local_pref, "communities": communities,
                "last_seen": occurred_at
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Convergence times for the last N events on a prefix
    /// Returns the gap in seconds between sequential announce events per peer
    pub fn prefix_convergence(&self, prefix: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            r#"SELECT
                peer_addr,
                occurred_at,
                action,
                epoch(occurred_at) - lag(epoch(occurred_at)) OVER (PARTITION BY peer_addr ORDER BY occurred_at) AS gap_secs
               FROM route_events
               WHERE prefix = '{prefix}'
               ORDER BY occurred_at DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let peer_addr:  String     = row.get(0)?;
            let occurred_at: String    = row.get(1)?;
            let action:     String     = row.get(2)?;
            let gap_secs:   Option<f64> = row.get(3)?;
            Ok(serde_json::json!({
                "peer_addr": peer_addr,
                "occurred_at": occurred_at,
                "action": action,
                "gap_secs": gap_secs
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Peer session timeline — up/down events in last N days with duration
    pub fn peer_session_timeline(&self, peer_addr: &str, days: u32) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            r#"SELECT occurred_at, event_type, reason,
                      epoch(lead(occurred_at) OVER (ORDER BY occurred_at)) - epoch(occurred_at) AS duration_secs
               FROM peer_events
               WHERE peer_addr = '{peer_addr}'
                 AND occurred_at >= NOW() - INTERVAL '{days} days'
               ORDER BY occurred_at DESC"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let occurred_at:   String     = row.get(0)?;
            let event_type:    String     = row.get(1)?;
            let reason:        Option<String> = row.get(2)?;
            let duration_secs: Option<f64>   = row.get(3)?;
            Ok(serde_json::json!({
                "occurred_at": occurred_at,
                "event_type": event_type,
                "reason": reason,
                "duration_secs": duration_secs
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// RPKI analysis — breakdown by validity + per-peer invalid rates
    pub fn rpki_analysis(&self) -> Result<serde_json::Value> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();

        // Overall breakdown
        let breakdown_sql = r#"SELECT rpki_validity, COUNT(DISTINCT prefix) AS cnt
            FROM (
              SELECT prefix, rpki_validity,
                     ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
              FROM route_events WHERE action = 'announce'
            ) t WHERE rn = 1
            GROUP BY rpki_validity ORDER BY cnt DESC"#;
        let mut stmt = conn.prepare(breakdown_sql)?;
        let breakdown: Vec<serde_json::Value> = stmt.query_map([], |row| {
            let validity: Option<String> = row.get(0)?;
            let cnt: u64 = row.get(1)?;
            Ok(serde_json::json!({ "validity": validity.unwrap_or_else(|| "unknown".into()), "count": cnt }))
        })?.filter_map(|r| r.ok()).collect();

        // Per-peer invalid rate
        let peer_sql = r#"SELECT peer_addr, peer_as,
                SUM(CASE WHEN rpki_validity='invalid' THEN 1 ELSE 0 END)::FLOAT / COUNT(*) AS invalid_rate,
                COUNT(*) AS total
            FROM route_events WHERE action = 'announce'
            GROUP BY peer_addr, peer_as ORDER BY invalid_rate DESC LIMIT 20"#;
        let mut stmt2 = conn.prepare(peer_sql)?;
        let per_peer: Vec<serde_json::Value> = stmt2.query_map([], |row| {
            let peer_addr:    String = row.get(0)?;
            let peer_as:      u32    = row.get(1)?;
            let invalid_rate: f64    = row.get(2)?;
            let total:        u64    = row.get(3)?;
            Ok(serde_json::json!({ "peer_addr": peer_addr, "peer_as": peer_as, "invalid_rate": invalid_rate, "total": total }))
        })?.filter_map(|r| r.ok()).collect();

        Ok(serde_json::json!({ "breakdown": breakdown, "per_peer": per_peer }))
    }

    /// Policy analysis — pre vs post-policy diff for a peer
    pub fn policy_delta(&self, peer_addr: &str) -> Result<serde_json::Value> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = format!(
            r#"SELECT rib_type, COUNT(DISTINCT prefix) AS prefix_count
               FROM (
                 SELECT prefix, rib_type,
                        ROW_NUMBER() OVER (PARTITION BY prefix, rib_type ORDER BY occurred_at DESC) AS rn
                 FROM route_events WHERE peer_addr = '{peer_addr}' AND action = 'announce'
               ) t WHERE rn = 1
               GROUP BY rib_type"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let by_rib: Vec<serde_json::Value> = stmt.query_map([], |row| {
            let rib_type:      String = row.get(0)?;
            let prefix_count:  u64   = row.get(1)?;
            Ok(serde_json::json!({ "rib_type": rib_type, "prefix_count": prefix_count }))
        })?.filter_map(|r| r.ok()).collect();

        Ok(serde_json::json!({ "peer_addr": peer_addr, "by_rib_type": by_rib }))
    }

    /// ML anomaly detections from the Python pipeline (most recent first)
    pub fn ml_anomalies(&self, limit: usize, kind: Option<&str>) -> Result<Vec<Value>> {
        let locked = self.store.lock().unwrap();
        let conn   = locked.conn();
        let kind_filter = kind.map(|k| format!("AND kind = '{k}'")).unwrap_or_default();
        let sql = format!(
            r#"SELECT detected_at, kind, prefix, peer_addr, score, description, severity
               FROM ml_anomalies
               WHERE 1=1 {kind_filter}
               ORDER BY detected_at DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let detected_at: String    = row.get(0)?;
            let kind:        String    = row.get(1)?;
            let prefix:      Option<String> = row.get(2)?;
            let peer_addr:   Option<String> = row.get(3)?;
            let score:       Option<f64>    = row.get(4)?;
            let description: Option<String> = row.get(5)?;
            let severity:    Option<String> = row.get(6)?;
            Ok(serde_json::json!({
                "detected_at": detected_at, "kind": kind,
                "prefix": prefix, "peer_addr": peer_addr,
                "score": score, "description": description,
                "severity": severity
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    // ─── RV6-5 new query methods ──────────────────────────────────────────────

    /// SR Policy list (RV6-5) — reads from srpolicy_events table
    pub fn srpolicy_list(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_name = 'srpolicy_events'",
            [], |row| row.get(0),
        ).unwrap_or(false);
        if !table_exists { return Ok(Vec::new()); }
        let sql = format!(
            r#"SELECT occurred_at, speaker_addr, peer_addr, peer_as, action,
                      endpoint, color, preference, bsid, segment_list, distinguisher
               FROM srpolicy_events
               ORDER BY occurred_at DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "occurred_at":  row.get::<_,String>(0)?,
                "speaker_addr": row.get::<_,String>(1)?,
                "peer_addr":    row.get::<_,String>(2)?,
                "peer_as":      row.get::<_,u32>(3)?,
                "action":       row.get::<_,String>(4)?,
                "endpoint":     row.get::<_,Option<String>>(5)?,
                "color":        row.get::<_,Option<u32>>(6)?,
                "preference":   row.get::<_,Option<u32>>(7)?,
                "bsid":         row.get::<_,Option<String>>(8)?,
                "segment_list": row.get::<_,Option<String>>(9)?,
                "distinguisher": row.get::<_,Option<u32>>(10)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// SR Policy list filtered by peer address
    pub fn srpolicy_by_peer(&self, peer_addr: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_name = 'srpolicy_events'",
            [], |row| row.get(0),
        ).unwrap_or(false);
        if !table_exists { return Ok(Vec::new()); }
        let sql = format!(
            r#"SELECT occurred_at, speaker_addr, peer_addr, peer_as, action,
                      endpoint, color, preference, bsid, segment_list, distinguisher
               FROM srpolicy_events
               WHERE peer_addr = '{peer_addr}'
               ORDER BY occurred_at DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "occurred_at":  row.get::<_,String>(0)?,
                "speaker_addr": row.get::<_,String>(1)?,
                "peer_addr":    row.get::<_,String>(2)?,
                "peer_as":      row.get::<_,u32>(3)?,
                "action":       row.get::<_,String>(4)?,
                "endpoint":     row.get::<_,Option<String>>(5)?,
                "color":        row.get::<_,Option<u32>>(6)?,
                "preference":   row.get::<_,Option<u32>>(7)?,
                "bsid":         row.get::<_,Option<String>>(8)?,
                "segment_list": row.get::<_,Option<String>>(9)?,
                "distinguisher": row.get::<_,Option<u32>>(10)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// AS-path graph data for Sankey rendering (RV6-5)
    /// Returns { nodes: [{id, label}], links: [{source, target, value}] }
    pub fn aspath_graph(&self, filter_asn: Option<u32>, peer: Option<&str>, limit: usize) -> Result<serde_json::Value> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let peer_clause = peer.map(|p| format!("AND peer_addr = '{p}'")).unwrap_or_default();
        let asn_clause  = filter_asn.map(|a| format!("AND as_path LIKE '%{a}%'")).unwrap_or_default();
        let sql = format!(
            r#"SELECT as_path FROM route_events
               WHERE action = 'announce' AND as_path IS NOT NULL AND as_path <> ''
               {peer_clause} {asn_clause}
               ORDER BY occurred_at DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        // Build edge-count map from consecutive ASN pairs
        let mut edge_counts: std::collections::HashMap<(u32, u32), u64> = std::collections::HashMap::new();
        let _ = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let asns: Vec<u32> = path.split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            for pair in asns.windows(2) {
                *edge_counts.entry((pair[0], pair[1])).or_insert(0) += 1;
            }
            Ok(())
        })?.for_each(|_| {});

        let mut asn_set: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (src, dst) in edge_counts.keys() {
            asn_set.insert(*src);
            asn_set.insert(*dst);
        }
        let nodes: Vec<serde_json::Value> = asn_set.iter()
            .map(|asn| serde_json::json!({ "id": asn.to_string(), "label": format!("AS{asn}") }))
            .collect();
        let links: Vec<serde_json::Value> = edge_counts.iter()
            .map(|((src, dst), count)| serde_json::json!({
                "source": src.to_string(),
                "target": dst.to_string(),
                "value":  count,
            }))
            .collect();
        Ok(serde_json::json!({ "nodes": nodes, "links": links }))
    }

    /// BMP statistics counter history (RV6-5)
    pub fn bmp_stats_history(&self, peer_addr: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let peer_clause = peer_addr.map(|p| format!("WHERE peer_addr = '{p}'")).unwrap_or_default();
        let sql = format!(
            r#"SELECT occurred_at, speaker_addr, peer_addr, counter_name, counter_value, stat_type, afi, safi
               FROM stats_events
               {peer_clause}
               ORDER BY occurred_at DESC
               LIMIT {limit}"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "occurred_at":   row.get::<_,String>(0)?,
                "speaker_addr":  row.get::<_,String>(1)?,
                "peer_addr":     row.get::<_,String>(2)?,
                "counter_name":  row.get::<_,String>(3)?,
                "counter_value": row.get::<_,u64>(4)?,
                "stat_type":     row.get::<_,Option<u16>>(5)?,
                "afi":           row.get::<_,Option<u16>>(6)?,
                "safi":          row.get::<_,Option<u8>>(7)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// RPKI coverage analysis (RV6-5)
    /// Returns what % of announced prefixes have a matching ROA.
    pub fn rpki_coverage(&self) -> Result<serde_json::Value> {
        let locked = self.store.lock().unwrap();
        let conn = locked.conn();
        let sql = r#"
            SELECT
                COUNT(DISTINCT prefix) AS total_prefixes,
                SUM(CASE WHEN rpki_validity IN ('valid','invalid') THEN 1 ELSE 0 END) AS covered,
                SUM(CASE WHEN rpki_validity = 'valid'     THEN 1 ELSE 0 END) AS valid,
                SUM(CASE WHEN rpki_validity = 'invalid'   THEN 1 ELSE 0 END) AS invalid,
                SUM(CASE WHEN rpki_validity = 'not-found' OR rpki_validity IS NULL THEN 1 ELSE 0 END) AS not_covered
            FROM (
                SELECT prefix, rpki_validity,
                       ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
                FROM route_events WHERE action = 'announce'
            ) t WHERE rn = 1
        "#;
        let mut stmt = conn.prepare(sql)?;
        let row = stmt.query_row([], |row| {
            let total:       u64 = row.get(0)?;
            let covered:     u64 = row.get(1)?;
            let valid:       u64 = row.get(2)?;
            let invalid:     u64 = row.get(3)?;
            let not_covered: u64 = row.get(4)?;
            Ok((total, covered, valid, invalid, not_covered))
        })?;
        let (total, covered, valid, invalid, not_covered) = row;
        let coverage_pct = if total > 0 { covered as f64 / total as f64 * 100.0 } else { 0.0 };
        Ok(serde_json::json!({
            "total_prefixes":  total,
            "covered":         covered,
            "not_covered":     not_covered,
            "valid":           valid,
            "invalid":         invalid,
            "coverage_pct":    (coverage_pct * 10.0).round() / 10.0,
        }))
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
