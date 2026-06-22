"""BGP topology snapshots for STGNN training (RV4-4 T3).

Adapted from bonsai's snapshot_store.py + train_stgnn.py architecture.

A BgpTopologySnapshot represents one time-slice of the BGP peer graph:
  Nodes  = BGP speakers + peers (from bgpls_nodes + peer_events)
  Edges  = active BGP sessions (peer_events Up) + BGP-LS links
  Node features per peer:
    route_count, churn_rate_1h, rpki_invalid_ratio,
    session_uptime_secs, flap_count_24h

A sequence of T=8 snapshots (5-minute intervals) forms the temporal
input to the GATv2-GRU STGNN.

Usage::

    from rbmppy.analytics import RouteAnalytics
    from bmppy.ml.topology_snapshot import BgpTopologySnapshot, SnapshotSequence

    analytics = RouteAnalytics("runtime/routes.duckdb")
    snap = BgpTopologySnapshot.from_duckdb(analytics)
    print(snap.summary())

    # Build T=8 sequence for STGNN training
    seq = SnapshotSequence.build(analytics, T=8, interval_minutes=5)
    # Convert to PyTorch Geometric (requires torch-geometric installed)
    data_list = seq.to_pyg()
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone, timedelta
from typing import Optional

try:
    import pandas as pd
    import numpy as np
except ImportError as e:
    raise ImportError("pandas and numpy are required: pip install pandas numpy") from e

from rbmppy.analytics import RouteAnalytics


NODE_FEATURE_COLS = [
    "route_count",
    "churn_rate_1h",
    "rpki_invalid_ratio",
    "session_uptime_secs",
    "flap_count_24h",
    # RV9-D1: Path Status TLV features (RFC 9069)
    "best_count",
    "ecmp_count",
    "backup_count",
    "filtered_count",
    "nonselected_count",
    "redundancy_ratio",
]


@dataclass
class BgpTopologySnapshot:
    """Single time-slice of the BGP peer graph."""

    timestamp:     datetime
    nodes_df:      "pd.DataFrame"   # columns: peer_addr + NODE_FEATURE_COLS
    edges_df:      "pd.DataFrame"   # columns: source, target, igp_metric, adj_sid
    node_index:    dict             # peer_addr → integer node index

    @classmethod
    def from_duckdb(
        cls,
        analytics: RouteAnalytics,
        at: Optional[datetime] = None,
    ) -> "BgpTopologySnapshot":
        """Build a snapshot at *at* (default: now) from DuckDB."""
        ts  = at or datetime.now(timezone.utc)
        conn = analytics.conn

        # Node features — per-peer aggregates
        nodes_df = conn.execute(f"""
            WITH peer_base AS (
                SELECT DISTINCT peer_addr
                FROM peer_events
                WHERE occurred_at <= TIMESTAMPTZ '{ts.isoformat()}'
            ),
            route_stats AS (
                SELECT
                    peer_addr,
                    COUNT(*) FILTER (WHERE action = 'announce')           AS route_count,
                    COUNT(*) FILTER (
                        WHERE occurred_at >= TIMESTAMPTZ '{(ts - timedelta(hours=1)).isoformat()}'
                    )                                                      AS churn_count_1h,
                    COUNT(*) FILTER (WHERE rpki_validity = 'invalid')     AS rpki_invalid,
                    COUNT(*)                                               AS total_routes
                FROM route_events
                WHERE occurred_at <= TIMESTAMPTZ '{ts.isoformat()}'
                GROUP BY peer_addr
            ),
            flap_stats AS (
                SELECT
                    peer_addr,
                    COUNT(*) FILTER (WHERE event_type = 'peer_down')      AS flap_count_24h,
                    MAX(occurred_at) FILTER (WHERE event_type = 'peer_up') AS last_up
                FROM peer_events
                WHERE occurred_at BETWEEN
                    TIMESTAMPTZ '{(ts - timedelta(hours=24)).isoformat()}'
                    AND TIMESTAMPTZ '{ts.isoformat()}'
                GROUP BY peer_addr
            )
            ,
            path_status AS (
                SELECT
                    peer_addr,
                    COUNT(*) FILTER (WHERE path_status = 'best')         AS best_count,
                    COUNT(*) FILTER (WHERE path_status = 'ecmp')         AS ecmp_count,
                    COUNT(*) FILTER (WHERE path_status = 'backup')       AS backup_count,
                    COUNT(*) FILTER (WHERE path_status = 'filtered')     AS filtered_count,
                    COUNT(*) FILTER (WHERE path_status = 'nonselected')  AS nonselected_count,
                    COUNT(*)                                              AS ps_total
                FROM route_events
                WHERE action = 'announce'
                  AND occurred_at <= TIMESTAMPTZ '{ts.isoformat()}'
                  AND path_status IS NOT NULL
                GROUP BY peer_addr
            )
            SELECT
                pb.peer_addr,
                COALESCE(rs.route_count,    0)          AS route_count,
                COALESCE(rs.churn_count_1h, 0)          AS churn_rate_1h,
                CASE WHEN COALESCE(rs.total_routes,0) > 0
                     THEN rs.rpki_invalid::FLOAT / rs.total_routes
                     ELSE 0.0
                END                                     AS rpki_invalid_ratio,
                CASE WHEN fs.last_up IS NOT NULL
                     THEN EPOCH(TIMESTAMPTZ '{ts.isoformat()}') - EPOCH(fs.last_up)
                     ELSE 0.0
                END                                     AS session_uptime_secs,
                COALESCE(fs.flap_count_24h, 0)          AS flap_count_24h,
                COALESCE(ps.best_count, 0)              AS best_count,
                COALESCE(ps.ecmp_count, 0)              AS ecmp_count,
                COALESCE(ps.backup_count, 0)            AS backup_count,
                COALESCE(ps.filtered_count, 0)           AS filtered_count,
                COALESCE(ps.nonselected_count, 0)        AS nonselected_count,
                CASE WHEN COALESCE(ps.ps_total, 0) > 0
                     THEN (COALESCE(ps.ecmp_count,0) + COALESCE(ps.backup_count,0))::FLOAT
                          / ps.ps_total
                     ELSE 0.0
                END                                     AS redundancy_ratio
            FROM peer_base pb
            LEFT JOIN route_stats  rs ON pb.peer_addr = rs.peer_addr
            LEFT JOIN flap_stats   fs ON pb.peer_addr = fs.peer_addr
            LEFT JOIN path_status  ps ON pb.peer_addr = ps.peer_addr
        """).df()

        # Build node index
        node_index = {addr: i for i, addr in enumerate(nodes_df["peer_addr"])}

        # Edges — from bgpls_links if available
        edges_df = pd.DataFrame(columns=["source", "target", "igp_metric", "adj_sid"])
        try:
            table_exists = conn.execute(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'bgpls_links'"
            ).fetchone()[0]
            if table_exists:
                edges_df = conn.execute(f"""
                    SELECT local_router_id AS source, remote_router_id AS target,
                           igp_metric, adj_sid_labels AS adj_sid
                    FROM (
                        SELECT *, ROW_NUMBER() OVER (
                            PARTITION BY local_router_id, remote_router_id
                            ORDER BY occurred_at DESC
                        ) AS rn
                        FROM bgpls_links
                        WHERE action = 'announce'
                          AND occurred_at <= TIMESTAMPTZ '{ts.isoformat()}'
                    ) WHERE rn = 1
                """).df()
        except Exception:
            pass

        return cls(
            timestamp=ts,
            nodes_df=nodes_df,
            edges_df=edges_df,
            node_index=node_index,
        )

    def feature_matrix(self) -> "np.ndarray":
        """Node feature matrix X of shape (N, len(NODE_FEATURE_COLS))."""
        return self.nodes_df[NODE_FEATURE_COLS].fillna(0).to_numpy(dtype=np.float32)

    def edge_index(self) -> "np.ndarray":
        """COO edge index of shape (2, E) using integer node indices."""
        if self.edges_df.empty:
            return np.zeros((2, 0), dtype=np.int64)
        src = self.edges_df["source"].map(self.node_index).dropna().astype(int)
        dst = self.edges_df["target"].map(self.node_index).dropna().astype(int)
        valid = src.notna() & dst.notna()
        return np.stack([src[valid].to_numpy(), dst[valid].to_numpy()])

    def summary(self) -> str:
        return (
            f"BgpTopologySnapshot @ {self.timestamp.isoformat()} | "
            f"{len(self.nodes_df)} nodes | {len(self.edges_df)} edges"
        )

    def to_pyg(self) -> object:
        """Convert to PyTorch Geometric Data object (requires torch-geometric)."""
        try:
            import torch
            from torch_geometric.data import Data
        except ImportError as e:
            raise ImportError(
                "torch and torch-geometric required: pip install torch torch-geometric"
            ) from e
        x      = torch.tensor(self.feature_matrix(), dtype=torch.float)
        ei     = torch.tensor(self.edge_index(), dtype=torch.long)
        return Data(x=x, edge_index=ei, num_nodes=len(self.nodes_df))


@dataclass
class SnapshotSequence:
    """Sequence of T snapshots for STGNN temporal input."""

    snapshots: list[BgpTopologySnapshot]
    T: int

    @classmethod
    def build(
        cls,
        analytics: RouteAnalytics,
        T: int = 8,
        interval_minutes: int = 5,
    ) -> "SnapshotSequence":
        """Build T evenly-spaced snapshots ending at now."""
        end = datetime.now(timezone.utc)
        timestamps = [
            end - timedelta(minutes=interval_minutes * (T - 1 - i))
            for i in range(T)
        ]
        snaps = [BgpTopologySnapshot.from_duckdb(analytics, at=t) for t in timestamps]
        return cls(snapshots=snaps, T=T)

    def to_pyg(self) -> list:
        """List of PyG Data objects — one per time step."""
        return [s.to_pyg() for s in self.snapshots]

    def save_arrow(self, path: str) -> None:
        """Save feature matrices as Arrow IPC for offline training."""
        try:
            import pyarrow as pa
            import pyarrow.ipc as ipc
        except ImportError as e:
            raise ImportError("pyarrow required: pip install pyarrow") from e

        import pathlib
        pathlib.Path(path).parent.mkdir(parents=True, exist_ok=True)
        arrays = []
        for i, snap in enumerate(self.snapshots):
            fm = snap.feature_matrix()
            arrays.append(pa.array(fm.flatten().tolist()))

        schema = pa.schema([
            pa.field(f"t{i}_{col}", pa.float32())
            for i in range(self.T)
            for col in NODE_FEATURE_COLS
        ])
        with pa.ipc.new_file(path, schema) as writer:
            batch = pa.record_batch(arrays, schema=schema)
            writer.write(batch)
