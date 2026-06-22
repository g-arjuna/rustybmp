"""
Bundle F3 — IRR/RADB client.

Asyncio TCP whois client to ``whois.radb.net:43`` for route object validation.

Usage::

    from rbmppy.irr_client import IrrClient

    client = IrrClient()
    result = await client.validate_route("1.2.3.0/24", 65001)
    members = await client.get_as_set("AS-EXAMPLE")
    routes  = await client.get_route_objects(65001)
"""
from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from typing import Optional

logger = logging.getLogger(__name__)

DEFAULT_HOST = "whois.radb.net"
DEFAULT_PORT = 43
TIMEOUT = 10.0


@dataclass
class IrrResult:
    """Result of an IRR route validation."""

    status: str  # "match", "mismatch", "not_found"
    route_object: Optional[str] = None
    origin_as: Optional[int] = None


class IrrClient:
    """Async whois client for IRR/RADB queries.

    Parameters
    ----------
    host : str
        RADB whois server hostname.
    port : int
        RADB whois server port.
    timeout : float
        Connection timeout in seconds.
    """

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
        timeout: float = TIMEOUT,
    ) -> None:
        self._host = host
        self._port = port
        self._timeout = timeout

    async def _query(self, query: str) -> str:
        """Send a whois query and return the response text."""
        try:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(self._host, self._port),
                timeout=self._timeout,
            )
            writer.write(f"{query}\r\n".encode())
            await writer.drain()

            data = await asyncio.wait_for(reader.read(65536), timeout=self._timeout)
            writer.close()
            await writer.wait_closed()
            return data.decode("utf-8", errors="replace")
        except (asyncio.TimeoutError, OSError) as exc:
            logger.warning("IRR query failed: %s — %s", query.strip(), exc)
            return ""

    async def validate_route(self, prefix: str, origin_as: int) -> IrrResult:
        """Validate a route-origin pair against IRR.

        Returns:
            IrrResult with status "match", "mismatch", or "not_found".
        """
        response = await self._query(f"-r -T route {prefix}")

        if not response.strip() or "No entries found" in response:
            return IrrResult(status="not_found")

        # Parse origin from response
        for line in response.splitlines():
            line = line.strip()
            if line.lower().startswith("origin:"):
                found_as = line.split(":", 1)[1].strip().upper()
                found_asn = int(found_as.replace("AS", ""))
                if found_asn == origin_as:
                    return IrrResult(
                        status="match",
                        route_object=prefix,
                        origin_as=found_asn,
                    )
                else:
                    return IrrResult(
                        status="mismatch",
                        route_object=prefix,
                        origin_as=found_asn,
                    )

        return IrrResult(status="not_found")

    async def get_as_set(self, as_set_name: str) -> list[int]:
        """Recursively expand an AS-SET to a list of member ASNs."""
        response = await self._query(f"-r -i origin -T aut-num {as_set_name}")

        members: list[int] = []
        for line in response.splitlines():
            line = line.strip()
            if line.lower().startswith("aut-num:"):
                asn_str = line.split(":", 1)[1].strip().upper().replace("AS", "")
                try:
                    members.append(int(asn_str))
                except ValueError:
                    pass

        # If no results, try members: field
        if not members:
            response = await self._query(f"-r -T as-set {as_set_name}")
            for line in response.splitlines():
                line = line.strip()
                if line.lower().startswith("members:"):
                    parts = line.split(":", 1)[1].strip().split(",")
                    for part in parts:
                        part = part.strip().upper().replace("AS", "")
                        try:
                            members.append(int(part))
                        except ValueError:
                            pass

        return sorted(set(members))

    async def get_route_objects(self, asn: int) -> list[str]:
        """Get all route objects for an ASN."""
        response = await self._query(f"-r -i origin AS{asn}")

        routes: list[str] = []
        for line in response.splitlines():
            line = line.strip()
            if line.lower().startswith("route:"):
                route = line.split(":", 1)[1].strip()
                routes.append(route)

        return sorted(set(routes))
