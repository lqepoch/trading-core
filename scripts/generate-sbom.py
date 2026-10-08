#!/usr/bin/env python3
"""Generate an SPDX 2.3 SBOM from the locked Rust, Python, and npm graphs."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import re
import subprocess
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "sbom.spdx.json"
PYPI_LICENSE_OVERRIDES = {
    # Verified from the metadata of the exact locked protobuf release.
    ("protobuf", "7.36.2"): "BSD-3-Clause",
}


def spdx_id(identity: str) -> str:
    return "SPDXRef-Package-" + hashlib.sha256(identity.encode("utf-8")).hexdigest()[:24]


def created_timestamp() -> str:
    epoch = os.environ.get("SOURCE_DATE_EPOCH")
    value = datetime.fromtimestamp(int(epoch), timezone.utc) if epoch else datetime.now(timezone.utc)
    return value.replace(microsecond=0).isoformat().replace("+00:00", "Z")


def package_record(
    *,
    identity: str,
    name: str,
    version: str,
    ecosystem: str,
    license_declared: str = "NOASSERTION",
    download_location: str = "NOASSERTION",
    checksum: tuple[str, str] | None = None,
    purpose: str = "LIBRARY",
) -> dict[str, object]:
    if ecosystem == "cargo":
        purl = f"pkg:cargo/{quote(name, safe='-._~')}@{version}"
    elif ecosystem == "pypi":
        purl = f"pkg:pypi/{quote(name, safe='-._~')}@{version}"
    elif ecosystem == "npm":
        escaped_name = quote(name, safe="-._~")
        purl = f"pkg:npm/{escaped_name}@{version}" if version != "UNVERSIONED" else f"pkg:npm/{escaped_name}"
    else:
        raise ValueError(f"unsupported package ecosystem: {ecosystem}")

    record: dict[str, object] = {
        "SPDXID": spdx_id(identity),
        "name": name,
        "versionInfo": version,
        "downloadLocation": download_location,
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": license_declared or "NOASSERTION",
        "copyrightText": "NOASSERTION",
        "primaryPackagePurpose": purpose,
        "externalRefs": [
            {
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": purl,
            }
        ],
    }
    if checksum:
        algorithm, value = checksum
        record["checksums"] = [{"algorithm": algorithm, "checksumValue": value}]
    return record


def dependency_relationships(package_id: str, dependencies: set[str]) -> list[dict[str, str]]:
    return [
        {
            "spdxElementId": package_id,
            "relationshipType": "DEPENDS_ON",
            "relatedSpdxElement": dependency_id,
        }
        for dependency_id in sorted(dependencies)
    ]


def cargo_graph() -> tuple[list[dict[str, object]], list[dict[str, str]], set[str]]:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "+1.98.1", "metadata", "--locked", "--format-version=1"],
            cwd=ROOT,
            text=True,
        )
    )
    packages = metadata["packages"]
    ids = {package["id"]: spdx_id(package["id"]) for package in packages}
    workspace_ids = set(metadata["workspace_members"])
    records: list[dict[str, object]] = []
    for package in sorted(packages, key=lambda item: (item["name"], item["version"], item["id"])):
        source = package.get("source") or ""
        location = "NOASSERTION"
        if source.startswith("registry+"):
            location = f"https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download"
        elif source.startswith("git+"):
            location = source.removeprefix("git+")
        checksum = package.get("checksum")
        records.append(
            package_record(
                identity=package["id"],
                name=package["name"],
                version=package["version"],
                ecosystem="cargo",
                license_declared=package.get("license") or "NOASSERTION",
                download_location=location,
                checksum=("SHA256", checksum) if checksum else None,
                purpose="APPLICATION" if package["id"] in workspace_ids else "LIBRARY",
            )
        )

    relationships: list[dict[str, str]] = []
    for node in metadata.get("resolve", {}).get("nodes", []):
        dependencies = {ids[dependency["pkg"]] for dependency in node.get("deps", [])}
        relationships.extend(dependency_relationships(ids[node["id"]], dependencies))
    return records, relationships, {ids[package_id] for package_id in workspace_ids}


def python_graph() -> tuple[list[dict[str, object]], list[dict[str, str]], set[str]]:
    lock = tomllib.loads((ROOT / "python/uv.lock").read_text(encoding="utf-8"))
    project = tomllib.loads((ROOT / "python/pyproject.toml").read_text(encoding="utf-8"))["project"]
    records: list[dict[str, object]] = []
    ids: dict[tuple[str, str], str] = {}
    packages = lock["package"]
    for package in packages:
        name, version = package["name"], package["version"]
        identity = f"pypi:{name}@{version}"
        ids[(name, version)] = spdx_id(identity)
        if package.get("source", {}).get("editable") == ".":
            records.append(
                package_record(
                    identity=identity,
                    name=name,
                    version=version,
                    ecosystem="pypi",
                    license_declared=project.get("license", {}).get("text", "MIT OR Apache-2.0")
                    if isinstance(project.get("license"), dict)
                    else project.get("license", "MIT OR Apache-2.0"),
                    purpose="APPLICATION",
                )
            )
            continue
        sdist = package.get("sdist", {})
        checksum_value = sdist.get("hash", "").removeprefix("sha256:")
        wheels = package.get("wheels", [])
        if not checksum_value and wheels:
            checksum_value = wheels[0].get("hash", "").removeprefix("sha256:")
        records.append(
            package_record(
                identity=identity,
                name=name,
                version=version,
                ecosystem="pypi",
                license_declared=PYPI_LICENSE_OVERRIDES.get((name, version), "NOASSERTION"),
                download_location=sdist.get("url", "https://pypi.org/project/{}/{}/".format(name, version)),
                checksum=("SHA256", checksum_value) if checksum_value else None,
            )
        )

    project_package = next(package for package in packages if package.get("source", {}).get("editable") == ".")
    project_id = ids[(project_package["name"], project_package["version"])]
    relationships: list[dict[str, str]] = []
    for package in packages:
        if package.get("source", {}).get("editable") == ".":
            continue
        package_id = ids[(package["name"], package["version"])]
        dependencies = {
            ids[(dependency["name"], next(
                item["version"]
                for item in packages
                if item["name"] == dependency["name"]
                and (not dependency.get("version") or item["version"] == dependency["version"])
            ))]
            for dependency in package.get("dependencies", [])
        }
        relationships.extend(dependency_relationships(package_id, dependencies))
    root_dependencies = {
        ids[(dependency["name"], next(
            item["version"]
            for item in packages
            if item["name"] == dependency["name"]
            and (not dependency.get("version") or item["version"] == dependency["version"])
        ))]
        for dependency in project_package.get("dependencies", [])
    }
    for requirement in tomllib.loads((ROOT / "python/pyproject.toml").read_text(encoding="utf-8"))[
        "build-system"
    ]["requires"]:
        match = re.fullmatch(r"([A-Za-z0-9_.-]+)==([A-Za-z0-9_.+-]+)", requirement)
        if not match:
            raise ValueError(f"Python build requirement must be exactly pinned: {requirement}")
        name, version = match.groups()
        identity = f"pypi:{name}@{version}"
        package_id = spdx_id(identity)
        records.append(
            package_record(
                identity=identity,
                name=name,
                version=version,
                ecosystem="pypi",
                download_location=f"https://pypi.org/project/{name}/{version}/",
            )
        )
        root_dependencies.add(package_id)
    relationships.extend(dependency_relationships(project_id, root_dependencies))
    return records, relationships, {project_id}


def npm_graph() -> tuple[list[dict[str, object]], list[dict[str, str]], set[str]]:
    lock = json.loads((ROOT / "typescript/package-lock.json").read_text(encoding="utf-8"))
    packages = lock["packages"]
    package_id_by_path: dict[str, str] = {}
    records: list[dict[str, object]] = []
    root_name = packages[""]["name"]
    root_identity = f"npm:{root_name}@UNVERSIONED"
    root_id = spdx_id(root_identity)
    records.append(
        package_record(
            identity=root_identity,
            name=root_name,
            version="UNVERSIONED",
            ecosystem="npm",
            purpose="APPLICATION",
        )
    )

    for package_path, package in sorted(packages.items()):
        if not package_path:
            continue
        name = package_path.rsplit("node_modules/", 1)[-1]
        version = package["version"]
        identity = f"npm:{package_path}@{version}"
        package_id_by_path[package_path] = spdx_id(identity)
        integrity = package.get("integrity", "")
        checksum = None
        if "-" in integrity:
            algorithm, value = integrity.split("-", 1)
            try:
                checksum = (algorithm.upper(), base64.b64decode(value, validate=True).hex())
            except (ValueError, TypeError):
                checksum = None
        records.append(
            package_record(
                identity=identity,
                name=name,
                version=version,
                ecosystem="npm",
                license_declared=package.get("license") or "NOASSERTION",
                download_location=package.get("resolved", "NOASSERTION"),
                checksum=checksum,
            )
        )

    def resolve(parent_path: str, dependency_name: str) -> str | None:
        prefix = parent_path
        while prefix:
            candidate = f"{prefix}/node_modules/{dependency_name}"
            if candidate in package_id_by_path:
                return package_id_by_path[candidate]
            if "/" not in prefix:
                break
            prefix = prefix.rsplit("/", 1)[0]
        top_level = f"node_modules/{dependency_name}"
        return package_id_by_path.get(top_level)

    relationships: list[dict[str, str]] = []
    root_dependencies = {
        dependency_id
        for dependency_name in packages[""].get("dependencies", {})
        if (dependency_id := resolve("", dependency_name)) is not None
    }
    root_dependencies.update(
        dependency_id
        for dependency_name in packages[""].get("devDependencies", {})
        if (dependency_id := resolve("", dependency_name)) is not None
    )
    relationships.extend(dependency_relationships(root_id, root_dependencies))
    for package_path, package in packages.items():
        package_id = package_id_by_path.get(package_path)
        if not package_id:
            continue
        dependency_names = set(package.get("dependencies", {})) | set(package.get("optionalDependencies", {}))
        dependencies = {
            dependency_id
            for dependency_name in dependency_names
            if (dependency_id := resolve(package_path, dependency_name)) is not None
        }
        relationships.extend(dependency_relationships(package_id, dependencies))
    return records, relationships, {root_id}


def main() -> None:
    records: list[dict[str, object]] = []
    relationships: list[dict[str, str]] = []
    described: set[str] = set()
    for graph in (cargo_graph(), python_graph(), npm_graph()):
        graph_records, graph_relationships, graph_described = graph
        records.extend(graph_records)
        relationships.extend(graph_relationships)
        described.update(graph_described)

    relationships.extend(
        {
            "spdxElementId": "SPDXRef-DOCUMENT",
            "relationshipType": "DESCRIBES",
            "relatedSpdxElement": package_id,
        }
        for package_id in described
    )
    relationships.sort(
        key=lambda item: (
            item["spdxElementId"],
            item["relationshipType"],
            item["relatedSpdxElement"],
        )
    )

    lock_paths = ("Cargo.lock", "python/uv.lock", "typescript/package-lock.json")
    namespace_hash = hashlib.sha256()
    for lock_path in lock_paths:
        namespace_hash.update(lock_path.encode("utf-8"))
        namespace_hash.update(b"\0")
        namespace_hash.update((ROOT / lock_path).read_bytes())
        namespace_hash.update(b"\0")
    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "lqepoch-trading-core",
        "documentNamespace": f"https://spdx.org/spdxdocs/lqepoch-trading-core-{namespace_hash.hexdigest()}",
        "creationInfo": {
            "creators": ["Tool: scripts/generate-sbom.py"],
            "created": created_timestamp(),
        },
        "packages": sorted(records, key=lambda item: (item["name"], item["versionInfo"], item["SPDXID"])),
        "relationships": relationships,
    }
    OUTPUT.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {OUTPUT.relative_to(ROOT)} ({len(records)} packages)")


if __name__ == "__main__":
    main()
