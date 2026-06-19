"""
RPKI validation client for rbmppy (RV3-3).

Provides:
  - RtrVrpCache  — in-process VRP table populated from Routinator's JSON dump
                   endpoint (/api/v1/vrps) or from a local JSON file.
  - RpkiValidator — validates a prefix/origin pair against the cache.
  - poll_rtr_cache() — async helper that refreshes the VRP table on a schedule.

Design: the VRP table is a sorted list of (prefix_network, max_len, origin_asn)
tuples. Validation follows RFC 6811 §2:
  1. Find all VRPs that cover the announcement (most-specific covering prefix).
  2. If none found → state = "not-found" (NotFound / Unknown).
  3. Among covering VRPs, if any matches origin AND prefix_len ≤ max_len → Valid.
  4. Otherwise → Invalid.
"""

from __future__ import annotations

import asyncio
import ipaddress
import json
import logging
from dataclasses import dataclass, field
from typing import List, Optional, Tuple

import httpx

logger = logging.getLogger(__name__)

# ─── Data types ───────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class Vrp:
    """A single Validated ROA Payload entry."""
    prefix:    ipaddress.IPv4Network | ipaddress.IPv6Network
    max_len:   int
    origin_as: int
    trust_anchor: str = ""

    def covers(self, addr_net: ipaddress.IPv4Network | ipaddress.IPv6Network) -> bool:
        """Return True if this VRP's prefix covers addr_net."""
        return (
            type(self.prefix) is type(addr_net)
            and addr_net.subnet_of(self.prefix)  # type: ignore[arg-type]
        )


@dataclass
class ValidationResult:
    prefix:    str
    origin_as: int
    state:     str          # "valid" | "invalid" | "not-found"
    matching_vrps: List[Vrp] = field(default_factory=list)

    @property
    def is_valid(self) -> bool:
        return self.state == "valid"


# ─── VRP cache ────────────────────────────────────────────────────────────────

class RtrVrpCache:
    """
    In-process VRP table.  Thread-safe for concurrent reads; refreshes are
    serialised via asyncio.Lock.
    """

    def __init__(self) -> None:
        self._vrps:       List[Vrp] = []
        self._serial:     int       = 0
        self._lock:       asyncio.Lock = asyncio.Lock()

    # ── Loading ───────────────────────────────────────────────────────────────

    async def load_from_url(self, url: str, timeout: float = 15.0) -> None:
        """Fetch VRPs from Routinator's /api/v1/vrps JSON endpoint."""
        async with self._lock:
            async with httpx.AsyncClient(timeout=timeout) as client:
                r = await client.get(url, headers={"Accept": "application/json"})
                r.raise_for_status()
                data = r.json()
            self._ingest(data)
            logger.info("RPKI VRP cache loaded: %d VRPs (serial=%d)", len(self._vrps), self._serial)

    def load_from_file(self, path: str) -> None:
        """Load VRPs from a local JSON file (same format as Routinator API)."""
        with open(path) as f:
            data = json.load(f)
        self._ingest(data)
        logger.info("RPKI VRP cache loaded from file: %d VRPs", len(self._vrps))

    def _ingest(self, data: dict) -> None:
        """Parse Routinator JSON format into Vrp list."""
        roas = data.get("roas", [])
        vrps = []
        for roa in roas:
            try:
                net = ipaddress.ip_network(roa["prefix"], strict=False)
                vrps.append(Vrp(
                    prefix=net,
                    max_len=int(roa.get("maxLength", net.prefixlen)),
                    origin_as=int(str(roa["asn"]).lstrip("AS")),
                    trust_anchor=roa.get("ta", ""),
                ))
            except (KeyError, ValueError) as exc:
                logger.debug("Skipping malformed VRP entry: %s (%s)", roa, exc)
        self._vrps = vrps
        self._serial = int(data.get("metadata", {}).get("serial", 0))

    # ── Query ─────────────────────────────────────────────────────────────────

    def validate(self, prefix: str, origin_as: int) -> ValidationResult:
        """RFC 6811 §2 validation. Thread-safe (reads only)."""
        try:
            net = ipaddress.ip_network(prefix, strict=False)
        except ValueError:
            return ValidationResult(prefix=prefix, origin_as=origin_as, state="not-found")

        covering = [v for v in self._vrps if v.covers(net)]

        if not covering:
            return ValidationResult(prefix=prefix, origin_as=origin_as, state="not-found")

        matching = [
            v for v in covering
            if v.origin_as == origin_as and net.prefixlen <= v.max_len
        ]

        state = "valid" if matching else "invalid"
        return ValidationResult(
            prefix=prefix,
            origin_as=origin_as,
            state=state,
            matching_vrps=matching if matching else covering,
        )

    @property
    def size(self) -> int:
        return len(self._vrps)

    @property
    def serial(self) -> int:
        return self._serial


# ─── Polling helper ───────────────────────────────────────────────────────────

async def poll_rtr_cache(
    cache:         RtrVrpCache,
    url:           str,
    interval_secs: int = 600,
) -> None:
    """
    Refresh the VRP cache on a schedule.  Run as a background asyncio task.

    Args:
        cache:         RtrVrpCache instance to refresh.
        url:           Routinator JSON endpoint, e.g. http://localhost:9556/api/v1/vrps
        interval_secs: Refresh interval (default 10 min).
    """
    while True:
        try:
            await cache.load_from_url(url)
        except Exception as exc:
            logger.warning("RPKI VRP refresh failed: %s", exc)
        await asyncio.sleep(interval_secs)


# ─── Convenience validator (stateless, calls Cloudflare API) ─────────────────

async def validate_via_api(
    prefix:     str,
    origin_asn: int,
    timeout:    float = 10.0,
) -> ValidationResult:
    """
    Validate via Cloudflare's RPKI API (https://rpki.cloudflare.com).
    Use when a local VRP cache is not available.
    """
    url    = "https://rpki.cloudflare.com/api/v1/validity"
    params = {"prefix": prefix, "asn": str(origin_asn)}
    try:
        async with httpx.AsyncClient(timeout=timeout) as client:
            r = await client.get(url, params=params)
            r.raise_for_status()
            body    = r.json()
            validity = body.get("validated_route", {}).get("validity", {})
            state   = validity.get("state", "not-found")
            roas    = validity.get("matching_roas", [])
            return ValidationResult(prefix=prefix, origin_as=origin_asn, state=state)
    except httpx.HTTPError as exc:
        logger.debug("Cloudflare RPKI API error: %s", exc)
        return ValidationResult(prefix=prefix, origin_as=origin_asn, state="not-found")
