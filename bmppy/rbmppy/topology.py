"""BGP-LS topology graph via Python NetworkX (RV4-6).

Derives an in-memory graph from DuckDB bgpls_nodes / bgpls_links tables.
No graph database required — NetworkX handles path computation and
blast-radius analysis efficiently for typical BGP-LS topologies (<10K nodes).

Usage::

    from rbmppy.analytics import RouteAnalytics
    from rbmppy.topology import BgpLsTopology, AsTopology

    analytics = RouteAnalytics("runtime/routes.duckdb")
    topo = BgpLsTopology(analytics)

    path  = topo.shortest_path("10.0.0.1", "10.0.0.5")
    blast = topo.blast_radius("10.0.0.3", max_hops=3)
    print(topo.to_dict())          # JSON-serialisable for /api/bgpls/graph

    as_topo = AsTopology(analytics)
    neighbors = as_topo.neighbors(64496)
    print("transit?", as_topo.is_transit(64496))
"""
from __future__ import annotations

from typing import Optional

try:
    import networkx as nx
    import pandas as pd
except ImportError as e:
    raise ImportError(
        "networkx and pandas are required for topology analysis. "
        "Install with: pip install networkx pandas"
    ) from e

from .analytics import RouteAnalytics


class BgpLsTopology:
    """IGP topology graph from BGP-LS data stored in DuckDB.

    Nodes  = routers (bgpls_nodes: router_id, node_name, protocol).
    Edges  = links   (bgpls_links: igp_metric, max_bandwidth, adj_sid_labels, srlg_groups).

    The graph is built once on instantiation.  Call rebuild() to refresh.
    """

    def __init__(self, analytics: RouteAnalytics) -> None:
        self.G: nx.DiGraph = nx.DiGraph()
        self._analytics = analytics
        self._load()

    def _load(self) -> None:
        conn = self._analytics.conn

        # Check tables exist before querying
        tables = {
            row[0]
            for row in conn.execute(
                "SELECT table_name FROM information_schema.tables "
                "WHERE table_name IN ('bgpls_nodes','bgpls_links')"
            ).fetchall()
        }

        if "bgpls_nodes" in tables:
            nodes_df = conn.execute(
                "SELECT DISTINCT router_id, node_name, protocol_id "
                "FROM bgpls_nodes WHERE action = 'announce'"
            ).df()
            for _, row in nodes_df.iterrows():
                self.G.add_node(
                    row["router_id"],
                    name=row.get("node_name"),
                    protocol=row.get("protocol_id"),
                )

        if "bgpls_links" in tables:
            links_df = conn.execute(
                """SELECT local_router_id, remote_router_id,
                          local_ip, remote_ip,
                          igp_metric, max_bandwidth, adj_sid_labels, srlg_groups
                   FROM (
                     SELECT *, ROW_NUMBER() OVER (
                         PARTITION BY local_router_id, remote_router_id
                         ORDER BY occurred_at DESC
                     ) AS rn
                     FROM bgpls_links WHERE action = 'announce'
                   ) WHERE rn = 1"""
            ).df()
            for _, row in links_df.iterrows():
                src, dst = row["local_router_id"], row["remote_router_id"]
                if src and dst:
                    self.G.add_edge(
                        src, dst,
                        igp_metric=int(row["igp_metric"]) if pd.notna(row.get("igp_metric")) else 1,
                        max_bandwidth=row.get("max_bandwidth"),
                        local_ip=row.get("local_ip"),
                        remote_ip=row.get("remote_ip"),
                        adj_sids=row.get("adj_sid_labels"),
                        srlg=row.get("srlg_groups"),
                    )

    def rebuild(self) -> None:
        """Reload graph from DuckDB (call after new BGP-LS data arrives)."""
        self.G.clear()
        self._load()

    def shortest_path(self, src: str, dst: str) -> list[str]:
        """Shortest path between two routers by IGP metric."""
        try:
            return nx.shortest_path(self.G, src, dst, weight="igp_metric")
        except (nx.NetworkXNoPath, nx.NodeNotFound):
            return []

    def blast_radius(self, node: str, max_hops: int = 3) -> set[str]:
        """All routers reachable from *node* within *max_hops* hops."""
        try:
            return (
                set(nx.single_source_shortest_path_length(self.G, node, cutoff=max_hops).keys())
                - {node}
            )
        except nx.NodeNotFound:
            return set()

    def srlg_diverse_paths(self, src: str, dst: str, n: int = 2) -> list[list[str]]:
        """Return up to *n* SRLG-diverse simple paths (min shared SRLG groups)."""
        try:
            all_paths = list(nx.all_simple_paths(self.G, src, dst, cutoff=10))
        except (nx.NetworkXNoPath, nx.NodeNotFound):
            return []
        if len(all_paths) <= 1:
            return all_paths

        def path_srlg(path: list[str]) -> set[str]:
            srlgs: set[str] = set()
            for u, v in zip(path, path[1:]):
                data = self.G.get_edge_data(u, v, {})
                for s in str(data.get("srlg") or "").split(","):
                    if s.strip():
                        srlgs.add(s.strip())
            return srlgs

        chosen = [all_paths[0]]
        for path in all_paths[1:]:
            if not (path_srlg(path) & path_srlg(chosen[0])):
                chosen.append(path)
            if len(chosen) >= n:
                break
        return chosen[:n]

    def to_dict(self) -> dict:
        """Serialize to dict for JSON API / D3.js consumption."""
        return {
            "nodes": [
                {"id": n, "name": d.get("name"), "protocol": d.get("protocol")}
                for n, d in self.G.nodes(data=True)
            ],
            "links": [
                {
                    "source": u,
                    "target": v,
                    "igp_metric": d.get("igp_metric"),
                    "local_ip": d.get("local_ip"),
                    "remote_ip": d.get("remote_ip"),
                    "adj_sids": d.get("adj_sids"),
                }
                for u, v, d in self.G.edges(data=True)
            ],
        }


