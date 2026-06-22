"""
Bundle F4 — Looking Glass: Cloudflare Radar + HE BGP.

Adds ``cloudflare_visible`` and ``he_bgp_origin`` fields to prefix
visibility queries.

Usage::

    from rbmppy.looking_glass import LookingGlassClient

    lg = LookingGlassClient(cloudflare_api_key="optional-key")
    result = await lg.check_prefix("1.2.3.0/24")
"""
from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from typing import Optional

import aiohttp

logger = logging.getLogger(__name__)

CF_RADAR_BASE = "https://api.cloudflare.com/client/v4/radar/bgp/routes"
HE_BGP_BASE = "https://bgp.he.net"


@dataclass
class PrefixVisibility:
    """Result of prefix visibility check across looking glasses."""

    prefix: str
    cloudflare_visible: Optional[bool] = None
    cloudflare_origin_as: Optional[int] = None
    he_bgp_origin: Optional[int] = None
    he_bgp_visible: Optional[bool] = None
    errors: list[str] = None

    def __post_init__(self):
        if self.errors is None:
            self.errors = []


class LookingGlassClient:
    """Multi-source looking glass client.

    Parameters
    ----------
    cloudflare_api_key : str or None
        Cloudflare Radar API key (optional, 10K/day free tier).
    timeout : float
        HTTP request timeout.
    """

    def __init__(
        self,
        cloudflare_api_key: Optional[str] = None,
        timeout: float = 10.0,
    ) -> None:
        self._cf_key = cloudflare_api_key
        self._timeout = aiohttp.ClientTimeout(total=timeout)

    async def check_prefix(self, prefix: str) -> PrefixVisibility:
        """Check prefix visibility on Cloudflare Radar and HE BGP."""
        result = PrefixVisibility(prefix=prefix)

        async with aiohttp.ClientSession(timeout=self._timeout) as session:
            # Run both lookups concurrently
            cf_task = self._check_cloudflare(session, prefix, result)
            he_task = self._check_he_bgp(session, prefix, result)
            await asyncio.gather(cf_task, he_task, return_exceptions=True)

        return result

    async def _check_cloudflare(
        self,
        session: aiohttp.ClientSession,
        prefix: str,
        result: PrefixVisibility,
    ) -> None:
        """Query Cloudflare Radar BGP routes API."""
        if not self._cf_key:
            result.errors.append("cloudflare: no API key configured")
            return

        try:
            headers = {"Authorization": f"Bearer {self._cf_key}"}
            params = {"prefix": prefix}
            async with session.get(CF_RADAR_BASE, headers=headers, params=params) as resp:
                if resp.status != 200:
                    result.errors.append(f"cloudflare: HTTP {resp.status}")
                    return
                data = await resp.json()
                routes = data.get("result", {}).get("routes", [])
                result.cloudflare_visible = len(routes) > 0
                if routes:
                    origin = routes[0].get("origin_asn")
                    result.cloudflare_origin_as = int(origin) if origin else None
        except Exception as exc:
            logger.warning("Cloudflare lookup failed: %s", exc)
            result.errors.append(f"cloudflare: {exc}")

    async def _check_he_bgp(
        self,
        session: aiohttp.ClientSession,
        prefix: str,
        result: PrefixVisibility,
    ) -> None:
        """Query Hurricane Electric BGP toolkit (public, rate-limited)."""
        try:
            url = f"{HE_BGP_BASE}/net/{prefix}"
            async with session.get(url) as resp:
                if resp.status != 200:
                    result.errors.append(f"he_bgp: HTTP {resp.status}")
                    return
                text = await resp.text()
                # Simple parsing — look for origin AS in page
                result.he_bgp_visible = "Origin AS" in text
                if result.he_bgp_visible:
                    # Extract ASN from page text
                    import re
                    match = re.search(r'AS(\d+)', text)
                    if match:
                        result.he_bgp_origin = int(match.group(1))
        except Exception as exc:
            logger.warning("HE BGP lookup failed: %s", exc)
            result.errors.append(f"he_bgp: {exc}")
