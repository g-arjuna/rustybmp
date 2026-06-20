"""Rolling Parquet archive with latest symlinks (RV4-4 T4).

Adapted from bonsai's parquet_store.py architecture.

Directory layout::

    ml/data/
      route_anomaly/
        2026-06-20T10-00-00Z_v1_45230rows.parquet
        latest -> 2026-06-20T10-00-00Z_v1_45230rows.parquet
      peer_stability/
        2026-06-20T10-00-00Z_v1_88peers.parquet
        latest -> ...
      bgp_snapshots/
        2026-06-20T10-00-00Z_T8_snapshots.arrow
        latest -> ...

Usage::

    from bmppy.ml.parquet_store import ParquetStore
    from rbmppy.parquet import export_route_features

    store = ParquetStore("ml/data")
    path  = store.new_path("route_anomaly", rows=45230, version=1, ext="parquet")
    export_route_features("runtime/routes.duckdb", str(path), days=7)
    store.update_latest("route_anomaly", path)

    latest = store.latest("route_anomaly")  # Path or None
"""
from __future__ import annotations

import os
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional


class ParquetStore:
    """Rolling archive of ML training artefacts."""

    def __init__(self, base_dir: str = "ml/data") -> None:
        self.base = Path(base_dir)

    def new_path(
        self,
        category: str,
        rows: int,
        version: int = 1,
        ext: str = "parquet",
        tag: Optional[str] = None,
    ) -> Path:
        """Generate a timestamped file path inside *category* directory."""
        ts  = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
        tag_part = f"_{tag}" if tag else f"_v{version}_{rows}rows"
        filename = f"{ts}{tag_part}.{ext}"
        dest = self.base / category
        dest.mkdir(parents=True, exist_ok=True)
        return dest / filename

    def update_latest(self, category: str, path: Path) -> None:
        """Atomically update the 'latest' symlink for *category*."""
        symlink = self.base / category / "latest"
        tmp = symlink.with_suffix(".tmp")
        if tmp.exists() or tmp.is_symlink():
            tmp.unlink()
        os.symlink(path.name, tmp)
        os.replace(tmp, symlink)

    def latest(self, category: str) -> Optional[Path]:
        """Return the Path pointed to by the 'latest' symlink, or None."""
        symlink = self.base / category / "latest"
        if symlink.is_symlink():
            target = symlink.parent / os.readlink(symlink)
            return target if target.exists() else None
        return None

    def list_files(self, category: str) -> list[Path]:
        """List all non-symlink files in *category*, newest first."""
        d = self.base / category
        if not d.exists():
            return []
        files = sorted(
            (f for f in d.iterdir() if f.is_file() and not f.name.startswith("latest")),
            key=lambda p: p.stat().st_mtime,
            reverse=True,
        )
        return files

    def prune(self, category: str, keep: int = 10) -> int:
        """Delete oldest files in *category*, keeping the *keep* most recent. Returns count deleted."""
        files = self.list_files(category)
        to_delete = files[keep:]
        for f in to_delete:
            f.unlink(missing_ok=True)
        return len(to_delete)
