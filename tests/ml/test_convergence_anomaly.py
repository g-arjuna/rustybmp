"""
Bundle D4 — ConvergenceAnomalyDetector unit tests.
"""
from __future__ import annotations

from datetime import datetime, timedelta, timezone

import pytest

from bmppy.rbmppy.convergence_anomaly_detector import (
    ConvergenceAnomalyDetector,
    ConvergenceAnomaly,
)

UTC = timezone.utc
T0 = datetime(2025, 6, 1, 12, 0, 0, tzinfo=UTC)


class TestConvergenceAnomalyDetector:
    @pytest.fixture
    def detector(self) -> ConvergenceAnomalyDetector:
        return ConvergenceAnomalyDetector(multiplier=3.0, min_history=3)

    def test_no_anomaly_without_history(self, detector: ConvergenceAnomalyDetector) -> None:
        """First events build history, no anomaly fires."""
        result = detector.evaluate("10.0.0.1", 1000.0, T0)
        assert result is None

    def test_no_anomaly_within_normal(self, detector: ConvergenceAnomalyDetector) -> None:
        """Events within 3× P50 should not fire."""
        peer = "10.0.0.1"
        for i in range(5):
            detector.evaluate(peer, 1000.0, T0 + timedelta(hours=i))
        # 1500ms is 1.5× P50 of 1000 — should NOT fire
        result = detector.evaluate(peer, 1500.0, T0 + timedelta(hours=6))
        assert result is None

    def test_anomaly_fires_above_3x_p50(self, detector: ConvergenceAnomalyDetector) -> None:
        """Events exceeding 3× P50 should fire."""
        peer = "10.0.0.1"
        for i in range(5):
            detector.evaluate(peer, 1000.0, T0 + timedelta(hours=i))
        # 4000ms is 4× P50 of 1000 — should fire
        result = detector.evaluate(peer, 4000.0, T0 + timedelta(hours=6))
        assert result is not None
        assert isinstance(result, ConvergenceAnomaly)
        assert result.kind == "slow_convergence"
        assert result.ratio >= 3.0

    def test_anomaly_severity_warn(self, detector: ConvergenceAnomalyDetector) -> None:
        """3-6× P50 should be 'warn'."""
        peer = "10.0.0.1"
        for i in range(5):
            detector.evaluate(peer, 1000.0, T0 + timedelta(hours=i))
        result = detector.evaluate(peer, 4000.0, T0 + timedelta(hours=6))
        assert result is not None
        assert result.severity == "warn"

    def test_anomaly_severity_critical(self, detector: ConvergenceAnomalyDetector) -> None:
        """6+× P50 should be 'critical'."""
        peer = "10.0.0.1"
        for i in range(5):
            detector.evaluate(peer, 1000.0, T0 + timedelta(hours=i))
        # 7000ms is 7× P50 — critical
        result = detector.evaluate(peer, 7000.0, T0 + timedelta(hours=6))
        assert result is not None
        assert result.severity == "critical"

    def test_peers_independent(self, detector: ConvergenceAnomalyDetector) -> None:
        """Different peers have independent histories."""
        for i in range(5):
            detector.evaluate("10.0.0.1", 1000.0, T0 + timedelta(hours=i))
            detector.evaluate("10.0.0.2", 5000.0, T0 + timedelta(hours=i))
        # 4000ms for peer 1 fires (4× of 1000)
        r1 = detector.evaluate("10.0.0.1", 4000.0, T0 + timedelta(hours=6))
        assert r1 is not None
        # 4000ms for peer 2 does NOT fire (0.8× of 5000)
        r2 = detector.evaluate("10.0.0.2", 4000.0, T0 + timedelta(hours=6))
        assert r2 is None

    def test_anomaly_count_tracks(self, detector: ConvergenceAnomalyDetector) -> None:
        """anomaly_count increments on each anomaly."""
        peer = "10.0.0.1"
        for i in range(5):
            detector.evaluate(peer, 1000.0, T0 + timedelta(hours=i))
        assert detector.anomaly_count == 0
        detector.evaluate(peer, 4000.0, T0 + timedelta(hours=6))
        assert detector.anomaly_count == 1

    def test_history_count(self, detector: ConvergenceAnomalyDetector) -> None:
        """history_count returns number of events stored for a peer."""
        assert detector.history_count("10.0.0.1") == 0
        detector.evaluate("10.0.0.1", 1000.0, T0)
        assert detector.history_count("10.0.0.1") == 1

    def test_old_entries_evicted(self) -> None:
        """Entries older than 7 days are evicted."""
        detector = ConvergenceAnomalyDetector(min_history=1)
        old = T0 - timedelta(days=10)
        detector.evaluate("10.0.0.1", 1000.0, old)
        # New evaluation at T0 should evict the old entry
        detector.evaluate("10.0.0.1", 1000.0, T0)
        assert detector.history_count("10.0.0.1") == 1  # only the new one

    def test_callback_invoked(self) -> None:
        """on_anomaly callback receives the anomaly."""
        anomalies = []
        detector = ConvergenceAnomalyDetector(on_anomaly=anomalies.append, min_history=3)
        peer = "10.0.0.1"
        for i in range(5):
            detector.evaluate(peer, 1000.0, T0 + timedelta(hours=i))
        detector.evaluate(peer, 4000.0, T0 + timedelta(hours=6))
        assert len(anomalies) == 1
        assert anomalies[0].peer_addr == peer