class AsTopology:
    """AS-level topology derived from BGP AS_PATH data.

    Nodes = ASNs seen in AS_PATHs.
    Edges = AS adjacency inferred from consecutive hops in AS_PATHs.

    Useful for: peer-relationship inference, transit AS identification,
    and AS-level blast-radius analysis.
    """

    def __init__(self, analytics: RouteAnalytics, days: int = 1, limit: int = 50_000) -> None:
        self.G: nx.DiGraph = nx.DiGraph()
        self._load(analytics, days, limit)

    def _load(self, analytics: RouteAnalytics, days: int, limit: int) -> None:
        df = analytics.conn.execute(
            f"""SELECT as_path FROM route_events
                WHERE action = 'announce' AND as_path IS NOT NULL AND as_path <> ''
                  AND occurred_at >= NOW() - INTERVAL '{days}' DAY
                LIMIT {limit}"""
        ).df()
        for _, row in df.iterrows():
            asns = [int(a) for a in str(row["as_path"]).split() if a.isdigit()]
            for i in range(len(asns) - 1):
                self.G.add_edge(asns[i], asns[i + 1])

    def neighbors(self, asn: int) -> list[int]:
        """Direct AS neighbours (both upstream and downstream)."""
        return list(self.G.successors(asn)) + list(self.G.predecessors(asn))

    def is_transit(self, asn: int) -> bool:
        """True if the ASN appears as an intermediate hop (has both in- and out-edges)."""
        return self.G.in_degree(asn) > 0 and self.G.out_degree(asn) > 0

    def top_transit_asns(self, n: int = 20) -> list[tuple[int, int]]:
        """Top-N transit ASNs by number of paths passing through them."""
        return sorted(
            ((asn, self.G.in_degree(asn) + self.G.out_degree(asn))
             for asn in self.G.nodes()),
            key=lambda x: x[1],
            reverse=True,
        )[:n]
