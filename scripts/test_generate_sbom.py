#!/usr/bin/env python3
"""Regression tests for the locked SPDX dependency graph generator."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

MODULE_PATH = Path(__file__).resolve().with_name("generate-sbom.py")
SPEC = importlib.util.spec_from_file_location("generate_sbom", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("failed to load the SPDX SBOM generator")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class GenerateSbomTest(unittest.TestCase):
    def test_local_cargo_identity_uses_repository_relative_manifest_path(self) -> None:
        package = {
            "id": "path+file:///checkout/crates/domain#0.1.0",
            "manifest_path": str(MODULE.ROOT / "crates/domain/Cargo.toml"),
            "name": "domain",
            "source": None,
            "version": "0.1.0",
        }

        self.assertEqual(
            MODULE.cargo_identity(package),
            "path:crates/domain/Cargo.toml#domain@0.1.0",
        )

    def test_scoped_npm_package_purl_preserves_namespace_separator(self) -> None:
        record = MODULE.package_record(
            identity="npm:@scope/package@1.2.3",
            name="@scope/package",
            version="1.2.3",
            ecosystem="npm",
        )

        self.assertEqual(
            record["externalRefs"][0]["referenceLocator"],
            "pkg:npm/%40scope/package@1.2.3",
        )

    def test_locked_peer_dependency_has_depends_on_relationship(self) -> None:
        records, relationships, _ = MODULE.npm_graph()
        lock = json.loads(
            (MODULE.ROOT / "typescript/package-lock.json").read_text(encoding="utf-8")
        )
        packages = lock["packages"]
        vfs_path = "node_modules/@typescript/vfs"
        peer_name = "typescript"
        vfs = packages[vfs_path]
        peer_path = f"node_modules/{peer_name}"
        peer = packages[peer_path]

        vfs_id = MODULE.spdx_id(f"npm:{vfs_path}@{vfs['version']}")
        peer_id = MODULE.spdx_id(f"npm:{peer_path}@{peer['version']}")

        self.assertIn(peer_name, vfs["peerDependencies"])
        self.assertTrue(any(record["SPDXID"] == vfs_id for record in records))
        self.assertTrue(any(record["SPDXID"] == peer_id for record in records))
        self.assertIn(
            {
                "spdxElementId": vfs_id,
                "relationshipType": "DEPENDS_ON",
                "relatedSpdxElement": peer_id,
            },
            relationships,
        )

    def test_root_peer_dependency_is_connected_when_locked(self) -> None:
        lock = {
            "packages": {
                "": {
                    "name": "@example/root",
                    "peerDependencies": {"@example/sdk": "^1.0.0"},
                },
                "node_modules/@example/sdk": {"version": "1.0.0"},
            }
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lock_path = root / "typescript/package-lock.json"
            lock_path.parent.mkdir(parents=True)
            lock_path.write_text(json.dumps(lock), encoding="utf-8")
            with patch.object(MODULE, "ROOT", root):
                _, relationships, _ = MODULE.npm_graph()

        root_id = MODULE.spdx_id("npm:@example/root@UNVERSIONED")
        sdk_id = MODULE.spdx_id("npm:node_modules/@example/sdk@1.0.0")
        self.assertIn(
            {
                "spdxElementId": root_id,
                "relationshipType": "DEPENDS_ON",
                "relatedSpdxElement": sdk_id,
            },
            relationships,
        )


if __name__ == "__main__":
    unittest.main()
