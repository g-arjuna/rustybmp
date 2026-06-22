"""
Bundle C1 — ConvergenceDetector unit tests.

Tests the convergence detection logic without a running server.
"""
from __future__ import annotations

import asyncio
from datetime import datetime, timedelta, timezone

import pytest

from bmppy.rbmppy.convergence_detector import ConvergenceDetector, ConvergenceEvent
from bmppy.rbmppy.models import PeerEvent, RouteEvent, StatEvent

UTC = timezone.utc
T0 = datetime(2025, 6, 1, 12, 0, 0, tzinfo=UTC)


def _peer_event(event_type: str, peer: str = "192.0.2.1", ts: datetime = T0) -> PeerEvent:
    return PeerEvent(
        id="evt-1",
        occurred_at=ts,
        speaker_addr="10.0.0.1",
        peer_addr=peer,
        peer_as=65001,
        event_type=event_type,
    )


def _route_event(
    action: str,
    prefix: str = "1.2.3.0/24",
    peer: str = "192.0.2.1",
    ts: datetime = T0,
) -> RouteEvent:
    return RouteEvent(
        id="re-1",
        occurred_at=ts,
        speaker_addr="10.0.0.1",
        peer_addr=peer,
        peer_as=65001,
        rib_type="pre-policy",
        action=action,
        prefix=prefix,
        afi="ipv4",
    )


def _stat_event(
    stat_type: int,
    value: int,
    peer: str = "192.0.2.1",
    ts: datetime = T0,
) -> StatEvent:
    return StatEvent(
        id="st-1",
        occurred_at=ts,
        speaker_addr="10.0.0.1",
        peer_addr=peer,
        counter_name="adj_rib_in",
        counter_value=value,
        stat_type=stat_type,
    )


