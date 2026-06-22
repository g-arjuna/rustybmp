"""
Bundle D3 — HijackClassifier unit tests.

Tests heuristic scoring, feature construction, and model loading fallback.
No sklearn required for heuristic-only tests.
"""
from __future__ import annotations

import numpy as np
import pytest

from bmppy.ml.hijack_classifier import (
    HijackClassifier,
    HijackFeatures,
    HijackPrediction,
    FEATURE_NAMES,
)


class TestHijackFeatures:
    def test_feature_count(self) -> None:
        assert len(FEATURE_NAMES) == 7

    def test_to_array_shape(self) -> None:
        f = HijackFeatures()
        arr = f.to_array()
        assert arr.shape == (7,)
        assert arr.dtype == np.float64

    def test_defaults_are_benign(self) -> None:
        f = HijackFeatures()
        assert not f.origin_asn_changed
        assert f.peer_as_is_expected

    def test_rpki_invalid_array_value(self) -> None:
        f = HijackFeatures(rpki_validity_enc=2)
        assert f.to_array()[2] == 2.0


class TestHeuristicScoring:
    def test_benign_score_low(self) -> None:
        clf = HijackClassifier()
        f = HijackFeatures()  # all defaults = benign
        pred = clf.predict(f)
        assert pred.probability < 0.2
        assert not pred.is_hijack

    def test_origin_change_rpki_invalid_scores_high(self) -> None:
        clf = HijackClassifier()
        f = HijackFeatures(
            origin_asn_changed=True,
            rpki_validity_enc=2,  # invalid
            is_subprefix_of_known=True,
            peer_as_is_expected=False,
        )
        pred = clf.predict(f)
        assert pred.probability >= 0.5
        assert pred.is_hijack

    def test_origin_change_rpki_valid_moderate(self) -> None:
        clf = HijackClassifier()
        f = HijackFeatures(
            origin_asn_changed=True,
            rpki_validity_enc=0,  # valid
            peer_as_is_expected=True,
        )
        pred = clf.predict(f)
        # Origin changed but RPKI valid + expected peer = low-ish
        assert pred.probability < 0.3

    def test_path_shortening_adds_score(self) -> None:
        clf = HijackClassifier()
        f1 = HijackFeatures(origin_asn_changed=True, peer_as_is_expected=False)
        f2 = HijackFeatures(origin_asn_changed=True, peer_as_is_expected=False, as_path_len_delta=-3)
        p1 = clf.predict(f1)
        p2 = clf.predict(f2)
        assert p2.probability > p1.probability

    def test_aspa_invalid_adds_score(self) -> None:
        clf = HijackClassifier()
        f1 = HijackFeatures(origin_asn_changed=True, peer_as_is_expected=False)
        f2 = HijackFeatures(origin_asn_changed=True, peer_as_is_expected=False, aspa_verdict_enc=2)
        p1 = clf.predict(f1)
        p2 = clf.predict(f2)
        assert p2.probability > p1.probability

    def test_heuristic_score_clamped_to_0_1(self) -> None:
        clf = HijackClassifier()
        # Max everything bad
        f = HijackFeatures(
            origin_asn_changed=True,
            rpki_validity_enc=2,
            is_subprefix_of_known=True,
            as_path_len_delta=-10,
            aspa_verdict_enc=2,
            peer_as_is_expected=False,
        )
        pred = clf.predict(f)
        assert 0.0 <= pred.probability <= 1.0


class TestClassifierFallback:
    def test_nonexistent_model_uses_heuristic(self) -> None:
        clf = HijackClassifier(model_path="/nonexistent/model.pkl")
        assert not clf.is_ml
        assert clf._model_version == "heuristic"

    def test_none_model_uses_heuristic(self) -> None:
        clf = HijackClassifier(model_path=None)
        assert not clf.is_ml

    def test_prediction_includes_model_version(self) -> None:
        clf = HijackClassifier()
        pred = clf.predict(HijackFeatures())
        assert pred.model_version == "heuristic"

    def test_batch_prediction(self) -> None:
        clf = HijackClassifier()
        features = [HijackFeatures(), HijackFeatures(origin_asn_changed=True)]
        preds = clf.predict_batch(features)
        assert len(preds) == 2
        assert preds[1].probability > preds[0].probability

    def test_custom_threshold(self) -> None:
        clf = HijackClassifier(threshold=0.9)
        f = HijackFeatures(origin_asn_changed=True, rpki_validity_enc=2, peer_as_is_expected=False)
        pred = clf.predict(f)
        # Score ~0.55 should be below 0.9 threshold
        assert not pred.is_hijack
