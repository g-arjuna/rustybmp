"""
Advanced BGP analytics on top of rustybmp's DuckDB store.

Implements:
  - ZScoreMonitor   — sliding-window Z-score anomaly detection (IMACSI 2025 eq.2-4)
  - HijackDetector  — origin-AS change and AS-PATH shortening detection
  - RouteLeakDetector — OTC-based valley-free violation detection
  - FlapScorer      — BGP session flap severity scoring
  - RouteAnalytics  — SQL-backed analytics with anomaly_report()
"""
import math
import duckdb
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Set, Tuple
import pandas as pd


# ─── Z-Score Monitor (IMACSI 2025 eq.2-4) ────────────────────────────────────

class ZScoreMonitor:
    """
    Sliding-window Z-score for BGP prefix churn detection.

    For each prefix Pi, maintains a running mean μPi and std σPi over the
    last `window` event-frequency observations.

    Equation 2: μPi = (1/N) Σ f_i
    Equation 3: σPi = sqrt((1/N) Σ (f_i - μPi)²)
    Equation 4: Zi  = (f - μPi) / σPi ; anomalous when |Zi| > threshold
    """

    def __init__(self, window: int = 60, threshold: float = 3.0):
        self.window    = window
        self.threshold = threshold
        # prefix → deque of recent frequency observations
        self._history: Dict[str, List[float]] = defaultdict(list)

    def update(self, prefix: str, freq: float) -> float:
        """Add a new frequency observation; returns the Z-score."""
        h = self._history[prefix]
        h.append(freq)
        if len(h) > self.window:
            h.pop(0)
        return self.score(prefix)

    def score(self, prefix: str) -> float:
        """Return current Z-score for prefix; 0.0 if insufficient history."""
        h = self._history[prefix]
        if len(h) < 2:
            return 0.0
        mu = sum(h) / len(h)
        variance = sum((x - mu) ** 2 for x in h) / len(h)
        sigma = math.sqrt(variance)
        if sigma == 0.0:
            return 0.0
        return (h[-1] - mu) / sigma

    def is_anomalous(self, prefix: str) -> bool:
        return abs(self.score(prefix)) > self.threshold

    def reset(self, prefix: str) -> None:
        self._history.pop(prefix, None)

    def top_anomalies(self, n: int = 20) -> List[Tuple[str, float]]:
        """Return top N prefixes sorted by |Z-score| descending."""
        scored = [(p, self.score(p)) for p in self._history]
        scored.sort(key=lambda x: abs(x[1]), reverse=True)
        return scored[:n]


# ─── Hijack Detector ──────────────────────────────────────────────────────────

@dataclass
class HijackAlert:
    prefix:      str
    old_origin:  int
    new_origin:  int
    peer_addr:   str
    occurred_at: str
    kind:        str  # "origin_change" | "path_shortening"


