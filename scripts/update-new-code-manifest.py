#!/usr/bin/env python3
"""Refresh digests for newly authored and generated files in SOURCE-MANIFEST.json."""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "SOURCE-MANIFEST.json"


def main() -> int:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    changed = subprocess.run(
        ["git", "ls-files", "--modified", "--others", "--exclude-standard"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    entries = []
    for relative in sorted(set(changed)):
        if relative == "SOURCE-MANIFEST.json":
            continue
        path = ROOT / relative
        if not path.is_file():
            continue
        entries.append(
            {
                "path": relative,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "origin": "New or generated trading-core work for CHG-2026-001; no upstream source copied.",
            }
        )
    manifest["new_code"] = entries
    MANIFEST.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