class TestConvergenceDetector:
    """Unit tests for ConvergenceDetector."""

    @pytest.fixture
    def detector(self) -> ConvergenceDetector:
        return ConvergenceDetector(db_url=None, idle_quiet_secs=5.0)

    @pytest.mark.asyncio
    async def test_peer_down_starts_tracking(self, detector: ConvergenceDetector) -> None:
        """peer_down event starts a convergence episode."""
        await detector.process_peer_event(_peer_event("peer_down"))
        assert detector.active_count == 1

    @pytest.mark.asyncio
    async def test_peer_up_without_down_is_noop(self, detector: ConvergenceDetector) -> None:
        """peer_up without prior peer_down does nothing."""
        await detector.process_peer_event(_peer_event("peer_up"))
        assert detector.active_count == 0
        assert detector.finalized_count == 0

    @pytest.mark.asyncio
    async def test_withdraw_increments_counter(self, detector: ConvergenceDetector) -> None:
        """Withdrawals increment the episode's withdraw count."""
        await detector.process_peer_event(_peer_event("peer_down"))
        await detector.process_route_event(_route_event("withdraw"))
        await detector.process_route_event(_route_event("withdraw", prefix="5.6.7.0/24"))
        # Still active (no EOR yet)
        assert detector.active_count == 1

    @pytest.mark.asyncio
    async def test_stats_eor_finalizes(self, detector: ConvergenceDetector) -> None:
        """Stats type 7 with value=0 finalizes the episode."""
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_route_event(_route_event("withdraw", ts=T0 + timedelta(seconds=1)))
        await detector.process_stats_event(
            _stat_event(stat_type=7, value=0, ts=T0 + timedelta(seconds=3))
        )
        assert detector.active_count == 0
        assert detector.finalized_count == 1

    @pytest.mark.asyncio
    async def test_stats_type8_eor_finalizes(self, detector: ConvergenceDetector) -> None:
        """Stats type 8 with value=0 also finalizes."""
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_stats_event(
            _stat_event(stat_type=8, value=0, ts=T0 + timedelta(seconds=2))
        )
        assert detector.finalized_count == 1

    @pytest.mark.asyncio
    async def test_stats_nonzero_does_not_finalize(self, detector: ConvergenceDetector) -> None:
        """Stats type 7 with non-zero value does not finalize."""
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_stats_event(
            _stat_event(stat_type=7, value=42, ts=T0 + timedelta(seconds=2))
        )
        assert detector.active_count == 1
        assert detector.finalized_count == 0

    @pytest.mark.asyncio
    async def test_idle_check_finalizes_quiet_peer(self) -> None:
        """run_idle_check finalizes episodes quiet for >= idle_quiet_secs."""
        detector = ConvergenceDetector(db_url=None, idle_quiet_secs=0.1)
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_route_event(_route_event("withdraw", ts=T0))
        # Wait briefly to exceed idle threshold
        await asyncio.sleep(0.15)
        await detector.run_idle_check()
        assert detector.active_count == 0
        assert detector.finalized_count == 1

    @pytest.mark.asyncio
    async def test_idle_check_requires_min_withdrawals(self) -> None:
        """run_idle_check does NOT finalize if withdraw_count < min."""
        detector = ConvergenceDetector(db_url=None, idle_quiet_secs=0.1, idle_min_withdrawals=5)
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        # Only 1 withdrawal, min is 5
        await detector.process_route_event(_route_event("withdraw", ts=T0))
        await asyncio.sleep(0.15)
        await detector.run_idle_check()
        assert detector.active_count == 1  # NOT finalized

    @pytest.mark.asyncio
    async def test_peer_up_finalizes_active_episode(self, detector: ConvergenceDetector) -> None:
        """peer_up while tracking finalizes the episode."""
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_route_event(_route_event("withdraw", ts=T0 + timedelta(seconds=1)))
        await detector.process_peer_event(
            _peer_event("peer_up", ts=T0 + timedelta(seconds=5))
        )
        assert detector.active_count == 0
        assert detector.finalized_count == 1

    @pytest.mark.asyncio
    async def test_on_event_callback_invoked(self) -> None:
        """on_event callback receives the finalized ConvergenceEvent."""
        events: list[ConvergenceEvent] = []
        detector = ConvergenceDetector(db_url=None, on_event=events.append)
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_stats_event(
            _stat_event(stat_type=7, value=0, ts=T0 + timedelta(seconds=2))
        )
        assert len(events) == 1
        ev = events[0]
        assert ev.peer_addr == "192.0.2.1"
        assert ev.speaker_addr == "10.0.0.1"
        assert ev.convergence_ms == pytest.approx(2000.0)
        assert ev.trigger_type == "peer_down"

    @pytest.mark.asyncio
    async def test_convergence_ms_calculation(self, detector: ConvergenceDetector) -> None:
        """convergence_ms is eor_at - started_at in milliseconds."""
        events: list[ConvergenceEvent] = []
        det = ConvergenceDetector(db_url=None, on_event=events.append)
        await det.process_peer_event(_peer_event("peer_down", ts=T0))
        eor_ts = T0 + timedelta(seconds=10, milliseconds=500)
        await det.process_stats_event(_stat_event(stat_type=7, value=0, ts=eor_ts))
        assert len(events) == 1
        assert events[0].convergence_ms == pytest.approx(10500.0)

    @pytest.mark.asyncio
    async def test_affected_prefixes_counted(self, detector: ConvergenceDetector) -> None:
        """affected_prefixes counts distinct prefixes in the episode."""
        events: list[ConvergenceEvent] = []
        det = ConvergenceDetector(db_url=None, on_event=events.append)
        await det.process_peer_event(_peer_event("peer_down", ts=T0))
        await det.process_route_event(_route_event("withdraw", prefix="1.0.0.0/8", ts=T0))
        await det.process_route_event(_route_event("withdraw", prefix="2.0.0.0/8", ts=T0))
        await det.process_route_event(_route_event("withdraw", prefix="1.0.0.0/8", ts=T0))  # dup
        await det.process_stats_event(_stat_event(stat_type=7, value=0, ts=T0 + timedelta(seconds=1)))
        assert events[0].affected_prefixes == 2  # distinct

    @pytest.mark.asyncio
    async def test_multiple_peers_tracked_independently(self, detector: ConvergenceDetector) -> None:
        """Two peers can have independent convergence episodes."""
        await detector.process_peer_event(_peer_event("peer_down", peer="192.0.2.1", ts=T0))
        await detector.process_peer_event(_peer_event("peer_down", peer="192.0.2.2", ts=T0))
        assert detector.active_count == 2

        # Finalize only peer 1
        await detector.process_stats_event(
            _stat_event(stat_type=7, value=0, peer="192.0.2.1", ts=T0 + timedelta(seconds=1))
        )
        assert detector.active_count == 1
        assert detector.finalized_count == 1

    @pytest.mark.asyncio
    async def test_route_for_unknown_peer_ignored(self, detector: ConvergenceDetector) -> None:
        """Routes for peers not being tracked are silently ignored."""
        await detector.process_route_event(_route_event("withdraw", peer="10.99.99.99"))
        assert detector.active_count == 0

    @pytest.mark.asyncio
    async def test_double_finalize_is_safe(self, detector: ConvergenceDetector) -> None:
        """Finalizing the same peer twice is a no-op."""
        await detector.process_peer_event(_peer_event("peer_down", ts=T0))
        await detector.process_stats_event(_stat_event(stat_type=7, value=0, ts=T0 + timedelta(seconds=1)))
        # Second finalize attempt
        await detector.process_stats_event(_stat_event(stat_type=7, value=0, ts=T0 + timedelta(seconds=2)))
        assert detector.finalized_count == 1  # only once
