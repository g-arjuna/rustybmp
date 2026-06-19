"""rbmppy — Python SDK and analytics companion for rustybmp."""

__version__ = "0.1.0"

from .client import RustybmpClient
from .stream import stream_events, stream_route_events, stream_peer_events
from .models import (
    RouteEvent, PeerEvent, SpeakerEvent, StatEvent,
    RibEntry, SpeakerSummary, PeerSummary, SseEvent,
)
from .analytics import RouteAnalytics
from .peering import lookup_asn, validate_prefix_rpki, NetworkInfo, RpkiResult
from .rpki import RtrVrpCache, ValidationResult, poll_rtr_cache, validate_via_api
from .internet import (
    IrrClient, RdapClient, BgpToolsClient,
    AsnInfo, RouteObject, OriginInfo, resolve_origin,
)
from .detectors import (
    Alert, DetectorPipeline,
    OriginChangeDetector, RouteLeakDetector,
    MEDOscillationDetector, BGPHijackDetector,
)

__all__ = [
    "RustybmpClient",
    "stream_events",
    "stream_route_events",
    "stream_peer_events",
    "RouteEvent",
    "PeerEvent",
    "SpeakerEvent",
    "StatEvent",
    "RibEntry",
    "SpeakerSummary",
    "PeerSummary",
    "SseEvent",
    "RouteAnalytics",
    "lookup_asn",
    "validate_prefix_rpki",
    "NetworkInfo",
    "RpkiResult",
    # rpki
    "RtrVrpCache",
    "ValidationResult",
    "poll_rtr_cache",
    "validate_via_api",
    # internet
    "IrrClient",
    "RdapClient",
    "BgpToolsClient",
    "AsnInfo",
    "RouteObject",
    "OriginInfo",
    "resolve_origin",
    # detectors
    "Alert",
    "DetectorPipeline",
    "OriginChangeDetector",
    "RouteLeakDetector",
    "MEDOscillationDetector",
    "BGPHijackDetector",
]
