"""PeeringDB lookup and RPKI validation stubs."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional
import httpx


@dataclass
class NetworkInfo:
    asn: int
    name: str
    irr_as_set: Optional[str] = None
    policy_general: Optional[str] = None
    info_prefixes4: int = 0
    info_prefixes6: int = 0


@dataclass
class RpkiResult:
    prefix: str
    origin_asn: int
    state: str  # "valid" | "invalid" | "unknown"
    roa_count: int = 0
    covering_roas: list[dict] = field(default_factory=list)


async def lookup_asn(asn: int, timeout: float = 10.0) -> Optional[NetworkInfo]:
    """Query PeeringDB for basic AS metadata."""
    url = f"https://www.peeringdb.com/api/net?asn={asn}"
    try:
        async with httpx.AsyncClient(timeout=timeout) as client:
            r = await client.get(url, headers={"Accept": "application/json"})
            r.raise_for_status()
            data = r.json().get("data", [])
            if not data:
                return None
            net = data[0]
            return NetworkInfo(
                asn=asn,
                name=net.get("name", ""),
                irr_as_set=net.get("irr_as_set") or None,
                policy_general=net.get("policy_general") or None,
                info_prefixes4=net.get("info_prefixes4", 0),
                info_prefixes6=net.get("info_prefixes6", 0),
            )
    except httpx.HTTPError:
        return None


async def validate_prefix_rpki(
    prefix: str,
    origin_asn: int,
    timeout: float = 10.0,
) -> RpkiResult:
    """Check prefix/origin validity against Cloudflare's RPKI validator."""
    url = "https://rpki.cloudflare.com/api/v1/validity"
    params = {"prefix": prefix, "asn": str(origin_asn)}
    try:
        async with httpx.AsyncClient(timeout=timeout) as client:
            r = await client.get(url, params=params)
            r.raise_for_status()
            body = r.json()
            state = body.get("validated_route", {}).get("validity", {}).get("state", "unknown")
            roas = body.get("validated_route", {}).get("validity", {}).get("matching_roas", [])
            return RpkiResult(
                prefix=prefix,
                origin_asn=origin_asn,
                state=state,
                roa_count=len(roas),
                covering_roas=roas,
            )
    except httpx.HTTPError:
        return RpkiResult(prefix=prefix, origin_asn=origin_asn, state="unknown")