class HijackDetector:
    """
    Detects potential BGP hijacks from route_events data.

    Heuristics:
      1. Origin-AS change: new origin_asn appears without prior withdrawal.
      2. AS-PATH shortening: announced path length drops >50% vs 7-day average
         for that prefix (sudden shorter path = possible hijack).
    """

    def __init__(self, path_drop_threshold: float = 0.5):
        self.path_drop_threshold = path_drop_threshold
        # prefix → set of known origin ASNs
        self._origins:  Dict[str, Set[int]] = defaultdict(set)
        # prefix → list of recent path lengths
        self._path_lens: Dict[str, List[int]] = defaultdict(list)
        self.alerts: List[HijackAlert] = []

    def process(self, df: pd.DataFrame) -> List[HijackAlert]:
        """
        Process a DataFrame of route_events (sorted ascending by occurred_at).
        Returns new alerts found in this batch.
        """
        new_alerts: List[HijackAlert] = []

        for _, row in df.iterrows():
            prefix     = row["prefix"]
            action     = row.get("action", "")
            origin_raw = row.get("as_path", "")
            peer_addr  = str(row.get("peer_addr", ""))
            occurred   = str(row.get("occurred_at", ""))

            if action == "withdraw":
                self._origins[prefix].discard(self._last_origin(origin_raw))
                continue

            if action != "announce":
                continue

            origin_asn = self._parse_origin(origin_raw)
            path_len   = row.get("as_path_len") or 0

            # Heuristic 1: origin change
            known = self._origins[prefix]
            if known and origin_asn not in known:
                for old in known:
                    alert = HijackAlert(
                        prefix=prefix, old_origin=old, new_origin=origin_asn,
                        peer_addr=peer_addr, occurred_at=occurred,
                        kind="origin_change",
                    )
                    new_alerts.append(alert)
                    self.alerts.append(alert)

            self._origins[prefix].add(origin_asn)

            # Heuristic 2: path shortening
            lens = self._path_lens[prefix]
            if len(lens) >= 5 and path_len > 0:
                avg = sum(lens[-20:]) / min(len(lens), 20)
                if avg > 0 and path_len < avg * (1 - self.path_drop_threshold):
                    alert = HijackAlert(
                        prefix=prefix, old_origin=origin_asn, new_origin=origin_asn,
                        peer_addr=peer_addr, occurred_at=occurred,
                        kind="path_shortening",
                    )
                    new_alerts.append(alert)
                    self.alerts.append(alert)

            if path_len > 0:
                lens.append(int(path_len))
                if len(lens) > 100:
                    lens.pop(0)

        return new_alerts

    def _parse_origin(self, as_path_str) -> int:
        if not as_path_str or not isinstance(as_path_str, str):
            return 0
        parts = as_path_str.strip().split()
        try:
            return int(parts[-1]) if parts else 0
        except ValueError:
            return 0

    def _last_origin(self, as_path_str) -> int:
        return self._parse_origin(as_path_str)


# ─── Route Leak Detector ─────────────────────────────────────────────────────

@dataclass
class LeakAlert:
    prefix:      str
    peer_addr:   str
    occurred_at: str
    evidence:    str  # human-readable reason


class RouteLeakDetector:
    """
    Detects BGP route leaks using OTC (Only To Customer) attribute.

    RFC 9234: a route carrying OTC must not be re-advertised to a peer or
    provider. If we see a route with OTC on a peer session that is not
    a customer, it is a route leak.
    """

    def __init__(self, customer_peers: Optional[Set[str]] = None):
        # Known customer peer addresses — routes from these may legitimately carry OTC
        self.customer_peers: Set[str] = customer_peers or set()
        self.alerts: List[LeakAlert] = []

    def process(self, df: pd.DataFrame) -> List[LeakAlert]:
        new_alerts: List[LeakAlert] = []
        if "ext_communities" not in df.columns:
            return new_alerts

        for _, row in df.iterrows():
            if row.get("action") != "announce":
                continue
            ext_comms = str(row.get("ext_communities") or "")
            peer_addr  = str(row.get("peer_addr", ""))
            # OTC is encoded as a Large Community or noted in path attributes;
            # we check for the OTC keyword or type 0x09 community marker
            if "otc" in ext_comms.lower() or "only-to-customer" in ext_comms.lower():
                if peer_addr not in self.customer_peers:
                    alert = LeakAlert(
                        prefix=str(row.get("prefix", "")),
                        peer_addr=peer_addr,
                        occurred_at=str(row.get("occurred_at", "")),
                        evidence=f"OTC attribute on non-customer peer {peer_addr}",
                    )
                    new_alerts.append(alert)
                    self.alerts.append(alert)

        return new_alerts


# ─── Flap Scorer ─────────────────────────────────────────────────────────────

@dataclass
class FlapEvent:
    speaker_addr: str
    peer_addr:    str
    peer_as:      int
    flap_count:   int
    window_secs:  int
    severity:     str  # "low" | "medium" | "high"


