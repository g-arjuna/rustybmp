"""
Bundle C1 — Convergence Event Detector.

Monitors the peer event / route event / stats event streams and detects
BGP convergence episodes.  A convergence episode is triggered by a
peer_down event and finalized when either:

  1. A Stats Report type 7 or 8 counter drops to 0 (EOR signal), OR
  2. The withdrawal rate falls below 5/sec for 5 consecutive seconds.

The completed event is inserted into the ``convergence_events`` DuckDB table
(schema created in RV7-UI6).

Usage::

    from rbmppy.convergence_detector import ConvergenceDetector

    detector = ConvergenceDetector(db_url="http://localhost:7878")
    await detector.process_peer_event(peer_event)
    await detector.process_route_event(route_event)
    await detector.process_stats_event(stat_event)
    await detector.run_idle_check()  # call every ~1s
"""

from __future__ import annotations

import asyncio
import logging
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Callable, Optional

import httpx

from .models import PeerEvent, RouteEvent, StatEvent

logger = logging.getLogger(__name__)

UTC = timezone.utc


# ─── Internal state per active convergence episode ────────────────────────────

@dataclass
class _ActiveEpisode:
    """Mutable state for an in-progress convergence episode."""

    started_at: datetime
    speaker_addr: str
    peer_addr: str
    trigger_type: str = "peer_down"
    withdraw_count: int = 0
    affected_prefixes: set = field(default_factory=set)
    last_burst_ts: datetime = field(default_factory=lambda: datetime.now(UTC))


# ─── Finalized convergence event ─────────────────────────────────────────────

@dataclass
class ConvergenceEvent:
    """Immutable record written to the ``convergence_events`` table."""

    event_id: str
    started_at: datetime
    eor_at: datetime
    convergence_ms: float
    speaker_addr: str
    peer_addr: str
    trigger_type: str
    affected_prefixes: int
    recovered_prefixes: int = 0
    unreachable_after: int = 0


# ─── Detector ─────────────────────────────────────────────────────────────────

