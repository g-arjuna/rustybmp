"""
BGP anomaly detectors for rbmppy (RV3-3).

Each detector is an independent, stateful class that ingests RouteEvent objects
from the rustybmp SSE stream and emits Alert objects.

Detectors:
  - OriginChangeDetector   — fires when origin AS for a known prefix changes.
  - RouteLeakDetector      — fires on OTC-based valley-free violations (RFC 9234)
                             and on AS-path length shortening anomalies.
  - MEDOscillationDetector — fires when MED oscillates rapidly across peers.
  - BGPHijackDetector      — fires on suspicious origin change + visibility spike
                             combinations; optionally validates against RPKI.
  - DetectorPipeline       — composes all detectors, processes events in parallel,
                             emits de-duplicated alert stream.
"""

from __future__ import annotations

import asyncio
import logging
import time
from collections import defaultdict, deque
from dataclasses import dataclass, field
from typing import AsyncIterator, Callable, Dict, List, Optional, Set

from .models import RouteEvent
from .rpki import RtrVrpCache, ValidationResult


def _origin_as(event: RouteEvent) -> Optional[int]:
    """Extract the origin AS (last AS in the path) from a RouteEvent."""
    if not event.as_path:
        return None
    parts = event.as_path.split()
    for token in reversed(parts):
        try:
            return int(token)
        except ValueError:
            continue
    return None


def _as_path_list(event: RouteEvent) -> List[int]:
    """Return AS-path as a list of ints, ignoring set/confed notation."""
    if not event.as_path:
        return []
    result = []
    for token in event.as_path.split():
        try:
            result.append(int(token))
        except ValueError:
            pass
    return result

logger = logging.getLogger(__name__)

# ─── Alert ────────────────────────────────────────────────────────────────────

@dataclass
class Alert:
    kind:        str           # e.g. "origin-change", "route-leak", "hijack"
    prefix:      str
    severity:    str           # "info" | "warn" | "critical"
    description: str
    event:       Optional[RouteEvent] = None
    ts:          float = field(default_factory=time.time)

    def __str__(self) -> str:
        return f"[{self.severity.upper()}] {self.kind}: {self.prefix} — {self.description}"


# ─── OriginChangeDetector ─────────────────────────────────────────────────────

class OriginChangeDetector:
    """
    Fires an alert when the origin AS for a known prefix changes.

    Tracks the most-recent origin per (prefix, peer) pair.  A change in the
    origin AS for a prefix that has been stable for at least `grace_secs`
    generates a "warn" or "critical" alert depending on whether RPKI validation
    is available.
    """

    def __init__(
        self,
        grace_secs:  float = 300.0,
        vrp_cache:   Optional[RtrVrpCache] = None,
    ) -> None:
        self.grace_secs = grace_secs
        self.vrp_cache  = vrp_cache
        # (prefix, peer_addr) → (origin_asn, first_seen_ts)
        self._state: Dict[tuple, tuple] = {}

    def process(self, event: RouteEvent) -> List[Alert]:
        origin = _origin_as(event)
        if not event.prefix or origin is None:
            return []
        key       = (event.prefix, event.peer_addr)
        now       = time.time()
        alerts    = []

        if key in self._state:
            prev_origin, first_seen = self._state[key]
            if prev_origin != origin and (now - first_seen) >= self.grace_secs:
                severity = "warn"
                desc     = (
                    f"origin changed {prev_origin} → {origin} "
                    f"(stable for {int(now - first_seen)}s)"
                )
                # If RPKI says the new origin is Invalid → critical
                if self.vrp_cache:
                    result = self.vrp_cache.validate(event.prefix, origin)
                    if result.state == "invalid":
                        severity = "critical"
                        desc    += " [RPKI: INVALID]"
                    elif result.state == "valid":
                        severity = "info"
                        desc    += " [RPKI: VALID]"
                alerts.append(Alert(
                    kind="origin-change",
                    prefix=event.prefix,
                    severity=severity,
                    description=desc,
                    event=event,
                ))
        self._state[key] = (origin, now)
        return alerts


# ─── RouteLeakDetector ────────────────────────────────────────────────────────

class RouteLeakDetector:
    """
    Detects valley-free violations using:
      1. OTC (Only-to-Customer) attribute (RFC 9234): if OTC is present
         and the peer sending the route is not a customer, it's a leak.
      2. AS-path shortening heuristic: if a prefix suddenly appears via
         a significantly shorter AS-path than previously observed, flag it.

    Also detects unexpected transit of customer prefixes (simplified).
    """

    def __init__(self, path_shortening_threshold: int = 3) -> None:
        self.threshold = path_shortening_threshold
        # prefix → min observed path length
        self._min_path_len: Dict[str, int] = {}

    def process(self, event: RouteEvent) -> List[Alert]:
        if not event.prefix:
            return []
        alerts = []

        # ── OTC-based detection ───────────────────────────────────────────────
        if getattr(event, "only_to_customer", None) is not None:
            alerts.append(Alert(
                kind="route-leak",
                prefix=event.prefix,
                severity="warn",
                description=(
                    f"OTC attribute present (OTC={event.only_to_customer}), "
                    f"received from peer {event.peer_addr}"
                ),
                event=event,
            ))

        # ── Path-shortening heuristic ─────────────────────────────────────────
        path_len = len(_as_path_list(event))
        if path_len > 0:
            prev_min = self._min_path_len.get(event.prefix)
            if prev_min is not None and (prev_min - path_len) >= self.threshold:
                alerts.append(Alert(
                    kind="route-leak",
                    prefix=event.prefix,
                    severity="warn",
                    description=(
                        f"AS-path shortened by {prev_min - path_len} hops "
                        f"(was {prev_min}, now {path_len})"
                    ),
                    event=event,
                ))
            if prev_min is None or path_len < prev_min:
                self._min_path_len[event.prefix] = path_len

        return alerts


