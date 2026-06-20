"""
Internet resource lookups for rbmppy (RV3-3).

Provides:
  - IrrClient    — query IRR databases (RIPE, ARIN, APNIC) via WHOIS TCP for
                   route-objects and AS-SET membership.
  - RdapClient   — query RDAP (ARIN/RIPE/APNIC/LACNIC/AFRINIC) for ASN and
                   IP prefix registration details.
  - BgpToolsClient — query BGP.Tools JSON API for prefix visibility, upstreams,
                   and ASN summary (no API key needed, public read API).
  - resolve_origin() — aggregate helper: given a prefix + origin ASN, produce
                   a structured OriginInfo with org name, IRR route-object,
                   upstream providers, and visible prefix count.
"""

from __future__ import annotations

import asyncio
import ipaddress
import logging
import re
from dataclasses import dataclass, field
from typing import List, Optional

import httpx

logger = logging.getLogger(__name__)

# ─── Data types ───────────────────────────────────────────────────────────────

@dataclass
class AsnInfo:
    asn:         int
    name:        str           = ""
    country:     str           = ""
    org:         str           = ""
    rir:         str           = ""
    irr_as_set:  Optional[str] = None
    upstream_asns: List[int]   = field(default_factory=list)
    prefix_count_v4: int       = 0
    prefix_count_v6: int       = 0


@dataclass
class RouteObject:
    prefix:   str
    origin:   int
    descr:    str = ""
    rir:      str = ""
    changed:  str = ""


@dataclass
class OriginInfo:
    prefix:       str
    origin_asn:   int
    asn_info:     Optional[AsnInfo]        = None
    route_objects: List[RouteObject]       = field(default_factory=list)
    upstream_asns: List[int]              = field(default_factory=list)
    visible_peers: int                    = 0


# ─── IRR client (WHOIS) ───────────────────────────────────────────────────────

class IrrClient:
    """
    Query IRR databases via WHOIS TCP (port 43).

    Supports RIPE, ARIN, APNIC, RADB, NTTCOM.
    """

    SERVERS = {
        "ripe":  "whois.ripe.net",
        "arin":  "rr.arin.net",
        "apnic": "whois.apnic.net",
        "radb":  "whois.radb.net",
        "ntt":   "rr.ntt.net",
    }
    PORT = 43

    def __init__(self, server: str = "ripe", timeout: float = 10.0) -> None:
        self.host    = self.SERVERS.get(server, server)
        self.timeout = timeout

    async def query(self, query: str) -> str:
        """Send a raw WHOIS query and return the full response."""
        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(self.host, self.PORT),
            timeout=self.timeout,
        )
        try:
            writer.write((query.strip() + "\r\n").encode())
            await writer.drain()
            data = await asyncio.wait_for(reader.read(65536), timeout=self.timeout)
            return data.decode(errors="replace")
        finally:
            writer.close()
            try:
                await writer.wait_closed()
            except Exception:
                pass

    async def route_objects(self, prefix: str) -> List[RouteObject]:
        """Return IRR route-objects covering the given prefix."""
        try:
            raw = await self.query(f"-K -r route,route6 {prefix}")
        except Exception as exc:
            logger.debug("IRR WHOIS error for %s: %s", prefix, exc)
            return []
        return _parse_route_objects(raw)

    async def as_set_members(self, as_set: str, depth: int = 2) -> List[int]:
        """Recursively expand an AS-SET and return member ASNs (depth-limited)."""
        seen:    set[str] = set()
        members: set[int] = set()
        await self._expand_as_set(as_set, depth, seen, members)
        return sorted(members)

    async def _expand_as_set(self, name: str, depth: int, seen: set, members: set) -> None:
        if depth <= 0 or name in seen:
            return
        seen.add(name)
        try:
            raw = await self.query(f"-K -r as-set {name}")
        except Exception as exc:
            logger.debug("IRR as-set expand error for %s: %s", name, exc)
            return
        for line in raw.splitlines():
            if line.lower().startswith("members:"):
                for token in re.split(r"[,\s]+", line.split(":", 1)[1].strip()):
                    token = token.strip()
                    if token.upper().startswith("AS") and not token.upper().startswith("AS-"):
                        try:
                            members.add(int(token[2:]))
                        except ValueError:
                            pass
                    elif token.startswith("AS-"):
                        await self._expand_as_set(token, depth - 1, seen, members)


