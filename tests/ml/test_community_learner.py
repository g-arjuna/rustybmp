"""
Bundle D5 — CommunitySemanticLearner unit tests.
"""
from __future__ import annotations

import pytest
import pandas as pd

from bmppy.ml.community_learner import CommunitySemanticLearner, CommunitySemantics


class TestCommunitySemanticLearner:
    @pytest.fixture
    def learner(self) -> CommunitySemanticLearner:
        return CommunitySemanticLearner(min_support=0.01, min_confidence=0.70)

    def test_empty_df_returns_empty(self, learner: CommunitySemanticLearner) -> None:
        df = pd.DataFrame(columns=["community", "local_pref", "action", "route_count"])
        results = learner.learn_from_dataframe(df)
        assert results == []

    def test_blackhole_detected(self, learner: CommunitySemanticLearner) -> None:
        df = pd.DataFrame({
            "community": ["65001:666"] * 10,
            "local_pref": [100] * 10,
            "action": ["announce"] * 10,
            "route_count": [1] * 10,
        })
        results = learner.learn_from_dataframe(df)
        meanings = {r.community: r.inferred_meaning for r in results}
        assert "65001:666" in meanings
        assert meanings["65001:666"] == "blackhole"

    def test_no_export_detected(self, learner: CommunitySemanticLearner) -> None:
        df = pd.DataFrame({
            "community": ["65535:65281"] * 10,
            "local_pref": [100] * 10,
            "action": ["announce"] * 10,
            "route_count": [1] * 10,
        })
        results = learner.learn_from_dataframe(df)
        meanings = {r.community: r.inferred_meaning for r in results}
        assert "65535:65281" in meanings
        assert meanings["65535:65281"] == "no-export"

    def test_high_local_pref_infers_preferred(self, learner: CommunitySemanticLearner) -> None:
        # 50 routes with community "65001:100" all having LP=200
        # + 50 routes with other community for support denominator
        df = pd.DataFrame({
            "community": ["65001:100"] * 50 + ["65001:999"] * 50,
            "local_pref": [200] * 50 + [100] * 50,
            "action": ["announce"] * 100,
            "route_count": [1] * 100,
        })
        results = learner.learn_from_dataframe(df)
        preferred = [r for r in results if "preferred" in r.inferred_meaning]
        assert len(preferred) >= 1
        assert preferred[0].community == "65001:100"

    def test_low_local_pref_infers_backup(self, learner: CommunitySemanticLearner) -> None:
        df = pd.DataFrame({
            "community": ["65001:200"] * 50 + ["65001:999"] * 50,
            "local_pref": [60] * 50 + [100] * 50,
            "action": ["announce"] * 100,
            "route_count": [1] * 100,
        })
        results = learner.learn_from_dataframe(df)
        backup = [r for r in results if "backup" in r.inferred_meaning]
        assert len(backup) >= 1

    def test_withdraw_heavy_infers_filter(self, learner: CommunitySemanticLearner) -> None:
        df = pd.DataFrame({
            "community": ["65001:300"] * 50 + ["65001:999"] * 50,
            "local_pref": [100] * 100,
            "action": ["withdraw"] * 45 + ["announce"] * 5 + ["announce"] * 50,
            "route_count": [1] * 100,
        })
        results = learner.learn_from_dataframe(df)
        filters = [r for r in results if "filter" in r.inferred_meaning or "deny" in r.inferred_meaning]
        assert len(filters) >= 1

    def test_confidence_threshold_respected(self) -> None:
        learner = CommunitySemanticLearner(min_confidence=0.99)
        df = pd.DataFrame({
            "community": ["65001:100"] * 50 + ["65001:999"] * 50,
            "local_pref": [200] * 40 + [100] * 10 + [100] * 50,
            "action": ["announce"] * 100,
            "route_count": [1] * 100,
        })
        results = learner.learn_from_dataframe(df)
        # With inconsistent LP values, confidence < 0.99
        preferred = [r for r in results if "preferred" in r.inferred_meaning]
        assert len(preferred) == 0

    def test_results_stored_on_learner(self, learner: CommunitySemanticLearner) -> None:
        df = pd.DataFrame({
            "community": ["65001:666"] * 10,
            "local_pref": [100] * 10,
            "action": ["announce"] * 10,
            "route_count": [1] * 10,
        })
        learner.learn_from_dataframe(df)
        assert len(learner.results) > 0