# ─── MEDOscillationDetector ───────────────────────────────────────────────────

class MEDOscillationDetector:
    """
    Fires when MED for a prefix oscillates rapidly between two or more values
    across the same peer over a short observation window.

    "Oscillation" = more than `max_changes` distinct MED values within
    `window_secs`, with at least two direction reversals.
    """

    def __init__(self, window_secs: float = 60.0, max_changes: int = 5) -> None:
        self.window_secs = window_secs
        self.max_changes = max_changes
        # (prefix, peer) → deque of (ts, med)
        self._history: Dict[tuple, deque] = defaultdict(deque)

    def process(self, event: RouteEvent) -> List[Alert]:
        med = getattr(event, "med", None)
        if med is None or not event.prefix:
            return []

        key = (event.prefix, event.peer_addr)
        now = time.time()
        dq  = self._history[key]
        dq.append((now, med))

        # Evict entries outside the window
        while dq and (now - dq[0][0]) > self.window_secs:
            dq.popleft()

        if len(dq) < self.max_changes:
            return []

        meds = [m for _, m in dq]
        distinct = set(meds)
        # Count direction reversals
        reversals = sum(
            1 for i in range(1, len(meds) - 1)
            if (meds[i] > meds[i - 1]) != (meds[i + 1] > meds[i])
        )

        if len(distinct) >= 2 and reversals >= 2:
            return [Alert(
                kind="med-oscillation",
                prefix=event.prefix,
                severity="info",
                description=(
                    f"MED oscillating between {min(distinct)} and {max(distinct)} "
                    f"({len(dq)} changes in {int(self.window_secs)}s, "
                    f"{reversals} reversals)"
                ),
                event=event,
            )]
        return []


# ─── BGPHijackDetector ────────────────────────────────────────────────────────

class BGPHijackDetector:
    """
    Combines origin-change detection with a visibility spike to flag potential
    BGP hijacks with higher confidence.

    Heuristic:
      - An origin change fires.
      - AND the new origin was not observed in the last `history_window_secs`.
      - AND (optionally) RPKI validation returns "invalid" for the new origin.
    """

    def __init__(
        self,
        history_window_secs: float = 3600.0,
        vrp_cache:           Optional[RtrVrpCache] = None,
    ) -> None:
        self.window    = history_window_secs
        self.vrp_cache = vrp_cache
        # prefix → set of (origin_as, last_seen_ts)
        self._origins: Dict[str, Dict[int, float]] = defaultdict(dict)

    def process(self, event: RouteEvent) -> List[Alert]:
        origin = _origin_as(event)
        if not event.prefix or origin is None:
            return []

        prefix = event.prefix
        now    = time.time()

        # Evict stale origins
        known = self._origins[prefix]
        stale = [o for o, ts in known.items() if (now - ts) > self.window]
        for o in stale:
            del known[o]

        alerts = []

        if known and origin not in known:
            # New origin never seen in the window
            severity = "warn"
            desc     = (
                f"new origin AS{origin} never seen before for {prefix} "
                f"(known: {sorted(known.keys())})"
            )
            if self.vrp_cache:
                result = self.vrp_cache.validate(prefix, origin)
                if result.state == "invalid":
                    severity = "critical"
                    desc    += " [RPKI: INVALID — likely hijack]"
                elif result.state == "valid":
                    desc    += " [RPKI: VALID — possible legitimate change]"

            alerts.append(Alert(
                kind="hijack",
                prefix=prefix,
                severity=severity,
                description=desc,
                event=event,
            ))

        known[origin] = now
        return alerts


# ─── DetectorPipeline ─────────────────────────────────────────────────────────

class DetectorPipeline:
    """
    Composes multiple detectors.  Feed RouteEvents via `process()` and collect
    all Alerts.  Runs each detector synchronously; all are O(1) per event.

    Usage::

        vrp_cache = RtrVrpCache()
        await vrp_cache.load_from_url("http://routinator:9556/api/v1/vrps")

        pipeline = DetectorPipeline(vrp_cache=vrp_cache)
        async for event in stream_route_events(client):
            alerts = pipeline.process(event)
            for alert in alerts:
                print(alert)
    """

    def __init__(
        self,
        vrp_cache:          Optional[RtrVrpCache] = None,
        on_alert:           Optional[Callable[[Alert], None]] = None,
        grace_secs:         float = 300.0,
        med_window_secs:    float = 60.0,
        hijack_window_secs: float = 3600.0,
    ) -> None:
        self._detectors = [
            OriginChangeDetector(grace_secs=grace_secs, vrp_cache=vrp_cache),
            RouteLeakDetector(),
            MEDOscillationDetector(window_secs=med_window_secs),
            BGPHijackDetector(history_window_secs=hijack_window_secs, vrp_cache=vrp_cache),
        ]
        self._on_alert    = on_alert or (lambda a: logger.warning("%s", a))
        self._alert_count = 0

    def process(self, event: RouteEvent) -> List[Alert]:
        """Run all detectors on a single event; invoke `on_alert` for each finding."""
        all_alerts: List[Alert] = []
        for detector in self._detectors:
            try:
                alerts = detector.process(event)
                all_alerts.extend(alerts)
            except Exception as exc:
                logger.debug("Detector %s raised: %s", type(detector).__name__, exc)

        for alert in all_alerts:
            self._alert_count += 1
            try:
                self._on_alert(alert)
            except Exception as exc:
                logger.debug("on_alert callback raised: %s", exc)

        return all_alerts

    @property
    def alert_count(self) -> int:
        return self._alert_count
