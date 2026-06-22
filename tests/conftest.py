"""
Root conftest for rustybmp pytest suites (Bundles B1/B2).

Provides shared fixtures:
  - FIXTURES_DIR : Path to tests/fixtures/bmp/
  - BASE_URL     : Server URL (default http://localhost:7878)
  - bmp_fixture  : helper that loads a .bin fixture by stem name
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Callable

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES_DIR = REPO_ROOT / "tests" / "fixtures" / "bmp"
BASE_URL = os.environ.get("RUSTYBMP_URL", "http://localhost:7878")


@pytest.fixture
def fixtures_dir() -> Path:
    """Return the path to the BMP binary fixtures directory."""
    return FIXTURES_DIR


@pytest.fixture
def bmp_fixture() -> Callable[[str], bytes]:
    """Return a helper that loads a fixture .bin by stem name.

    Usage::

        def test_foo(bmp_fixture):
            data = bmp_fixture("01_initiation")
    """

    def _load(stem: str) -> bytes:
        path = FIXTURES_DIR / f"{stem}.bin"
        assert path.exists(), f"Fixture not found: {path}"
        return path.read_bytes()

    return _load


@pytest.fixture
def base_url() -> str:
    """Return the rustybmp server base URL."""
    return BASE_URL
