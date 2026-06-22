"""Tests for bmppy/rbmppy/policy_advisor.py (RV9-NEW7)"""
import pytest
from bmppy.rbmppy.policy_advisor import (
    PolicyAdvisor,
    RouteCtx,
    SuggestionKind,
)


def make_route(**kwargs) -> RouteCtx:
    defaults = dict(
        prefix="203.0.113.0/24",
        peer_as=65001,
        as_path=[65001],
        communities=[],
        rpki_state="not_found",
        aspa_verdict="unknown",
        rib_type="pre-policy",
        filter_action="accept",
        local_pref=100,
        as_path_len=1,
    )
    defaults.update(kwargs)
    return RouteCtx(**defaults)


@pytest.fixture
def advisor():
    return PolicyAdvisor(community_semantics={"65001:100": "preferred transit"})


class TestShouldReject:
    def test_rpki_invalid_accepted_flagged(self, advisor):
        route = make_route(rpki_state="invalid", filter_action="accept")
        suggestions = advisor.analyze_filter_gaps([route])
        assert len(suggestions) == 1
        assert suggestions[0].kind == SuggestionKind.SHOULD_REJECT
        assert suggestions[0].confidence >= 0.90

    def test_aspa_invalid_flagged(self, advisor):
        route = make_route(aspa_verdict="invalid", filter_action="accept")
        suggestions = advisor.analyze_filter_gaps([route])
        assert len(suggestions) == 1
        assert suggestions[0].kind == SuggestionKind.SHOULD_REJECT

    def test_private_asn_in_path_flagged(self, advisor):
        route = make_route(as_path=[65001, 64512], rpki_state="not_found", filter_action="accept")
        suggestions = advisor.analyze_filter_gaps([route])
        assert len(suggestions) == 1
        assert suggestions[0].kind == SuggestionKind.SHOULD_REJECT

    def test_long_aspath_flagged(self, advisor):
        route = make_route(as_path=list(range(65001, 65027)), as_path_len=26, filter_action="accept")
        suggestions = advisor.analyze_filter_gaps([route])
        assert any(s.kind == SuggestionKind.SHOULD_REJECT for s in suggestions)

    def test_clean_route_not_flagged(self, advisor):
        route = make_route(rpki_state="valid", aspa_verdict="valid", filter_action="accept")
        suggestions = advisor.analyze_filter_gaps([route])
        assert all(s.kind != SuggestionKind.SHOULD_REJECT for s in suggestions)

    def test_roto_snippet_contains_reject(self, advisor):
        route = make_route(rpki_state="invalid", filter_action="accept")
        suggestions = advisor.analyze_filter_gaps([route])
        assert "reject" in suggestions[0].roto_snippet


class TestShouldAccept:
    def test_rpki_valid_rejected_flagged(self, advisor):
        route = make_route(rpki_state="valid", filter_action="reject")
        suggestions = advisor.analyze_filter_gaps([route])
        assert len(suggestions) == 1
        assert suggestions[0].kind == SuggestionKind.SHOULD_ACCEPT
        assert suggestions[0].confidence >= 0.70

    def test_known_community_rejected_flagged(self, advisor):
        route = make_route(communities=["65001:100"], filter_action="reject", rpki_state="not_found")
        suggestions = advisor.analyze_filter_gaps([route])
        assert len(suggestions) == 1
        assert suggestions[0].kind == SuggestionKind.SHOULD_ACCEPT

    def test_roto_snippet_contains_accept(self, advisor):
        route = make_route(rpki_state="valid", filter_action="reject")
        suggestions = advisor.analyze_filter_gaps([route])
        assert "accept" in suggestions[0].roto_snippet


class TestFallThrough:
    def test_fall_through_flagged(self, advisor):
        route = make_route(filter_action="fall_through")
        suggestions = advisor.analyze_filter_gaps([route])
        assert len(suggestions) == 1
        assert suggestions[0].kind == SuggestionKind.FALL_THROUGH
        assert suggestions[0].confidence == 0.50

    def test_fall_through_snippet_has_todo(self, advisor):
        route = make_route(filter_action="fall_through")
        suggestions = advisor.analyze_filter_gaps([route])
        assert "TODO" in suggestions[0].roto_snippet


class TestExplainRejection:
    def test_rpki_invalid_explanation(self, advisor):
        route = make_route(rpki_state="invalid", filter_action="reject")
        explanation = advisor.explain_rejection(route)
        assert "INVALID" in explanation

    def test_aspa_invalid_explanation(self, advisor):
        route = make_route(aspa_verdict="invalid", filter_action="reject")
        explanation = advisor.explain_rejection(route)
        assert "ASPA" in explanation

    def test_private_asn_explanation(self, advisor):
        route = make_route(as_path=[64512], filter_action="reject")
        explanation = advisor.explain_rejection(route)
        assert "private" in explanation.lower()

    def test_long_aspath_explanation(self, advisor):
        route = make_route(as_path_len=25, filter_action="reject")
        explanation = advisor.explain_rejection(route)
        assert "long" in explanation.lower() or "hops" in explanation

    def test_blackhole_community_explanation(self, advisor):
        route = make_route(communities=["65535:666"], filter_action="reject")
        explanation = advisor.explain_rejection(route)
        assert "blackhole" in explanation.lower() or "666" in explanation

    def test_no_reason_fallback(self, advisor):
        route = make_route(filter_action="reject")
        explanation = advisor.explain_rejection(route)
        assert len(explanation) > 0


class TestSorting:
    def test_suggestions_sorted_by_confidence_desc(self, advisor):
        routes = [
            make_route(rpki_state="invalid", filter_action="accept"),       # high conf
            make_route(filter_action="fall_through"),                       # 0.50 conf
            make_route(rpki_state="valid", filter_action="reject"),         # ~0.80 conf
        ]
        suggestions = advisor.analyze_filter_gaps(routes)
        confidences = [s.confidence for s in suggestions]
        assert confidences == sorted(confidences, reverse=True)
