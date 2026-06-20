"""DuckDB → Parquet export pipeline for ML training (RV4-4 T1).

Usage::

    from rbmppy.parquet import export_route_features, export_peer_stability

    # Export 7-day route feature matrix
    n = export_route_features("runtime/routes.duckdb", "ml/data/routes_7d.parquet", days=7)
    print(f"Exported {n} route rows")

    # Export per-peer stability features
    n = export_peer_stability("runtime/routes.duckdb", "ml/data/peers_7d.parquet", days=7)
    print(f"Exported {n} peer rows")
"""
from __future__ import annotations

import duckdb
from pathlib import Path
from datetime import datetime, timezone, timedelta
from typing import Optional


def export_route_features(
    db_path: str,
    output: str,
    days: int = 7,
    since: Optional[datetime] = None,
) -> int:
    """Export per-prefix route event features to Parquet (zstd compressed).

    Feature columns returned:
      prefix, peer_addr, peer_as, speaker_addr, rib_type, is_announce,
      hop_count, origin_asn, local_pref, med, community_count,
      rpki_enc (valid=1, not-found=0, invalid=-1), occurred_at_s, collector_id
    """
    since_ts = since or (datetime.now(timezone.utc) - timedelta(days=days))
    Path(output).parent.mkdir(parents=True, exist_ok=True)
    conn = duckdb.connect(db_path, read_only=True)
    conn.execute(f"""
        COPY (
            SELECT
                prefix,
                peer_addr,
                CAST(peer_as AS INTEGER)          AS peer_as,
                speaker_addr,
                rib_type,
                CASE WHEN action = 'announce' THEN 1 ELSE 0 END AS is_announce,
                COALESCE(as_path_len, 0)          AS hop_count,
                TRY_CAST(
                    list_last(string_split(trim(COALESCE(as_path, '')), ' '))
                    AS INTEGER
                )                                 AS origin_asn,
                COALESCE(local_pref, 100)         AS local_pref,
                COALESCE(med, 0)                  AS med,
                CASE
                    WHEN communities IS NULL OR communities = '' THEN 0
                    ELSE len(string_split(communities, ','))
                END                               AS community_count,
                CASE rpki_validity
                    WHEN 'valid'   THEN  1
                    WHEN 'invalid' THEN -1
                    ELSE 0
                END                               AS rpki_enc,
                EPOCH(occurred_at)                AS occurred_at_s,
                collector_id
            FROM route_events
            WHERE occurred_at >= TIMESTAMPTZ '{since_ts.isoformat()}'
        ) TO '{output}' (FORMAT PARQUET, COMPRESSION 'zstd')
    """)
    row = conn.execute(
        "SELECT COUNT(*) FROM route_events "
        f"WHERE occurred_at >= TIMESTAMPTZ '{since_ts.isoformat()}'"
    ).fetchone()
    conn.close()
    return row[0] if row else 0


def export_peer_stability(
    db_path: str,
    output: str,
    days: int = 7,
) -> int:
    """Export per-peer session stability features to Parquet.

    Feature columns:
      peer_addr, peer_as, speaker_addr,
      up_count, down_count (session flaps in window),
      current_route_count, churn_events, rpki_invalid_count, last_event_s
    """
    since_ts = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()
    Path(output).parent.mkdir(parents=True, exist_ok=True)
    conn = duckdb.connect(db_path, read_only=True)
    conn.execute(f"""
        COPY (
            SELECT
                p.peer_addr,
                p.peer_as,
                p.speaker_addr,
                COUNT(CASE WHEN p.event_type = 'peer_up'   THEN 1 END) AS up_count,
                COUNT(CASE WHEN p.event_type = 'peer_down' THEN 1 END) AS down_count,
                COALESCE(r.route_count,       0)   AS current_route_count,
                COALESCE(r.churn_events,      0)   AS churn_events,
                COALESCE(r.rpki_invalid_count,0)   AS rpki_invalid_count,
                EPOCH(MAX(p.occurred_at))          AS last_event_s
            FROM peer_events p
            LEFT JOIN (
                SELECT
                    peer_addr,
                    COUNT(*) FILTER (WHERE action = 'announce')          AS route_count,
                    COUNT(*)                                              AS churn_events,
                    COUNT(*) FILTER (WHERE rpki_validity = 'invalid')    AS rpki_invalid_count
                FROM route_events
                WHERE occurred_at >= TIMESTAMPTZ '{since_ts}'
                GROUP BY peer_addr
            ) r ON p.peer_addr = r.peer_addr
            WHERE p.occurred_at >= TIMESTAMPTZ '{since_ts}'
            GROUP BY p.peer_addr, p.peer_as, p.speaker_addr,
                     r.route_count, r.churn_events, r.rpki_invalid_count
        ) TO '{output}' (FORMAT PARQUET, COMPRESSION 'zstd')
    """)
    row = conn.execute(
        "SELECT COUNT(DISTINCT peer_addr) FROM peer_events "
        f"WHERE occurred_at >= TIMESTAMPTZ '{since_ts}'"
    ).fetchone()
    conn.close()
    return row[0] if row else 0


def __main__() -> None:
    """CLI: python -m rbmppy.parquet [--db PATH] [--out DIR] [--days N]"""
    import argparse
    parser = argparse.ArgumentParser(description="Export DuckDB tables to Parquet")
    parser.add_argument("--db",   default="runtime/routes.duckdb")
    parser.add_argument("--out",  default="ml/data")
    parser.add_argument("--days", type=int, default=7)
    args = parser.parse_args()

    n = export_route_features(args.db, f"{args.out}/routes_{args.days}d.parquet", args.days)
    print(f"Exported {n} route_events rows → {args.out}/routes_{args.days}d.parquet")

    n = export_peer_stability(args.db, f"{args.out}/peers_{args.days}d.parquet", args.days)
    print(f"Exported {n} peer rows → {args.out}/peers_{args.days}d.parquet")


if __name__ == "__main__":
    __main__()
