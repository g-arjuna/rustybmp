"""SSE streaming client for rustybmp's /events endpoint."""
from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Optional
import httpx

from .models import SseEvent


async def stream_events(
    base_url: str = "http://localhost:7878",
    event_types: Optional[list[str]] = None,
    timeout: float = 0.0,
) -> AsyncIterator[SseEvent]:
    """Async generator that yields parsed SseEvents from /events.

    Args:
        base_url: rustybmp HTTP base URL.
        event_types: If given, only yield events whose ``event`` field matches.
        timeout: Read timeout in seconds (0.0 = no timeout, i.e. stream forever).

    Usage::

        async for ev in stream_events("http://localhost:7878", event_types=["route"]):
            print(ev.event, ev.data["prefix"])
    """
    url = base_url.rstrip("/") + "/events"
    headers = {"Accept": "text/event-stream", "Cache-Control": "no-cache"}
    client_timeout = httpx.Timeout(connect=10.0, read=timeout or None, write=None, pool=None)

    async with httpx.AsyncClient(timeout=client_timeout) as client:
        async with client.stream("GET", url, headers=headers) as resp:
            resp.raise_for_status()
            event_name = "message"
            data_lines: list[str] = []

            async for raw in resp.aiter_lines():
                line = raw.strip()
                if line.startswith("event:"):
                    event_name = line[6:].strip()
                elif line.startswith("data:"):
                    data_lines.append(line[5:].strip())
                elif line == "":
                    if data_lines:
                        payload_str = "\n".join(data_lines)
                        try:
                            payload = json.loads(payload_str)
                        except json.JSONDecodeError:
                            payload = {"raw": payload_str}
                        ev = SseEvent(event=event_name, data=payload)
                        if event_types is None or ev.event in event_types:
                            yield ev
                        event_name = "message"
                        data_lines = []


async def stream_route_events(base_url: str = "http://localhost:7878") -> AsyncIterator[SseEvent]:
    async for ev in stream_events(base_url, event_types=["route_change"]):
        yield ev


async def stream_peer_events(base_url: str = "http://localhost:7878") -> AsyncIterator[SseEvent]:
    async for ev in stream_events(base_url, event_types=["peer_up", "peer_down"]):
        yield ev