class ConvergenceDetector:
    """Stateful convergence detector that tracks peer-down → EOR episodes.

    Parameters
    ----------
    db_url:
        Base URL of the rustybmp HTTP server (for inserting finalized
        events via ``POST /api/convergence``).  Set to ``None`` for
        unit-test / dry-run mode.
    idle_quiet_secs:
        Number of quiet seconds before an idle episode is finalized.
    idle_min_withdrawals:
        Minimum withdrawals before an idle-check finalization fires.
    on_event:
        Optional callback invoked with each finalized :class:`ConvergenceEvent`.
    """

    def __init__(
        self,
        db_url: Optional[str] = None,
        idle_quiet_secs: float = 5.0,
        idle_min_withdrawals: int = 1,
        on_event: Optional[Callable[[ConvergenceEvent], None]] = None,
    ) -> None:
        self._db_url = db_url
        self._idle_quiet_secs = idle_quiet_secs
        self._idle_min_withdrawals = idle_min_withdrawals
        self._on_event = on_event

        # peer_addr → _ActiveEpisode
        self._active: dict[str, _ActiveEpisode] = {}

        # Running stats
        self._finalized_count = 0

    # ── Public API ────────────────────────────────────────────────────────────

    @property
    def active_count(self) -> int:
        """Number of currently tracked convergence episodes."""
        return len(self._active)

    @property
    def finalized_count(self) -> int:
        """Total number of episodes finalized since construction."""
        return self._finalized_count

    async def process_peer_event(self, event: PeerEvent) -> None:
        """Start tracking on peer_down, clear on peer_up."""
        if event.event_type == "peer_down":
            now = event.occurred_at if event.occurred_at.tzinfo else event.occurred_at.replace(tzinfo=UTC)
            self._active[event.peer_addr] = _ActiveEpisode(
                started_at=now,
                speaker_addr=event.speaker_addr,
                peer_addr=event.peer_addr,
                trigger_type="peer_down",
                last_burst_ts=now,
            )
            logger.info("Convergence tracking started for peer %s", event.peer_addr)
        elif event.event_type == "peer_up":
            # Peer came back up without EOR — finalize if active
            if event.peer_addr in self._active:
                now = event.occurred_at if event.occurred_at.tzinfo else event.occurred_at.replace(tzinfo=UTC)
                await self._finalize(event.peer_addr, now)

    async def process_route_event(self, event: RouteEvent) -> None:
        """Increment withdrawal counter for active episodes."""
        if event.action == "withdraw" and event.peer_addr in self._active:
            ep = self._active[event.peer_addr]
            ep.withdraw_count += 1
            ep.affected_prefixes.add(event.prefix)
            now = event.occurred_at if event.occurred_at.tzinfo else event.occurred_at.replace(tzinfo=UTC)
            ep.last_burst_ts = now
        elif event.action == "announce" and event.peer_addr in self._active:
            # Track prefix re-announcements for recovery count
            ep = self._active[event.peer_addr]
            ep.affected_prefixes.add(event.prefix)
            now = event.occurred_at if event.occurred_at.tzinfo else event.occurred_at.replace(tzinfo=UTC)
            ep.last_burst_ts = now

    async def process_stats_event(self, event: StatEvent) -> None:
        """Detect EOR via stats type 7 or 8 dropping to 0."""
        if event.peer_addr not in self._active:
            return
        # stat_type 7 = adj-RIBs-In routes count, 8 = loc-RIB routes count
        if event.stat_type in (7, 8) and event.counter_value == 0:
            now = event.occurred_at if event.occurred_at.tzinfo else event.occurred_at.replace(tzinfo=UTC)
            logger.info(
                "EOR signal for peer %s (stat_type=%d → 0)",
                event.peer_addr,
                event.stat_type,
            )
            await self._finalize(event.peer_addr, now)

    async def run_idle_check(self) -> None:
        """Call periodically (~1s). Finalizes peers quiet for ``idle_quiet_secs``."""
        now = datetime.now(UTC)
        to_finalize = []
        for peer_addr, ep in self._active.items():
            elapsed = (now - ep.last_burst_ts).total_seconds()
            if elapsed >= self._idle_quiet_secs and ep.withdraw_count >= self._idle_min_withdrawals:
                to_finalize.append(peer_addr)

        for peer_addr in to_finalize:
            await self._finalize(peer_addr, now)

    # ── Internals ─────────────────────────────────────────────────────────────

    async def _finalize(self, peer_addr: str, eor_at: datetime) -> None:
        """Complete a convergence episode and persist it."""
        if peer_addr not in self._active:
            return

        ep = self._active.pop(peer_addr)
        ms = (eor_at - ep.started_at).total_seconds() * 1000

        event = ConvergenceEvent(
            event_id=str(uuid.uuid4()),
            started_at=ep.started_at,
            eor_at=eor_at,
            convergence_ms=ms,
            speaker_addr=ep.speaker_addr,
            peer_addr=ep.peer_addr,
            trigger_type=ep.trigger_type,
            affected_prefixes=len(ep.affected_prefixes),
            recovered_prefixes=0,
            unreachable_after=0,
        )

        self._finalized_count += 1
        logger.info(
            "Convergence event finalized: peer=%s ms=%.0f affected=%d",
            peer_addr,
            ms,
            event.affected_prefixes,
        )

        # Invoke callback
        if self._on_event:
            try:
                self._on_event(event)
            except Exception as exc:
                logger.warning("on_event callback error: %s", exc)

        # Persist to rustybmp
        if self._db_url:
            await self._persist(event)

    async def _persist(self, event: ConvergenceEvent) -> None:
        """POST the finalized event to rustybmp's convergence API."""
        url = f"{self._db_url.rstrip('/')}/api/convergence"
        payload = {
            "event_id": event.event_id,
            "started_at": event.started_at.isoformat(),
            "eor_at": event.eor_at.isoformat(),
            "convergence_ms": event.convergence_ms,
            "speaker_addr": event.speaker_addr,
            "peer_addr": event.peer_addr,
            "trigger_type": event.trigger_type,
            "affected_prefixes": event.affected_prefixes,
            "recovered_prefixes": event.recovered_prefixes,
            "unreachable_after": event.unreachable_after,
        }
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                resp = await client.post(url, json=payload)
                if resp.status_code != 200:
                    logger.warning(
                        "Convergence persist failed: %d %s",
                        resp.status_code,
                        resp.text[:200],
                    )
        except Exception as exc:
            logger.warning("Convergence persist error: %s", exc)
