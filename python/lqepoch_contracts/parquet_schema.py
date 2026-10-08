"""Cross-language Parquet logical schema canonicalization and fingerprints."""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Mapping

PARQUET_SCHEMA_FINGERPRINT_PREFIX = "LQEpoch-Parquet-Schema-v1\n"
PARQUET_LOGICAL_TYPES = frozenset(
    {
        "bool",
        "date_iso8601",
        "decimal_string",
        "sha256_hex",
        "timestamp_ns_utc",
        "uint32",
        "uint64",
        "utf8",
    }
)
_SCHEMA_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}\Z")
_FIELD_NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]{0,127}\Z")


def canonical_schema_json(descriptor: Mapping[str, object]) -> str:
    if set(descriptor) != {"schema_version", "schema_id", "fields"}:
        raise ValueError("unknown or missing schema descriptor field")
    schema_version = descriptor["schema_version"]
    if type(schema_version) is not int or schema_version != 1:
        raise ValueError("unsupported schema version")
    schema_id = descriptor["schema_id"]
    fields = descriptor["fields"]
    if not isinstance(schema_id, str) or _SCHEMA_ID.fullmatch(schema_id) is None:
        raise ValueError("invalid schema ID")
    if not isinstance(fields, list) or not fields:
        raise ValueError("schema fields must be a non-empty ordered list")

    seen: set[str] = set()
    for field in fields:
        if not isinstance(field, Mapping) or set(field) != {"name", "nullable", "type"}:
            raise ValueError("unknown or missing schema field property")
        name, nullable, logical_type = field["name"], field["nullable"], field["type"]
        if not isinstance(name, str) or _FIELD_NAME.fullmatch(name) is None or name in seen:
            raise ValueError("invalid or duplicate schema field name")
        if (
            not isinstance(nullable, bool)
            or not isinstance(logical_type, str)
            or logical_type not in PARQUET_LOGICAL_TYPES
        ):
            raise ValueError("invalid schema field type or nullability")
        seen.add(name)

    return json.dumps(descriptor, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def fingerprint_schema_sha256(descriptor: Mapping[str, object]) -> str:
    canonical = canonical_schema_json(descriptor)
    preimage = (PARQUET_SCHEMA_FINGERPRINT_PREFIX + canonical).encode("utf-8")
    return hashlib.sha256(preimage).hexdigest()
