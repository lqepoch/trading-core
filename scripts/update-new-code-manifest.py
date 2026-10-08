#!/usr/bin/env python3
"""Refresh digests for newly authored and generated files in SOURCE-MANIFEST.json."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path
from pathlib import PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "SOURCE-MANIFEST.json"
NEW_CODE_ORIGIN = (
    "New or generated trading-core work for CHG-2026-001; no upstream source copied."
)


def refreshed_new_code_entries(
    manifest: dict[str, object], changed_paths: list[str], root: Path
) -> list[dict[str, str]]:
    """Preserve registered paths and add changed files without silently losing provenance."""
    root = root.resolve()
    previous = manifest.get("new_code", [])
    if not isinstance(previous, list):
        raise ValueError("SOURCE-MANIFEST.json new_code must be an array")
    entries_by_path: dict[str, dict[str, str]] = {}
    for entry in previous:
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
            raise ValueError("SOURCE-MANIFEST.json contains an invalid new_code entry")
        relative = entry["path"]
        if relative in entries_by_path:
            raise ValueError(f"SOURCE-MANIFEST.json contains duplicate new_code path: {relative}")
        _manifest_path(root, relative)
        entries_by_path[entry["path"]] = dict(entry)

    missing = sorted(
        relative for relative in entries_by_path if not _manifest_path(root, relative).is_file()
    )
    if missing:
        raise ValueError(
            "registered new_code paths were removed; remove them explicitly and record the "
            "migration or retirement in SOURCE-MANIFEST.json before refreshing: "
            + ", ".join(missing)
        )

    for relative, entry in entries_by_path.items():
        entry["sha256"] = hashlib.sha256(_manifest_path(root, relative).read_bytes()).hexdigest()

    for relative in sorted(set(changed_paths)):
        if relative == "SOURCE-MANIFEST.json":
            continue
        path = _manifest_path(root, relative)
        if not path.is_file():
            continue
        entry = entries_by_path.get(relative, {"path": relative, "origin": NEW_CODE_ORIGIN})
        entry["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        entries_by_path[relative] = entry

    return [entries_by_path[path] for path in sorted(entries_by_path)]


def _manifest_path(root: Path, relative: str) -> Path:
    if not relative or "\\" in relative:
        raise ValueError(f"new_code path is not a canonical repository-relative path: {relative!r}")
    pure = PurePosixPath(relative)
    windows_drive_path = (
        bool(pure.parts)
        and len(pure.parts[0]) >= 2
        and pure.parts[0][0].isalpha()
        and pure.parts[0][1] == ":"
    )
    if (
        not pure.parts
        or pure.is_absolute()
        or pure.as_posix() != relative
        or windows_drive_path
        or any(part in {"", ".", ".."} for part in pure.parts)
    ):
        raise ValueError(f"new_code path escapes or aliases the repository root: {relative!r}")
    candidate = root.joinpath(*pure.parts)
    cursor = root
    for part in pure.parts:
        cursor = cursor / part
        if cursor.is_symlink():
            raise ValueError(f"new_code paths must not traverse symlinks: {relative!r}")
    resolved = candidate.resolve(strict=False)
    if os.path.commonpath((root, resolved)) != str(root):
        raise ValueError(f"new_code path escapes the repository root: {relative!r}")
    return candidate


def main() -> int:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    changed = subprocess.run(
        ["git", "ls-files", "--modified", "--others", "--exclude-standard"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    manifest["new_code"] = refreshed_new_code_entries(manifest, changed, ROOT)
    MANIFEST.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