def _parse_route_objects(raw: str) -> List[RouteObject]:
    objects, current = [], {}
    for line in raw.splitlines():
        line = line.rstrip()
        if line.startswith("%") or line.startswith("#"):
            continue
        if not line:
            if current:
                ro = _build_route_object(current)
                if ro:
                    objects.append(ro)
                current = {}
            continue
        if ":" in line:
            key, _, value = line.partition(":")
            current[key.strip().lower()] = value.strip()
    if current:
        ro = _build_route_object(current)
        if ro:
            objects.append(ro)
    return objects


def _build_route_object(d: dict) -> Optional[RouteObject]:
    prefix = d.get("route") or d.get("route6")
    origin_raw = d.get("origin", "AS0")
    if not prefix:
        return None
    try:
        origin = int(origin_raw.lstrip("ASas"))
    except ValueError:
        return None
    return RouteObject(
        prefix=prefix,
        origin=origin,
        descr=d.get("descr", ""),
    )


# ─── RDAP client ──────────────────────────────────────────────────────────────

class RdapClient:
    """Query RDAP for ASN and prefix registration information."""

    _RDAP_ASN_BASE = "https://rdap.org/autnum/{asn}"
    _RDAP_IP_BASE  = "https://rdap.org/ip/{prefix}"

    def __init__(self, timeout: float = 10.0) -> None:
        self.timeout = timeout

    async def lookup_asn(self, asn: int) -> Optional[AsnInfo]:
        url = self._RDAP_ASN_BASE.format(asn=asn)
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.get(url)
                r.raise_for_status()
                data = r.json()
            return AsnInfo(
                asn=asn,
                name=data.get("name", ""),
                org=_rdap_org(data),
                country=_rdap_country(data),
                rir=data.get("port43", "").split(".")[0].upper(),
            )
        except Exception as exc:
            logger.debug("RDAP ASN lookup failed for AS%d: %s", asn, exc)
            return None

    async def lookup_prefix(self, prefix: str) -> Optional[RouteObject]:
        url = self._RDAP_IP_BASE.format(prefix=prefix)
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.get(url)
                r.raise_for_status()
                data = r.json()
            cidr = data.get("cidr0_cidrs", [{}])[0]
            p = cidr.get("v4prefix") or cidr.get("v6prefix")
            if not p:
                p = prefix
            return RouteObject(prefix=p, origin=0, descr=data.get("name", ""))
        except Exception as exc:
            logger.debug("RDAP prefix lookup failed for %s: %s", prefix, exc)
            return None


def _rdap_org(data: dict) -> str:
    for entity in data.get("entities", []):
        for role in entity.get("roles", []):
            if role in ("registrant", "administrative"):
                vcard = entity.get("vcardArray", [None, []])[1]
                for field in vcard:
                    if isinstance(field, list) and field[0] == "fn":
                        return field[3]
    return ""


def _rdap_country(data: dict) -> str:
    return data.get("country", "")


# ─── BGP.Tools client ─────────────────────────────────────────────────────────

