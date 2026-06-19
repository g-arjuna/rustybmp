"""Pydantic models mirroring rustybmp's JSON API responses."""
from __future__ import annotations

from datetime import datetime
from typing import Any, Optional
from pydantic import BaseModel, field_validator


class RouteEvent(BaseModel):
    id: str
    occurred_at: datetime
    speaker_addr: str
    peer_addr: str
    peer_as: int
    rib_type: str
    action: str  # "announce" | "withdraw"
    prefix: str
    afi: str
    origin: Optional[str] = None
    as_path: Optional[str] = None
    as_path_len: Optional[int] = None
    next_hop: Optional[str] = None
    local_pref: Optional[int] = None
    med: Optional[int] = None
    communities: Optional[str] = None
    ext_communities: Optional[str] = None
    large_communities: Optional[str] = None
    originator_id: Optional[str] = None
    atomic_aggregate: bool = False

    @property
    def community_list(self) -> list[str]:
        if not self.communities:
            return []
        return [c for c in self.communities.split(",") if c]


class PeerEvent(BaseModel):
    id: str
    occurred_at: datetime
    speaker_addr: str
    peer_addr: str
    peer_as: Optional[int] = None
    event_type: str  # "peer_up" | "peer_down"
    local_as: Optional[int] = None
    hold_time: Optional[int] = None
    capabilities: Optional[list[Any]] = None
    reason: Optional[str] = None


class SpeakerEvent(BaseModel):
    id: str
    occurred_at: datetime
    speaker_addr: str
    event_type: str  # "speaker_up" | "speaker_down"
    sys_name: Optional[str] = None
    sys_descr: Optional[str] = None
    reason: Optional[str] = None


class StatEvent(BaseModel):
    id: str
    occurred_at: datetime
    speaker_addr: str
    peer_addr: str
    counter_name: str
    counter_value: int
    stat_type: Optional[int] = None
    afi: Optional[int] = None
    safi: Optional[int] = None


class RibEntry(BaseModel):
    prefix: str
    peer_addr: str
    peer_as: int
    rib_type: str
    is_best: bool = True
    path_id: Optional[int] = None
    origin: Optional[str] = None
    as_path: Optional[str] = None
    next_hop: Optional[str] = None
    local_pref: Optional[int] = None
    med: Optional[int] = None
    communities: Optional[str] = None


class SpeakerSummary(BaseModel):
    addr: str
    sys_name: Optional[str] = None
    sys_descr: Optional[str] = None
    peer_count: int = 0


class PeerSummary(BaseModel):
    peer_addr: str
    peer_as: int
    rib_type: str
    state: str  # "up" | "down"
    prefix_count: int = 0
    hold_time: Optional[int] = None


class SseEvent(BaseModel):
    """Parsed SSE event from the /events stream."""
    event: str
    data: dict[str, Any]
