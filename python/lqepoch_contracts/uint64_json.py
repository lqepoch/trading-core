"""Strict JSON projection helpers for protobuf uint64 fields."""

from __future__ import annotations

import re
from collections.abc import Mapping, Sequence

UINT64_MAX = (1 << 64) - 1
_CANONICAL_UINT64 = re.compile(r"(?:0|[1-9][0-9]*)\Z")
JsonPath = Sequence[str]


def parse_uint64_json(value: object) -> int:
    """Parse only canonical decimal strings in the protobuf uint64 range."""
    if not isinstance(value, str) or _CANONICAL_UINT64.fullmatch(value) is None:
        raise ValueError("uint64 JSON values must be canonical decimal strings")
    parsed = int(value)
    if parsed > UINT64_MAX:
        raise ValueError("uint64 JSON value exceeds the unsigned 64-bit maximum")
    return parsed


def validate_uint64_json_paths(document: object, paths: Sequence[JsonPath]) -> None:
    """Validate required uint64 JSON fields before a ProtoJSON parser sees them."""
    for path in paths:
        current: object = document
        for component in path:
            if not isinstance(current, Mapping) or component not in current:
                raise ValueError(f"missing uint64 JSON field: {'.'.join(path)}")
            current = current[component]
        parse_uint64_json(current)
