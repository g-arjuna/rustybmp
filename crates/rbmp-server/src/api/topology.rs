/// BGP-LS topology graph API (RV4-6 / RV4-3 T4).
///
/// GET /api/bgpls/graph?protocol=isis
///
/// Returns the current BGP-LS node and link graph as JSON suitable for
/// D3.js force-directed rendering.  Reads from DuckDB bgpls_nodes / bgpls_links.
/// If those tables don't exist yet, returns an empty graph.
use axum::{extract::{Query, State}, Json};
use serde::{Deserialize, Serialize};

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