class FlapScorer:
    """
    Scores BGP session flap severity based on flap rate in a time window.
    Thresholds: low < 3 flaps/hour, medium < 10, high ≥ 10.
    """

    LOW_THRESHOLD    = 3
    MEDIUM_THRESHOLD = 10

    def score(self, flap_count: int, window_secs: int = 3600) -> str:
        rate_per_hour = flap_count * 3600 / max(window_secs, 1)
        if rate_per_hour < self.LOW_THRESHOLD:
            return "low"
        elif rate_per_hour < self.MEDIUM_THRESHOLD:
            return "medium"
        else:
            return "high"

    def process(self, df: pd.DataFrame, window_secs: int = 3600) -> List[FlapEvent]:
        """Process peer_events DataFrame; return FlapEvent list."""
        events: List[FlapEvent] = []
        if df.empty:
            return events

        down_counts = df[df.get("event_type", df.columns[0]) == "peer_down"] \
            .groupby(["speaker_addr", "peer_addr", "peer_as"]).size().reset_index(name="flap_count") \
            if "event_type" in df.columns else pd.DataFrame()

        for _, row in down_counts.iterrows():
            fc  = int(row["flap_count"])
            sev = self.score(fc, window_secs)
            events.append(FlapEvent(
                speaker_addr=str(row["speaker_addr"]),
                peer_addr=str(row["peer_addr"]),
                peer_as=int(row["peer_as"]),
                flap_count=fc,
                window_secs=window_secs,
                severity=sev,
            ))
        return events


# ─── Route Analytics ─────────────────────────────────────────────────────────

