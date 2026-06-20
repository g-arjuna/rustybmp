/// BGP-LS topology graph API (RV4-6 / RV4-3 T4).
///
/// GET /api/bgpls/graph?protocol=isis
///
/// Returns the current BGP-LS node and link graph as JSON suitable for
/// D3.js force-directed rendering.  Reads from DuckDB bgpls_nodes / bgpls_links.
/// If those tables don't exist yet, returns an empty graph.
use axum::{extract::{Query, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct TopologyQuery {
    /// Filter by protocol: "isis" | "ospf" | "direct" (empty = all)
    pub protocol: Option<String>,
}

#[derive(Serialize)]
pub struct TopologyNode {
    pub id:          String,
    pub name:        Option<String>,
    pub protocol:    Option<String>,
    pub router_id:   String,
}

#[derive(Serialize)]
pub struct TopologyLink {
    pub source:      String,
    pub target:      String,
    pub local_ip:    Option<String>,
    pub remote_ip:   Option<String>,
    pub igp_metric:  Option<u32>,
    pub adj_sid:     Option<String>,
}

#[derive(Serialize)]
pub struct TopologyGraph {
    pub nodes: Vec<TopologyNode>,
    pub links: Vec<TopologyLink>,
}

pub async fn bgpls_graph(
    State(state): State<AppState>,
    Query(params): Query<TopologyQuery>,
) -> Json<TopologyGraph> {
    let graph = build_graph(&state, params.protocol.as_deref());
    Json(graph)
}

fn build_graph(state: &AppState, protocol_filter: Option<&str>) -> TopologyGraph {
    let store = match state.store.lock() {
        Ok(s) => s,
        Err(_) => return empty_graph(),
    };
    let conn = store.conn();

    // Check if bgpls_nodes table exists
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_name = 'bgpls_nodes'",
        [],
        |row| row.get(0),
    ).unwrap_or(false);

    if !table_exists {
        return empty_graph();
    }

    let proto_clause = protocol_filter
        .map(|p| format!("AND protocol_id = '{p}'"))
        .unwrap_or_default();

    // Nodes
    let node_sql = format!(
        "SELECT DISTINCT router_id, node_name, protocol_id
         FROM bgpls_nodes
         WHERE action = 'announce' {proto_clause}
         LIMIT 2000"
    );

    let mut nodes = Vec::new();
    if let Ok(mut stmt) = conn.prepare(&node_sql) {
        let _ = stmt.query_map([], |row| {
            let router_id: String    = row.get(0)?;
            let node_name: Option<String> = row.get(1)?;
            let protocol: Option<String>  = row.get(2)?;
            Ok(TopologyNode {
                id:        router_id.clone(),
                name:      node_name,
                protocol,
                router_id,
            })
        }).map(|rows| {
            for r in rows.flatten() {
                nodes.push(r);
            }
        });
    }

    // Links — most recent state per (local, remote) pair
    let link_sql =
        "SELECT local_router_id, remote_router_id, local_ip, remote_ip, igp_metric, adj_sid_labels
         FROM (
           SELECT *, ROW_NUMBER() OVER (
               PARTITION BY local_router_id, remote_router_id
               ORDER BY occurred_at DESC
           ) AS rn
           FROM bgpls_links WHERE action = 'announce'
         ) WHERE rn = 1
         LIMIT 10000";

    let mut links = Vec::new();
    if let Ok(mut stmt) = conn.prepare(link_sql) {
        let _ = stmt.query_map([], |row| {
            let source: String          = row.get(0)?;
            let target: String          = row.get(1)?;
            let local_ip: Option<String>  = row.get(2)?;
            let remote_ip: Option<String> = row.get(3)?;
            let igp_metric: Option<u32>   = row.get(4)?;
            let adj_sid: Option<String>   = row.get(5)?;
            Ok(TopologyLink { source, target, local_ip, remote_ip, igp_metric, adj_sid })
        }).map(|rows| {
            for r in rows.flatten() {
                links.push(r);
            }
        });
    }

    TopologyGraph { nodes, links }
}

fn empty_graph() -> TopologyGraph {
    TopologyGraph { nodes: Vec::new(), links: Vec::new() }
}

// ─── BGP-LS path query (RV6-5) ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PathQuery {
    pub from: Option<String>,
    pub to:   Option<String>,
}

/// GET /api/bgpls/path?from={router_id}&to={router_id}
/// Returns shortest IGP path between two router-IDs using BGP-LS link metrics.
pub async fn bgpls_path(
    State(state): State<AppState>,
    Query(params): Query<PathQuery>,
) -> Json<Value> {
    let from = params.from.unwrap_or_default();
    let to   = params.to.unwrap_or_default();
    if from.is_empty() || to.is_empty() {
        return Json(serde_json::json!({
            "error": "from and to query params required",
        }));
    }

    let graph = build_graph(&state, None);
    let path  = dijkstra_path(&graph, &from, &to);

    let found = path.is_some();
    Json(serde_json::json!({
        "from":   from,
        "to":     to,
        "path":   path.unwrap_or_default(),
        "found":  found,
    }))
}

/// Simple Dijkstra over the BGP-LS topology using igp_metric as edge weight.
fn dijkstra_path(graph: &TopologyGraph, src: &str, dst: &str) -> Option<Vec<String>> {
    use std::collections::{BinaryHeap, HashMap};
    use std::cmp::Reverse;

    // Build adjacency list from links
    let mut adj: HashMap<&str, Vec<(&str, u32)>> = HashMap::new();
    for link in &graph.links {
        let w = link.igp_metric.unwrap_or(1);
        adj.entry(&link.source).or_default().push((&link.target, w));
        adj.entry(&link.target).or_default().push((&link.source, w));
    }

    let mut dist: HashMap<&str, u32> = HashMap::new();
    let mut prev: HashMap<&str, &str> = HashMap::new();
    let mut heap: BinaryHeap<Reverse<(u32, &str)>> = BinaryHeap::new();

    dist.insert(src, 0);
    heap.push(Reverse((0, src)));

    while let Some(Reverse((cost, node))) = heap.pop() {
        if node == dst { break; }
        if cost > *dist.get(node).unwrap_or(&u32::MAX) { continue; }
        for &(neighbor, weight) in adj.get(node).unwrap_or(&vec![]) {
            let next_cost = cost + weight;
            let entry = dist.entry(neighbor).or_insert(u32::MAX);
            if next_cost < *entry {
                *entry = next_cost;
                prev.insert(neighbor, node);
                heap.push(Reverse((next_cost, neighbor)));
            }
        }
    }

    if !dist.contains_key(dst) { return None; }

    let mut path = Vec::new();
    let mut cur = dst;
    path.push(cur.to_string());
    while let Some(&p) = prev.get(cur) {
        path.push(p.to_string());
        cur = p;
        if cur == src { break; }
    }
    path.reverse();
    if path.first().map(|s| s.as_str()) == Some(src) { Some(path) } else { None }
}