class BgpToolsClient:
    """
    Query the BGP.Tools public JSON API for real-time routing visibility.

    API docs: https://bgp.tools/kb/api
    No API key required for basic queries (respectful usage: max 1 req/sec).
    """

    _BASE = "https://bgp.tools"

    def __init__(self, timeout: float = 10.0) -> None:
        self.timeout = timeout

    async def asn_summary(self, asn: int) -> Optional[AsnInfo]:
        """Return basic ASN info from BGP.Tools."""
        url = f"{self._BASE}/api/v1/whois?q=AS{asn}"
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.get(url, headers={"Accept": "application/json"})
                r.raise_for_status()
                data = r.json()
            return AsnInfo(
                asn=asn,
                name=data.get("name", ""),
                country=data.get("country", ""),
                org=data.get("descr", ""),
                upstream_asns=[int(u) for u in data.get("upstreams", []) if str(u).isdigit()],
            )
        except Exception as exc:
            logger.debug("BGP.Tools ASN summary failed for AS%d: %s", asn, exc)
            return None

    async def prefix_visibility(self, prefix: str) -> int:
        """Return number of peers that see this prefix (0 if unknown)."""
        url = f"{self._BASE}/api/v1/whois?q={prefix}"
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.get(url, headers={"Accept": "application/json"})
                r.raise_for_status()
                data = r.json()
            return int(data.get("visibility", {}).get("total", 0))
        except Exception as exc:
            logger.debug("BGP.Tools prefix visibility failed for %s: %s", prefix, exc)
            return 0


# ─── RIPE STAT client (RV8-EXT1) ─────────────────────────────────────────────

@dataclass
class RipeStatResult:
    """Aggregated result from RIPE STAT data calls."""
    prefix:           str
    announced:        bool               = False
    visibility_peers: int                = 0
    visibility_pct:   float              = 0.0
    origin_asns:      List[int]          = field(default_factory=list)
    covering_roas:    List[dict]         = field(default_factory=list)
    first_seen:       Optional[str]      = None
    last_seen:        Optional[str]      = None
    country:          str                = ""
    rir:              str                = ""
    raw:              dict               = field(default_factory=dict)


class RipeStatClient:
    """
    Query the RIPE STAT public REST API (https://stat.ripe.net/docs/02.data-api/).

    No API key required — uses the public data endpoints.
    Rate limit: ~30 req/min per IP; this client adds a small delay.
    """

    _BASE = "https://stat.ripe.net/data"

    def __init__(self, timeout: float = 15.0, source_app: str = "rustybmp") -> None:
        self.timeout    = timeout
        self.source_app = source_app

    def _headers(self) -> dict:
        return {"User-Agent": f"{self.source_app}/0.8.0"}

    async def prefix_overview(self, prefix: str) -> RipeStatResult:
        """
        Fetch prefix overview: announced status, origin ASNs, visibility.
        Combines prefix-overview and visibility data calls.
        """
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            overview_task    = client.get(
                f"{self._BASE}/prefix-overview/data.json",
                params={"resource": prefix, "sourceapp": self.source_app},
                headers=self._headers(),
            )
            visibility_task  = client.get(
                f"{self._BASE}/visibility/data.json",
                params={"resource": prefix, "sourceapp": self.source_app},
                headers=self._headers(),
            )
            rpki_task        = client.get(
                f"{self._BASE}/rpki-validation/data.json",
                params={"resource": prefix, "sourceapp": self.source_app},
                headers=self._headers(),
            )

            try:
                ov_resp, vis_resp, rpki_resp = await asyncio.gather(
                    overview_task, visibility_task, rpki_task
                )
            except Exception as exc:
                logger.warning("RIPE STAT parallel fetch failed for %s: %s", prefix, exc)
                return RipeStatResult(prefix=prefix)

        result = RipeStatResult(prefix=prefix)

        # Prefix overview
        try:
            ov = ov_resp.json().get("data", {})
            result.announced  = ov.get("announced", False)
            result.origin_asns = [int(a["asn"]) for a in ov.get("asns", []) if "asn" in a]
            result.country    = ov.get("block", {}).get("country", "")
            result.rir        = ov.get("block", {}).get("registry", "")
        except Exception as exc:
            logger.debug("RIPE STAT prefix-overview parse error for %s: %s", prefix, exc)

        # Visibility
        try:
            vis = vis_resp.json().get("data", {})
            peers            = vis.get("visibilities", [])
            if peers:
                latest = max(peers, key=lambda p: p.get("probe_ts", ""))
                result.visibility_peers = latest.get("full_table_peer_count", 0)
                total_peers             = latest.get("total_ris_peers", 0)
                if total_peers > 0:
                    result.visibility_pct = round(
                        result.visibility_peers / total_peers * 100, 1
                    )
        except Exception as exc:
            logger.debug("RIPE STAT visibility parse error for %s: %s", prefix, exc)

        # RPKI / ROAs
        try:
            rpki = rpki_resp.json().get("data", {})
            result.covering_roas = rpki.get("validating_roas", [])
        except Exception as exc:
            logger.debug("RIPE STAT rpki parse error for %s: %s", prefix, exc)

        return result

    async def routing_history(
        self,
        prefix:     str,
        start_time: Optional[str] = None,
        end_time:   Optional[str] = None,
    ) -> List[dict]:
        """
        Fetch routing history (announce/withdraw timeline) for a prefix.
        Returns a list of dicts with 'time', 'action', and 'origin_asn'.
        """
        params: dict = {"resource": prefix, "sourceapp": self.source_app}
        if start_time:
            params["starttime"] = start_time
        if end_time:
            params["endtime"] = end_time
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.get(
                    f"{self._BASE}/routing-history/data.json",
                    params=params,
                    headers=self._headers(),
                )
                r.raise_for_status()
                data = r.json().get("data", {})
            return data.get("by_origin", [])
        except Exception as exc:
            logger.debug("RIPE STAT routing-history failed for %s: %s", prefix, exc)
            return []

    async def asn_neighbours(self, asn: int) -> dict:
        """Return upstream and downstream neighbours for an ASN."""
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.get(
                    f"{self._BASE}/asn-neighbours/data.json",
                    params={"resource": f"AS{asn}", "sourceapp": self.source_app},
                    headers=self._headers(),
                )
                r.raise_for_status()
                return r.json().get("data", {})
        except Exception as exc:
            logger.debug("RIPE STAT asn-neighbours failed for AS%d: %s", asn, exc)
            return {}


