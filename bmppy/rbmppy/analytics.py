"""
Advanced BGP analytics on top of rustybmp's DuckDB store.
Connect via DuckDB file path or rustybmp REST API.
"""
import duckdb
from dataclasses import dataclass
from typing import Optional
import pandas as pd


class RouteAnalytics:
    """Read-only analytics over the rustybmp DuckDB store."""

    def __init__(self, db_path: str):
        self.conn = duckdb.connect(db_path, read_only=True)

    def current_rib(self, peer_addr: Optional[str] = None) -> pd.DataFrame:
        """Return the current RIB (latest state per prefix).

        Correctly handles withdrawn prefixes: a prefix whose most recent event
        is a 'withdraw' will NOT appear in the result.
        """
        peer_filter = f"AND peer_addr = '{peer_addr}'" if peer_addr else ""
        return self.conn.execute(f"""
            SELECT * FROM (
                SELECT *, ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
                FROM route_events
                WHERE 1=1 {peer_filter}
            ) WHERE rn = 1 AND action = 'announce'
        """).df()

    def prefix_history(self, prefix: str) -> pd.DataFrame:
        """Full announce/withdraw history for a specific prefix."""
        return self.conn.execute(
            "SELECT * FROM route_events WHERE prefix = ? ORDER BY occurred_at DESC",
            [prefix]
        ).df()

    def churn_analysis(self, top_n: int = 50) -> pd.DataFrame:
        """Top N most frequently changing prefixes."""
        return self.conn.execute(f"""
            SELECT prefix, COUNT(*) AS events,
                   SUM(CASE WHEN action='announce' THEN 1 ELSE 0 END) AS announces,
                   SUM(CASE WHEN action='withdraw' THEN 1 ELSE 0 END) AS withdraws,
                   MIN(occurred_at) AS first_seen, MAX(occurred_at) AS last_seen
            FROM route_events
            GROUP BY prefix ORDER BY events DESC LIMIT {top_n}
        """).df()

    def as_path_analysis(self) -> pd.DataFrame:
        """AS path length distribution and prepending detection."""
        return self.conn.execute("""
            SELECT as_path_len,
                   COUNT(*) AS count,
                   COUNT(DISTINCT prefix) AS unique_prefixes
            FROM route_events
            WHERE action = 'announce' AND as_path_len IS NOT NULL
            GROUP BY as_path_len ORDER BY as_path_len
        """).df()

    def community_usage(self, top_n: int = 50) -> pd.DataFrame:
        """Most common BGP communities across all routes."""
        return self.conn.execute(f"""
            SELECT unnest(string_split(communities, ',')) AS community, COUNT(*) AS count
            FROM route_events
            WHERE communities IS NOT NULL AND communities <> ''
            GROUP BY community ORDER BY count DESC LIMIT {top_n}
        """).df()

    def peer_flap_timeline(self) -> pd.DataFrame:
        """Peer up/down events timeline."""
        return self.conn.execute("""
            SELECT occurred_at, speaker_addr, peer_addr, peer_as, event_type, reason
            FROM peer_events ORDER BY occurred_at DESC LIMIT 1000
        """).df()

    def route_visibility(self, prefix: str) -> pd.DataFrame:
        """Across how many peers is this prefix visible."""
        return self.conn.execute(
            """SELECT peer_addr, MAX(occurred_at) AS last_seen, action
               FROM route_events WHERE prefix = ?
               GROUP BY peer_addr, action ORDER BY last_seen DESC""",
            [prefix]
        ).df()

    def close(self):
        self.conn.close()
