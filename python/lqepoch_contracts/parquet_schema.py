"""Cross-language Parquet logical schema canonicalization and fingerprints."""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Mapping
from copy import deepcopy
from datetime import date
from functools import lru_cache
from importlib.resources import files

PARQUET_SCHEMA_FINGERPRINT_PREFIX = "LQEpoch-Parquet-Schema-v1\n"
PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY = "lqepoch.schema_descriptor.v1"
PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY = "lqepoch.schema_fingerprint_sha256"
PARQUET_LOGICAL_TYPES = frozenset(
    {
        "bool",
        "binary",
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


@lru_cache(maxsize=1)
def _trusted_registry() -> dict[str, object]:
    """Load and verify the single generated registry resource packaged with this wheel."""
    resource = files("lqepoch_contracts").joinpath(
        "resources", "parquet-schema-registry.json"
    )
    registry = json.loads(resource.read_text(encoding="utf-8"))
    if registry.get("fingerprint_prefix") != PARQUET_SCHEMA_FINGERPRINT_PREFIX:
        raise RuntimeError("packaged Parquet schema registry has an unsupported fingerprint prefix")
    schemas = registry.get("schemas")
    if not isinstance(schemas, list) or not schemas:
        raise RuntimeError("packaged Parquet schema registry has no trusted schemas")
    seen: set[str] = set()
    for entry in schemas:
        if not isinstance(entry, dict):
            raise RuntimeError("invalid trusted Parquet schema registry entry")
        descriptor = entry.get("descriptor")
        schema_id = descriptor.get("schema_id") if isinstance(descriptor, dict) else None
        if not isinstance(schema_id, str) or schema_id in seen:
            raise RuntimeError("trusted Parquet schema IDs must be unique strings")
        canonical = canonical_schema_json(descriptor)
        if entry.get("canonical_json") != canonical:
            raise RuntimeError(f"trusted Parquet schema {schema_id} has stale canonical bytes")
        if entry.get("sha256") != fingerprint_schema_sha256(descriptor):
            raise RuntimeError(f"trusted Parquet schema {schema_id} has a stale fingerprint")
        seen.add(schema_id)
    return registry


def load_trusted_parquet_schema_registry() -> dict[str, object]:
    """Return a defensive copy of the verified registry embedded in the installed wheel."""
    return deepcopy(_trusted_registry())


def trusted_parquet_schema_descriptor(schema_id: str) -> dict[str, object]:
    """Return the registered descriptor for a schema ID, rejecting unknown identities."""
    for entry in _trusted_registry()["schemas"]:
        if entry["descriptor"]["schema_id"] == schema_id:
            return deepcopy(entry["descriptor"])
    raise ValueError(f"unregistered Parquet schema ID: {schema_id}")


def trusted_parquet_schema_sha256(schema_id: str) -> str:
    """Return the golden logical-shape fingerprint for a registered schema ID."""
    for entry in _trusted_registry()["schemas"]:
        if entry["descriptor"]["schema_id"] == schema_id:
            return entry["sha256"]
    raise ValueError(f"unregistered Parquet schema ID: {schema_id}")


def trusted_parquet_schema_metadata(schema_id: str) -> dict[bytes, bytes]:
    """Return the exact optional Arrow/Parquet metadata pair derived from the registry."""
    descriptor = trusted_parquet_schema_descriptor(schema_id)
    canonical = canonical_schema_json(descriptor)
    fingerprint = fingerprint_schema_sha256(descriptor)
    return {
        PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY.encode("ascii"): canonical.encode("utf-8"),
        PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.encode("ascii"): fingerprint.encode("ascii"),
    }


def validate_optional_parquet_schema_metadata(
    schema_id: str, metadata: Mapping[bytes, bytes] | None
) -> None:
    """Allow absent registry metadata, but require both exact values if either key appears."""
    trusted = trusted_parquet_schema_metadata(schema_id)
    if metadata is None:
        return
    if not isinstance(metadata, Mapping) or any(
        not isinstance(key, bytes) or not isinstance(value, bytes)
        for key, value in metadata.items()
    ):
        raise ValueError("Arrow schema metadata must be a byte-to-byte mapping")
    descriptor_key = PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY.encode("ascii")
    fingerprint_key = PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.encode("ascii")
    descriptor = metadata.get(descriptor_key)
    fingerprint = metadata.get(fingerprint_key)
    if descriptor is None and fingerprint is None:
        return
    if descriptor is None or fingerprint is None:
        raise ValueError("schema metadata must include both registered descriptor keys")
    if descriptor != trusted[descriptor_key] or fingerprint != trusted[fingerprint_key]:
        raise ValueError("schema metadata differs from the registered descriptor or fingerprint")


def verify_pyarrow_schema(schema: object, schema_id: str) -> None:
    """Require actual Arrow fields to match the trusted ordered physical schema exactly.

    Field metadata is ignored because it is not part of the fingerprint. Names, order, physical
    types, timestamp unit/timezone, and nullability are checked against the core registry.
    """
    try:
        import pyarrow as pa
    except ImportError as error:
        raise RuntimeError("verify_pyarrow_schema requires the pinned `arrow` optional extra") from error

    descriptor = trusted_parquet_schema_descriptor(schema_id)
    validate_optional_parquet_schema_metadata(schema_id, getattr(schema, "metadata", None))
    fields = descriptor["fields"]
    if len(schema) != len(fields):
        raise ValueError("Arrow schema field count does not match the trusted Parquet schema")
    for index, expected in enumerate(fields):
        actual = schema.field(index)
        if actual.name != expected["name"] or actual.nullable is not expected["nullable"]:
            raise ValueError(f"Arrow field {index} name or nullability does not match the registry")
        logical_type = expected["type"]
        arrow_type = actual.type
        expected_type = _pyarrow_type(pa, logical_type)
        if arrow_type != expected_type:
            raise ValueError(
                f"Arrow field {actual.name} has physical type {arrow_type}, expected {logical_type}"
            )


def pyarrow_schema_for_trusted_schema(schema_id: str) -> object:
    """Build the ordered physical Arrow schema for a registered logical schema ID."""
    try:
        import pyarrow as pa
    except ImportError as error:
        raise RuntimeError("pyarrow_schema_for_trusted_schema requires the pinned `arrow` extra") from error

    descriptor = trusted_parquet_schema_descriptor(schema_id)
    return pa.schema(
        [
            pa.field(
                field["name"],
                _pyarrow_type(pa, field["type"]),
                nullable=field["nullable"],
            )
            for field in descriptor["fields"]
        ]
    )


def _pyarrow_type(pa: object, logical_type: str) -> object:
    type_builders = {
        "binary": pa.binary,
        "bool": pa.bool_,
        "date_iso8601": pa.string,
        "decimal_string": pa.string,
        "sha256_hex": pa.string,
        "timestamp_ns_utc": lambda: pa.timestamp("ns", tz="UTC"),
        "uint32": pa.uint32,
        "uint64": pa.uint64,
        "utf8": pa.string,
    }
    try:
        return type_builders[logical_type]()
    except KeyError as error:
        raise ValueError(f"unsupported registered logical type: {logical_type}") from error


def validate_date_iso8601(value: object) -> str:
    """Require the canonical Gregorian `YYYY-MM-DD` UTF-8 row value."""
    if not isinstance(value, str) or len(value) != 10:
        raise ValueError("date_iso8601 values must be canonical YYYY-MM-DD strings")
    try:
        parsed = date.fromisoformat(value)
    except ValueError as error:
        raise ValueError("date_iso8601 value is not a valid Gregorian date") from error
    if parsed.isoformat() != value:
        raise ValueError("date_iso8601 values must be canonical YYYY-MM-DD strings")
    return value


def validate_raw_frame_bytes(value: object) -> bytes:
    """Require an exact byte payload within the one-MiB decoded-row bound."""
    if not isinstance(value, bytes):
        raise ValueError("raw frame payload must be bytes")
    if len(value) > 1024 * 1024:
        raise ValueError("raw frame payload exceeds the 1 MiB bound")
    return value