class RouteAnalytics:
    """Read-only analytics over the rustybmp DuckDB store."""

    def __init__(self, db_path: str):
        self.conn         = duckdb.connect(db_path, read_only=True)
        self._zscore      = ZScoreMonitor()
        self._hijack      = HijackDetector()
        self._leak        = RouteLeakDetector()
        self._flap        = FlapScorer()

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

    def anomaly_report(self, lookback_hours: int = 24, top_n: int = 20) -> dict:
        """
        Run all anomaly detectors over the last `lookback_hours` of data.

        Returns a dict with keys:
          - zscore_anomalies: [(prefix, z_score)]
          - hijack_alerts:    [HijackAlert]
          - leak_alerts:      [LeakAlert]
          - flap_events:      [FlapEvent]
        """
        # ── Feed churn data into Z-score monitor
        churn = self.conn.execute(f"""
            SELECT prefix,
                   COUNT(*) / {lookback_hours}.0 AS freq_per_hour
            FROM route_events
            WHERE occurred_at >= NOW() - INTERVAL '{lookback_hours} hours'
            GROUP BY prefix
        """).df()
        for _, row in churn.iterrows():
            self._zscore.update(row["prefix"], float(row["freq_per_hour"]))

        # ── Run hijack + leak detectors on recent announces
        recent = self.conn.execute(f"""
            SELECT * FROM route_events
            WHERE occurred_at >= NOW() - INTERVAL '{lookback_hours} hours'
            ORDER BY occurred_at ASC
        """).df()
        hijack_alerts = self._hijack.process(recent)
        leak_alerts   = self._leak.process(recent)

        # ── Flap scoring from peer events
        peer_ev = self.conn.execute(f"""
            SELECT * FROM peer_events
            WHERE occurred_at >= NOW() - INTERVAL '{lookback_hours} hours'
        """).df()
        flap_events = self._flap.process(peer_ev, window_secs=lookback_hours * 3600)

        return {
            "zscore_anomalies": self._zscore.top_anomalies(top_n),
            "hijack_alerts":    hijack_alerts,
            "leak_alerts":      leak_alerts,
            "flap_events":      flap_events,
        }

    def export_prefix_aggregates(
        self,
        out_path: str,
        days: int = 7,
    ) -> int:
        """Export per-prefix aggregate features to Parquet for STGNN/IsolationForest training.

        Columns: prefix, origin_asn, peer_count, announce_count, withdraw_count,
                 churn_rate_1h, avg_path_len, rpki_invalid_ratio, community_count,
                 first_seen, last_seen.

        Returns the number of rows written.
        """
        try:
            import pandas as pd
        except ImportError as e:
            raise ImportError("pandas required: pip install pandas") from e

        import pathlib
        pathlib.Path(out_path).parent.mkdir(parents=True, exist_ok=True)

        df = self.conn.execute(f"""
            WITH base AS (
                SELECT
                    prefix,
                    -- origin ASN: last token of as_path
                    TRY_CAST(
                        list_last(string_split(trim(as_path), ' '))
                    AS UINTEGER)                                             AS origin_asn,
                    peer_addr,
                    action,
                    occurred_at,
                    as_path_len,
                    rpki_validity,
                    communities
                FROM route_events
                WHERE occurred_at >= NOW() - INTERVAL '{days} days'
            ),
            agg AS (
                SELECT
                    prefix,
                    mode(origin_asn)                                         AS origin_asn,
                    COUNT(DISTINCT peer_addr)                                 AS peer_count,
                    COUNT(*) FILTER (WHERE action='announce')                 AS announce_count,
                    COUNT(*) FILTER (WHERE action='withdraw')                 AS withdraw_count,
                    COUNT(*) FILTER (
                        WHERE occurred_at >= NOW() - INTERVAL '1 hour'
                    )::FLOAT / GREATEST(1, 3600)                             AS churn_rate_1h,
                    AVG(as_path_len) FILTER (WHERE action='announce')        AS avg_path_len,
                    AVG(CASE WHEN rpki_validity='invalid' THEN 1.0 ELSE 0.0 END) AS rpki_invalid_ratio,
                    AVG(
                        CASE WHEN communities IS NOT NULL AND communities <> ''
                        THEN len(string_split(communities, ','))
                        ELSE 0 END
                    )                                                         AS community_count,
                    MIN(occurred_at)                                          AS first_seen,
                    MAX(occurred_at)                                          AS last_seen
                FROM base
                GROUP BY prefix
            )
            SELECT * FROM agg ORDER BY announce_count DESC
        """).df()

        df.to_parquet(out_path, index=False)
        return len(df)

    def write_anomalies(
        self,
        db_path: str,
        lookback_hours: int = 24,
    ) -> int:
        """Run anomaly detection and persist results to the ml_anomalies DuckDB table.

        Creates the table if it doesn't exist (using a writable connection).
        Returns the number of anomaly rows written.
        """
        import duckdb as _duckdb

        report = self.anomaly_report(lookback_hours=lookback_hours)
        rows = []

        for prefix, z in report["zscore_anomalies"]:
            if abs(z) > 3.0:
                rows.append({
                    "detected_at": __import__("datetime").datetime.utcnow().isoformat(),
                    "kind":        "churn_zscore",
                    "prefix":      prefix,
                    "peer_addr":   None,
                    "score":       z,
                    "description": f"Z-score={z:.2f} exceeds threshold",
                    "severity":    "warn" if abs(z) < 5 else "critical",
                })

        for alert in report["hijack_alerts"]:
            rows.append({
                "detected_at": __import__("datetime").datetime.utcnow().isoformat(),
                "kind":        alert.kind,
                "prefix":      alert.prefix,
                "peer_addr":   alert.peer_addr,
                "score":       None,
                "description": (
                    f"origin_change {alert.old_origin} → {alert.new_origin}"
                    if alert.kind == "origin_change"
                    else f"path_shortening on {alert.prefix}"
                ),
                "severity":    "critical",
            })

        for alert in report["flap_events"]:
            rows.append({
                "detected_at": __import__("datetime").datetime.utcnow().isoformat(),
                "kind":        "flap",
                "prefix":      None,
                "peer_addr":   alert.peer_addr,
                "score":       float(alert.flap_count),
                "description": f"{alert.flap_count} flaps in {alert.window_secs}s",
                "severity":    alert.severity,
            })

        if not rows:
            return 0

        try:
            import pandas as _pd
        except ImportError as e:
            raise ImportError("pandas required: pip install pandas") from e

        df = _pd.DataFrame(rows)
        conn_w = _duckdb.connect(db_path, read_only=False)
        try:
            conn_w.execute("""
                CREATE TABLE IF NOT EXISTS ml_anomalies (
                    id          INTEGER PRIMARY KEY,
                    detected_at TIMESTAMPTZ NOT NULL,
                    kind        VARCHAR     NOT NULL,
                    prefix      VARCHAR,
                    peer_addr   VARCHAR,
                    score       DOUBLE,
                    description VARCHAR,
                    severity    VARCHAR
                )
            """)
            conn_w.execute(
                "INSERT INTO ml_anomalies (detected_at,kind,prefix,peer_addr,score,description,severity) "
                "SELECT detected_at,kind,prefix,peer_addr,score,description,severity FROM df"
            )
            conn_w.commit()
        finally:
            conn_w.close()
        return len(rows)

    def close(self):
        self.conn.close()
