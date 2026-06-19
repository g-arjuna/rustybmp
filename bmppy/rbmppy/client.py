"""Async HTTP client for the rustybmp REST API."""
from __future__ import annotations

from typing import Any, Optional
import httpx

from .models import (
    RouteEvent, PeerEvent, SpeakerEvent, StatEvent,
    RibEntry, SpeakerSummary, PeerSummary,
)


class RustybmpClient:
    """Async client for the rustybmp HTTP API.

    Usage::

        async with RustybmpClient("http://localhost:7878") as c:
            speakers = await c.get_speakers()
    """

    def __init__(self, base_url: str = "http://localhost:7878", timeout: float = 30.0):
        self._base = base_url.rstrip("/")
        self._timeout = timeout
        self._client: Optional[httpx.AsyncClient] = None

    async def __aenter__(self) -> RustybmpClient:
        self._client = httpx.AsyncClient(base_url=self._base, timeout=self._timeout)
        return self

    async def __aexit__(self, *_: Any) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None

    def _http(self) -> httpx.AsyncClient:
        if self._client is None:
            raise RuntimeError("Use 'async with RustybmpClient(...)' or call connect()/close()")
        return self._client

    async def connect(self) -> None:
        self._client = httpx.AsyncClient(base_url=self._base, timeout=self._timeout)

    async def close(self) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None

    # ── Speakers ──────────────────────────────────────────────────────────────

    async def get_speakers(self) -> list[SpeakerSummary]:
        r = await self._http().get("/api/speakers")
        r.raise_for_status()
        return [SpeakerSummary(**s) for s in r.json()]

    # ── Peers ─────────────────────────────────────────────────────────────────

    async def get_peers(self, speaker: Optional[str] = None) -> list[PeerSummary]:
        params = {}
        if speaker:
            params["speaker"] = speaker
        r = await self._http().get("/api/peers", params=params)
        r.raise_for_status()
        return [PeerSummary(**p) for p in r.json()]

    # ── RIB ───────────────────────────────────────────────────────────────────

    async def get_rib(
        self,
        peer: Optional[str] = None,
        prefix: Optional[str] = None,
        limit: int = 1000,
    ) -> list[RibEntry]:
        params: dict[str, Any] = {"limit": limit}
        if peer:
            params["peer"] = peer
        if prefix:
            params["prefix"] = prefix
        r = await self._http().get("/api/rib", params=params)
        r.raise_for_status()
        return [RibEntry(**e) for e in r.json()]

    # ── Events (historical) ───────────────────────────────────────────────────

    async def get_route_events(
        self,
        prefix: Optional[str] = None,
        peer: Optional[str] = None,
        limit: int = 500,
    ) -> list[RouteEvent]:
        params: dict[str, Any] = {"limit": limit}
        if prefix:
            params["prefix"] = prefix
        if peer:
            params["peer"] = peer
        r = await self._http().get("/api/events/routes", params=params)
        r.raise_for_status()
        return [RouteEvent(**e) for e in r.json()]

    async def get_peer_events(
        self,
        peer: Optional[str] = None,
        limit: int = 200,
    ) -> list[PeerEvent]:
        params: dict[str, Any] = {"limit": limit}
        if peer:
            params["peer"] = peer
        r = await self._http().get("/api/events/peers", params=params)
        r.raise_for_status()
        return [PeerEvent(**e) for e in r.json()]

    async def get_stats(
        self,
        peer: Optional[str] = None,
        limit: int = 500,
    ) -> list[StatEvent]:
        params: dict[str, Any] = {"limit": limit}
        if peer:
            params["peer"] = peer
        r = await self._http().get("/api/events/stats", params=params)
        r.raise_for_status()
        return [StatEvent(**e) for e in r.json()]

    # ── Raw query ─────────────────────────────────────────────────────────────

    async def query(self, sql: str) -> list[dict[str, Any]]:
        """Run an ad-hoc SQL query against the DuckDB store (read-only)."""
        r = await self._http().post("/api/query", json={"sql": sql})
        r.raise_for_status()
        return r.json()

    # ── Health ────────────────────────────────────────────────────────────────

    async def health(self) -> dict[str, Any]:
        r = await self._http().get("/health")
        r.raise_for_status()
        return r.json()