# ─── Aggregate helper ─────────────────────────────────────────────────────────

async def resolve_origin(
    prefix:     str,
    origin_asn: int,
    irr_server: str = "ripe",
    timeout:    float = 10.0,
) -> OriginInfo:
    """
    Produce a full OriginInfo by querying IRR, RDAP, and BGP.Tools in parallel.
    """
    irr    = IrrClient(server=irr_server, timeout=timeout)
    rdap   = RdapClient(timeout=timeout)
    bgpt   = BgpToolsClient(timeout=timeout)

    route_objects_task  = irr.route_objects(prefix)
    asn_info_task       = rdap.lookup_asn(origin_asn)
    visibility_task     = bgpt.prefix_visibility(prefix)
    bgpt_summary_task   = bgpt.asn_summary(origin_asn)

    route_objects, asn_info, visible_peers, bgpt_summary = await asyncio.gather(
        route_objects_task,
        asn_info_task,
        visibility_task,
        bgpt_summary_task,
        return_exceptions=True,
    )

    # Merge bgpt_summary into asn_info
    if isinstance(asn_info, Exception) or asn_info is None:
        asn_info = bgpt_summary if not isinstance(bgpt_summary, Exception) else None
    elif not isinstance(bgpt_summary, Exception) and bgpt_summary:
        if not asn_info.upstream_asns:
            asn_info.upstream_asns = bgpt_summary.upstream_asns

    return OriginInfo(
        prefix=prefix,
        origin_asn=origin_asn,
        asn_info=asn_info if not isinstance(asn_info, Exception) else None,
        route_objects=route_objects if not isinstance(route_objects, Exception) else [],
        upstream_asns=asn_info.upstream_asns if asn_info and not isinstance(asn_info, Exception) else [],
        visible_peers=visible_peers if not isinstance(visible_peers, Exception) else 0,
    )
