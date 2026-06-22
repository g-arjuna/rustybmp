"""
Bundle D4 — Convergence Anomaly Detector.

Fires ``slow_convergence`` anomaly when a new convergence event exceeds
3× the rolling 7-day P50 for that peer.

Designed to be called from ``ConvergenceDetector._finalize()`` after each
completed convergence episode.

Usage::

    from rbmppy.convergence_anomaly_detector import ConvergenceAnomalyDetector

    cad = ConvergenceAnomalyDetector(db_url="http://localhost:7878")
    anomaly = cad.evaluate(convergence_event)
    if anomaly:
        print(f"Slow convergence: {anomaly}")
"""
from __future__ import annotations

import logging
from collections import defaultdict, deque
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from typing import Optional

logger = logging.getLogger(__name__)

UTC = timezone.utc

# Rolling window: keep up to 7 days of convergence_ms values per peer
_ROLLING_WINDOW_DAYS = 7
_MAX_HISTORY = 2000  # cap per peer
_SLOW_MULTIPLIER = 3.0  # 3× P50 threshold


@dataclass
class ConvergenceAnomaly:
    """A slow-convergence anomaly alert."""

    peer_addr: str
    convergence_ms: float
    p50_ms: float
    ratio: float
    occurred_at: datetime
    kind: str = "slow_convergence"
    severity: str = "warn"


class ConvergenceAnomalyDetector:
    """Detects slow convergence by comparing to rolling 7-day P50.

    Parameters
    ----------
    multiplier:
        How many times the P50 the current event must exceed to fire.
    min_history:
        Minimum number of historical events required before evaluation.
    on_anomaly:
        Optional callback invoked with each :class:`ConvergenceAnomaly`.
    """

    def __init__(
        self,
        multiplier: float = _SLOW_MULTIPLIER,
        min_history: int = 3,
        on_anomaly: Optional[callable] = None,
    ) -> None:
        self._multiplier = multiplier
        self._min_history = min_history
        self._on_anomaly = on_anomaly

        # peer_addr → deque of (occurred_at, convergence_ms)
        self._history: dict[str, deque] = defaultdict(lambda: deque(maxlen=_MAX_HISTORY))

        self._anomaly_count = 0

    @property
    def anomaly_count(self) -> int:
        return self._anomaly_count

    def evaluate(
        self,
        peer_addr: str,
        convergence_ms: float,
        occurred_at: Optional[datetime] = None,
    ) -> Optional[ConvergenceAnomaly]:
        """Evaluate a convergence event against the rolling P50.

        Returns a :class:`ConvergenceAnomaly` if the event is anomalous,
        or ``None`` if within normal range.
        """
        ts = occurred_at or datetime.now(UTC)
        dq = self._history[peer_addr]

        # Evict entries older than 7 days
        cutoff = ts - timedelta(days=_ROLLING_WINDOW_DAYS)
        while dq and dq[0][0] < cutoff:
            dq.popleft()

        # Need enough history to compute P50
        if len(dq) < self._min_history:
            dq.append((ts, convergence_ms))
            return None

        # Compute P50 (median) of historical values
        values = sorted(v for _, v in dq)
        mid = len(values) // 2
        p50 = values[mid] if len(values) % 2 else (values[mid - 1] + values[mid]) / 2.0

        # Record current event
        dq.append((ts, convergence_ms))

        if p50 <= 0:
            return None

        ratio = convergence_ms / p50

        if ratio >= self._multiplier:
            severity = "critical" if ratio >= self._multiplier * 2 else "warn"
            anomaly = ConvergenceAnomaly(
                peer_addr=peer_addr,
                convergence_ms=convergence_ms,
                p50_ms=p50,
                ratio=ratio,
                occurred_at=ts,
                severity=severity,
            )
            self._anomaly_count += 1
            logger.info(
                "Slow convergence: peer=%s ms=%.0f p50=%.0f ratio=%.1fx",
                peer_addr, convergence_ms, p50, ratio,
            )

            if self._on_anomaly:
                try:
                    self._on_anomaly(anomaly)
                except Exception as exc:
                    logger.warning("on_anomaly callback error: %s", exc)

            return anomaly

        return None

    def history_count(self, peer_addr: str) -> int:
        """Number of convergence events in history for a peer."""
        return len(self._history.get(peer_addr, []))
