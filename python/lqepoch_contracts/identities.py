"""Shared identity-boundary validators for immutable market manifests."""

from __future__ import annotations

import re
import unicodedata
from collections.abc import Sequence

_DATASET_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,255}\Z")
_OBJECT_NAME = re.compile(r"[A-Za-z0-9_.-]{1,512}\Z")
_LOCAL_TEST_TAIL = re.compile(r"[A-Za-z0-9_.:-]+\Z")


def _bounded_text(value: str, maximum_utf8_bytes: int) -> bool:
    return (
        bool(value)
        and len(value.encode("utf-8")) <= maximum_utf8_bytes
        and value.strip() == value
        and not any(unicodedata.category(character) == "Cc" for character in value)
    )


def valid_source_identity(value: str) -> bool:
    return _bounded_text(value, 128)


def valid_dataset_id(value: str) -> bool:
    if _DATASET_ID.fullmatch(value) is None or ".." in value:
        return False
    return not any(
        token.lower() in {"latest", "fallback"}
        for token in re.split(r"[._:-]", value)
    )


def valid_market_symbol(value: str) -> bool:
    return _bounded_text(value, 256)


def valid_sorted_symbols(values: Sequence[str]) -> bool:
    return bool(values) and all(valid_market_symbol(value) for value in values) and all(
        left < right for left, right in zip(values, values[1:])
    )


def valid_object_name(value: str) -> bool:
    return _OBJECT_NAME.fullmatch(value) is not None and value not in {".", ".."}


def valid_object_id(value: str, *, local_test: bool) -> bool:
    if not _bounded_text(value, 512):
        return False
    if not local_test:
        return not value.startswith("local-test:")
    if not value.startswith("local-test:"):
        return False
    tail = value.removeprefix("local-test:")
    return bool(tail) and ".." not in tail and _LOCAL_TEST_TAIL.fullmatch(tail) is not None
