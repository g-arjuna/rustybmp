"""
Bundle F5 — RIPE Atlas Traceroute client.

Async traceroute creation and result polling via RIPE Atlas Measurement API.

Usage::

    from rbmppy.ripe_atlas import RipeAtlasClient

    client = RipeAtlasClient(api_key="optional-key")
    mid = await client.create_traceroute("1.2.3.4")
    result = await client.get_results(mid)
"""
from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Optional

import aiohttp

logger = logging.getLogger(__name__)

ATLAS_BASE = "https://atlas.ripe.net/api/v2"


@dataclass
class TracerouteHop:
    """A single hop in a traceroute result."""

    hop: int
    ip: Optional[str] = None
    rtt_ms: Optional[float] = None
    asn: Optional[int] = None


@dataclass
class TracerouteResult:
    """Result of a RIPE Atlas traceroute measurement."""

    measurement_id: str
    status: str  # "pending", "complete", "error"
    target: Optional[str] = None
    hops: list[TracerouteHop] = field(default_factory=list)
    error: Optional[str] = None


class RipeAtlasClient:
    """Async RIPE Atlas measurement client.

    Parameters
    ----------
    api_key : str or None
        RIPE Atlas API key. Anonymous allows 100 measurements/day.
    timeout : float
        HTTP request timeout.
    """

    def __init__(
        self,
        api_key: Optional[str] = None,
        timeout: float = 30.0,
    ) -> None:
        self._api_key = api_key
        self._timeout = aiohttp.ClientTimeout(total=timeout)

    def _headers(self) -> dict[str, str]:
        headers = {"Accept": "application/json", "Content-Type": "application/json"}
        if self._api_key:
            headers["Authorization"] = f"Key {self._api_key}"
        return headers

    async def create_traceroute(
        self,
        target: str,
        probe_count: int = 3,
        protocol: str = "ICMP",
        af: int = 4,
    ) -> str:
        """Create a one-off traceroute measurement.

        Returns the measurement ID string.
        """
        payload = {
            "definitions": [{
                "type": "traceroute",
                "target": target,
                "af": af,
                "protocol": protocol,
                "description": f"RustyBMP traceroute to {target}",
                "is_oneoff": True,
            }],
            "probes": [{
                "type": "area",
                "value": "WW",
                "requested": probe_count,
            }],
        }

        async with aiohttp.ClientSession(timeout=self._timeout) as session:
            async with session.post(
                f"{ATLAS_BASE}/measurements/",
                json=payload,
                headers=self._headers(),
            ) as resp:
                if resp.status not in (200, 201):
                    body = await resp.text()
                    raise RuntimeError(f"Atlas measurement create failed: {resp.status} {body}")
                data = await resp.json()
                mid = str(data.get("measurements", [None])[0])
                logger.info("Created Atlas traceroute measurement: %s → %s", mid, target)
                return mid

    async def get_results(self, measurement_id: str) -> TracerouteResult:
        """Poll for measurement results.

        Returns a TracerouteResult with status "pending" if not yet complete.
        """
        async with aiohttp.ClientSession(timeout=self._timeout) as session:
            # Check measurement status
            async with session.get(
                f"{ATLAS_BASE}/measurements/{measurement_id}/",
                headers=self._headers(),
            ) as resp:
                if resp.status != 200:
                    return TracerouteResult(
                        measurement_id=measurement_id,
                        status="error",
                        error=f"HTTP {resp.status}",
                    )
                meta = await resp.json()
                status = meta.get("status", {})
                status_name = status.get("name", "Unknown") if isinstance(status, dict) else str(status)

                if status_name not in ("Specified", "Ongoing"):
                    # Try to get results
                    async with session.get(
                        f"{ATLAS_BASE}/measurements/{measurement_id}/results/",
                        headers=self._headers(),
                    ) as results_resp:
                        if results_resp.status != 200:
                            return TracerouteResult(
                                measurement_id=measurement_id,
                                status="error",
                                error=f"Results HTTP {results_resp.status}",
                            )
                        results_data = await results_resp.json()
                        hops = self._parse_hops(results_data)
                        return TracerouteResult(
                            measurement_id=measurement_id,
                            status="complete",
                            target=meta.get("target"),
                            hops=hops,
                        )

                return TracerouteResult(
                    measurement_id=measurement_id,
                    status="pending",
                    target=meta.get("target"),
                )

    @staticmethod
    def _parse_hops(results: list) -> list[TracerouteHop]:
        """Parse RIPE Atlas traceroute result JSON into hops."""
        hops = []
        if not results:
            return hops

        # Take first probe result
        result = results[0] if isinstance(results, list) else results
        for hop_data in result.get("result", []):
            hop_num = hop_data.get("hop", 0)
            # Each hop has multiple attempts
            for attempt in hop_data.get("result", []):
                ip = attempt.get("from")
                rtt = attempt.get("rtt")
                if ip and ip != "*":
                    hops.append(TracerouteHop(
                        hop=hop_num,
                        ip=ip,
                        rtt_ms=rtt,
                    ))
                    break  # One per hop

        return hops
