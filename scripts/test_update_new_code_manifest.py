#!/usr/bin/env python3
"""Deterministic incremental tests for SOURCE-MANIFEST new_code refreshes."""

from __future__ import annotations

import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().with_name("update-new-code-manifest.py")
SPEC = importlib.util.spec_from_file_location("new_code_manifest", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("failed to load the source-manifest update tool")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
refreshed_new_code_entries = MODULE.refreshed_new_code_entries


class NewCodeManifestTest(unittest.TestCase):
    def test_first_refresh_registers_new_paths_and_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            content = b"synthetic schema v1\n"
            (root / "schemas.json").write_bytes(content)

            actual = refreshed_new_code_entries({}, ["schemas.json"], root)

        self.assertEqual(actual[0]["path"], "schemas.json")
        self.assertEqual(actual[0]["sha256"], hashlib.sha256(content).hexdigest())
        self.assertIn("no upstream source copied", actual[0]["origin"])

    def test_second_refresh_preserves_existing_paths_and_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "first.rs").write_bytes(b"old")
            first = refreshed_new_code_entries({}, ["first.rs"], root)
            (root / "first.rs").write_bytes(b"updated")
            (root / "second.ts").write_bytes(b"generated")

            manifest = {"new_code": first}
            actual = refreshed_new_code_entries(
                manifest, ["second.ts", "first.rs", "second.ts"], root
            )
            repeated = refreshed_new_code_entries(manifest, ["first.rs", "second.ts"], root)

        self.assertEqual([entry["path"] for entry in actual], ["first.rs", "second.ts"])
        self.assertEqual(actual, repeated)
        self.assertEqual(actual[0]["sha256"], hashlib.sha256(b"updated").hexdigest())

    def test_deleted_registered_path_requires_explicit_retirement_record(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {
                "new_code": [
                    {
                        "path": "removed.rs",
                        "sha256": "a" * 64,
                        "origin": "prior project-owned source",
                    }
                ]
            }

            with self.assertRaisesRegex(ValueError, "record the migration or retirement"):
                refreshed_new_code_entries(manifest, [], root)

        self.assertEqual(manifest["new_code"][0]["path"], "removed.rs")

    def test_invalid_registered_paths_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary_root = Path(directory)
            root = temporary_root / "repo"
            root.mkdir()
            outside = temporary_root / "outside.txt"
            outside.write_text("outside", encoding="utf-8")
            (root / "link.txt").symlink_to(outside)
            outside_dir = temporary_root / "outside-dir"
            outside_dir.mkdir()
            (outside_dir / "file.txt").write_text("outside", encoding="utf-8")
            (root / "link-dir").symlink_to(outside_dir, target_is_directory=True)

            for relative in (
                str(outside),
                "../outside.txt",
                "C:/outside.txt",
                ".",
                "folder\\outside.txt",
                "link.txt",
                "link-dir/file.txt",
            ):
                with self.subTest(path=relative), self.assertRaises(ValueError):
                    refreshed_new_code_entries(
                        {"new_code": [{"path": relative, "sha256": "a" * 64}]}, [], root
                    )

    def test_duplicate_registered_paths_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "same.rs").write_bytes(b"same")
            duplicate = {"new_code": [{"path": "same.rs"}, {"path": "same.rs"}]}

            with self.assertRaisesRegex(ValueError, "duplicate new_code path"):
                refreshed_new_code_entries(duplicate, [], root)


if __name__ == "__main__":
    unittest.main()
