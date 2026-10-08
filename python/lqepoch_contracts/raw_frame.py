"""Bounded validators for capture-scoped raw and normalized Parquet rows.

These helpers validate row identity and raw/event correlation. They do not prove provider
entitlement, input durability, historical completeness, or research qualification.
For normalized event admission, use the Rust validator as the authority for complete domain
semantics, including OCC candidates and event-specific numeric rules.
"""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Mapping, Sequence
from datetime import date
from decimal import Decimal, InvalidOperation

from .identities import valid_market_symbol, valid_source_identity
from .parquet_schema import trusted_parquet_schema_descriptor
from .uint64_json import UINT64_MAX, parse_uint64_json

MARKET_RAW_FRAME_V2_SCHEMA_ID = "lqepoch.market_raw_frame.v2"
MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID = "lqepoch.market_raw_json_frame.v2"
MARKET_EVENT_V3_SCHEMA_ID = "lqepoch.market_event.v3"
MAX_RAW_FRAME_BYTES = 1024 * 1024
MAX_RAW_FRAME_EVENT_COUNT = 512
MAX_RAW_CAPTURE_CHUNK_FRAMES = 1024
MAX_RAW_CAPTURE_CHUNK_BYTES = 16 * 1024 * 1024
MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES = 16 * 1024 * 1024
MAX_RAW_FRAME_SYMBOLS_JSON_BYTES = MAX_RAW_FRAME_EVENT_COUNT * (2 * 256 + 3) + 1

_CAPTURE_INSTANCE_ID = re.compile(r"[0-9a-f]{12}4[0-9a-f]{3}[89ab][0-9a-f]{15}\Z")
_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
_RFC3339_UTC = re.compile(
    r"([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})"
    r"(?:\.([0-9]{1,9}))?(?:Z|([+-])([0-9]{2}):([0-9]{2}))\Z"
)
_DECIMAL = re.compile(r"-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?\Z")
_NUMERIC_ENCODINGS = frozenset(
    {
        "decimal_token",
        "integer_token",
        "binary_float64_shortest_decimal",
        "binary_float32_shortest_decimal",
    }
)
_ENTITLEMENTS = frozenset({"unknown", "authorized", "unauthorized"})
_DISPOSITIONS = frozenset(
    {"market_data", "control", "unknown_message", "malformed_message", "provider_error"}
)
_EVENT_KINDS = frozenset({"stock_quote", "stock_trade", "option_quote", "option_trade"})
_REFERENCE_FIELDS = (
    "raw_frame_capture_instance_id",
    "raw_frame_source_generation",
    "raw_frame_generation",
    "raw_frame_sequence",
    "raw_frame_event_ordinal",
    "raw_frame_event_count",
)


def validate_capture_instance_id_v2(value: object) -> str:
    """Require the spool's 32-digit lowercase UUIDv4/RFC-variant representation."""
    if not isinstance(value, str) or _CAPTURE_INSTANCE_ID.fullmatch(value) is None:
        raise ValueError("capture instance ID must be a lowercase UUIDv4 with RFC variant")
    return value


def validate_raw_frame_row_v2(row: Mapping[str, object], schema_id: str) -> None:
    """Validate one exact MessagePack or JSON raw-frame Parquet row."""
    _validate_raw_frame_row_v2_with_symbols(row, schema_id)


def _validate_raw_frame_row_v2_with_symbols(
    row: Mapping[str, object], schema_id: str
) -> list[str]:
    if schema_id not in {MARKET_RAW_FRAME_V2_SCHEMA_ID, MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID}:
        raise ValueError("raw-frame v2 validator requires a trusted raw-frame v2 schema ID")
    _require_exact_row_fields(row, schema_id)
    if type(row["schema_version"]) is not int or row["schema_version"] != 2:
        raise ValueError("raw-frame v2 row schema_version must equal 2")
    _validate_source(row["provider"], row["feed"], row["entitlement"])
    projection_encoding = row["source_numeric_encoding"]
    if projection_encoding is not None and (
        not isinstance(projection_encoding, str)
        or projection_encoding not in _NUMERIC_ENCODINGS
    ):
        raise ValueError("raw-frame projection encoding must be a normalized numeric encoding")
    validate_capture_instance_id_v2(row["capture_instance_id"])
    _positive_uint64(row["source_generation"], "source_generation")
    _positive_uint64(row["source_frame_sequence"], "source_frame_sequence")
    _positive_uint64(row["canonical_generation"], "canonical_generation")
    _timestamp_nanoseconds(row["received_timestamp_utc"])
    frame_sha256 = row["frame_sha256"]
    frame_bytes = row["frame_bytes"]
    if not isinstance(frame_sha256, str) or _SHA256.fullmatch(frame_sha256) is None:
        raise ValueError("raw-frame SHA-256 must be lowercase hexadecimal")
    if not isinstance(frame_bytes, bytes) or len(frame_bytes) > MAX_RAW_FRAME_BYTES:
        raise ValueError("raw-frame payload must be bytes within the 1 MiB bound")
    if hashlib.sha256(frame_bytes).hexdigest() != frame_sha256:
        raise ValueError("raw-frame SHA-256 does not match the exact bytes")
    event_count = row["event_count"]
    if type(event_count) is not int or not 0 <= event_count <= MAX_RAW_FRAME_EVENT_COUNT:
        raise ValueError("raw-frame event count is outside the per-frame bound")
    disposition = row["disposition"]
    if not isinstance(disposition, str) or disposition not in _DISPOSITIONS:
        raise ValueError("raw-frame disposition is unknown")
    symbols = _canonical_symbols(row["symbols_json"])
    if disposition == "market_data" and (event_count == 0 or not symbols):
        raise ValueError("market-data raw frames require expected events and symbols")
    if disposition == "control" and (event_count != 0 or symbols):
        raise ValueError("control raw frames cannot claim normalized market events")
    if event_count > 0 and (not symbols or len(symbols) > event_count):
        raise ValueError("expected events require a bounded non-empty symbol set")
    return symbols


def validate_market_event_row_v3(row: Mapping[str, object]) -> None:
    """Validate V3 Parquet row shape and its optional all-or-none capture reference.

    This helper checks bounded row structure and correlation invariants. Rust's market event
    validator remains the authority for complete source, instrument, price, and size semantics.
    """
    _require_exact_row_fields(row, MARKET_EVENT_V3_SCHEMA_ID)
    if type(row["schema_version"]) is not int or row["schema_version"] != 1:
        raise ValueError("event v3 storage rows retain wire schema_version 1")
    _validate_source(row["provider"], row["feed"], row["entitlement"])
    encoding = row["numeric_encoding"]
    if not isinstance(encoding, str) or encoding not in _NUMERIC_ENCODINGS:
        raise ValueError("event row requires a normalized numeric encoding")
    source_record_id = row["source_record_id"]
    if source_record_id is not None and not valid_source_identity(source_record_id):
        raise ValueError("invalid source record identity")
    raw_sha = row["raw_frame_sha256"]
    if raw_sha is not None and (not isinstance(raw_sha, str) or _SHA256.fullmatch(raw_sha) is None):
        raise ValueError("invalid raw-frame SHA-256 reference")
    if (
        encoding.startswith("binary_float") and raw_sha is None
        or (row["provider"] == "synthetic" or row["feed"] == "synthetic")
        and encoding != "decimal_token"
    ):
        raise ValueError("event source encoding requires a binary-float digest and decimal synthetic values")
    generation = _positive_uint64(row["generation"], "generation")
    _positive_uint64(row["sequence"], "sequence")
    if row["source_timestamp"] is not None:
        _timestamp_nanoseconds(row["source_timestamp"])
    _timestamp_nanoseconds(row["received_timestamp"])
    event_kind = row["event_kind"]
    symbol = row["symbol"]
    if not isinstance(event_kind, str) or event_kind not in _EVENT_KINDS:
        raise ValueError("unknown normalized event kind")
    if not isinstance(symbol, str) or not valid_market_symbol(symbol):
        raise ValueError("invalid normalized event symbol")
    _validate_event_values(row, event_kind)

    present = [row.get(name) is not None for name in _REFERENCE_FIELDS]
    if any(present) and not all(present):
        raise ValueError("event v3 capture reference columns must be all present or all absent")
    if all(present):
        validate_capture_instance_id_v2(row["raw_frame_capture_instance_id"])
        source_generation = _positive_uint64(
            row["raw_frame_source_generation"], "raw_frame_source_generation"
        )
        canonical_generation = _positive_uint64(
            row["raw_frame_generation"], "raw_frame_generation"
        )
        _positive_uint64(row["raw_frame_sequence"], "raw_frame_sequence")
        ordinal = row["raw_frame_event_ordinal"]
        count = row["raw_frame_event_count"]
        if (
            type(ordinal) is not int
            or type(count) is not int
            or not 1 <= ordinal <= count <= MAX_RAW_FRAME_EVENT_COUNT
            or canonical_generation != generation
            or raw_sha is None
        ):
            raise ValueError("invalid event v3 capture reference")
        del source_generation


def validate_event_against_raw_frame_row_v2(
    event: Mapping[str, object], raw_frame: Mapping[str, object], schema_id: str
) -> None:
    """Cross-check one event's complete pre-decode key against a validated raw row."""
    symbols = _validate_raw_frame_row_v2_with_symbols(raw_frame, schema_id)
    validate_market_event_row_v3(event)
    _validate_event_frame_pair(event, raw_frame, symbols)


def validate_raw_event_chunk_v2(
    raw_frames: Sequence[Mapping[str, object]],
    events: Sequence[Mapping[str, object]],
    schema_id: str,
) -> None:
    """Require one bounded UUID/source-generation chunk with contiguous, complete projections."""
    if not isinstance(raw_frames, (list, tuple)) or not raw_frames:
        raise ValueError("capture chunk must contain at least one raw frame")
    if len(raw_frames) > MAX_RAW_CAPTURE_CHUNK_FRAMES:
        raise ValueError("capture chunk exceeds the 1024-frame bound")
    if not isinstance(events, (list, tuple)):
        raise ValueError("capture events must be a bounded sequence")
    if len(events) > MAX_RAW_CAPTURE_CHUNK_FRAMES * MAX_RAW_FRAME_EVENT_COUNT:
        raise ValueError("capture chunk expected event count exceeds its structural bound")
    capture_id: str | None = None
    source_generation: int | None = None
    expected_sequence: int | None = None
    payload_bytes = 0
    metadata_bytes = 0
    expected_event_count = 0
    frame_by_sequence: dict[int, int] = {}
    grouped_events: list[list[Mapping[str, object]]] = [[] for _ in raw_frames]
    symbols_by_frame: list[list[str]] = []

    for index, frame in enumerate(raw_frames):
        symbols = _validate_raw_frame_row_v2_with_symbols(frame, schema_id)
        current_capture_id = validate_capture_instance_id_v2(frame["capture_instance_id"])
        current_source_generation = _positive_uint64(frame["source_generation"], "source_generation")
        sequence = _positive_uint64(frame["source_frame_sequence"], "source_frame_sequence")
        if capture_id is None:
            capture_id = current_capture_id
            source_generation = current_source_generation
        elif current_capture_id != capture_id or current_source_generation != source_generation:
            raise ValueError("capture chunk cannot mix capture UUIDs or source generations")
        if expected_sequence is not None and sequence != expected_sequence:
            raise ValueError("capture chunk frame sequences must be contiguous")
        if sequence in frame_by_sequence:
            raise ValueError("capture chunk contains a duplicate source frame sequence")
        frame_by_sequence[sequence] = index
        expected_sequence = sequence + 1
        payload_bytes += len(frame["frame_bytes"])
        metadata_bytes += len(str(frame["symbols_json"]).encode("utf-8"))
        if payload_bytes > MAX_RAW_CAPTURE_CHUNK_BYTES:
            raise ValueError("capture chunk exceeds the 16 MiB payload bound")
        if metadata_bytes > MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES:
            raise ValueError("capture chunk exceeds the 16 MiB symbols metadata bound")
        expected_event_count += frame["event_count"]
        symbols_by_frame.append(symbols)

    if len(events) != expected_event_count:
        raise ValueError("capture chunk does not contain every expected normalized event")
    for event in events:
        validate_market_event_row_v3(event)
        reference = event["raw_frame_sequence"]
        sequence = _positive_uint64(reference, "raw_frame_sequence")
        if (
            event["raw_frame_capture_instance_id"] != capture_id
            or _positive_uint64(event["raw_frame_source_generation"], "raw_frame_source_generation")
            != source_generation
            or sequence not in frame_by_sequence
        ):
            raise ValueError("event capture key does not identify a frame in this chunk")
        grouped_events[frame_by_sequence[sequence]].append(event)

    for frame, frame_events, symbols in zip(raw_frames, grouped_events, symbols_by_frame, strict=True):
        if len(frame_events) != frame["event_count"]:
            raise ValueError("capture chunk is missing a frame projection")
        ordinals: set[int] = set()
        for event in frame_events:
            _validate_event_frame_pair(event, frame, symbols)
            ordinal = event["raw_frame_event_ordinal"]
            if ordinal in ordinals:
                raise ValueError("capture chunk contains duplicate event ordinals")
            ordinals.add(ordinal)
        if ordinals != set(range(1, frame["event_count"] + 1)):
            raise ValueError("capture chunk event ordinals are incomplete")


def _validate_event_frame_pair(
    event: Mapping[str, object], raw_frame: Mapping[str, object], symbols: Sequence[str]
) -> None:
    if (
        event["raw_frame_capture_instance_id"] != raw_frame["capture_instance_id"]
        or _positive_uint64(event["raw_frame_source_generation"], "raw_frame_source_generation")
        != _positive_uint64(raw_frame["source_generation"], "source_generation")
        or _positive_uint64(event["raw_frame_generation"], "raw_frame_generation")
        != _positive_uint64(raw_frame["canonical_generation"], "canonical_generation")
        or _positive_uint64(event["generation"], "generation")
        != _positive_uint64(raw_frame["canonical_generation"], "canonical_generation")
        or _positive_uint64(event["raw_frame_sequence"], "raw_frame_sequence")
        != _positive_uint64(raw_frame["source_frame_sequence"], "source_frame_sequence")
        or event["raw_frame_event_count"] != raw_frame["event_count"]
        or event["raw_frame_sha256"] != raw_frame["frame_sha256"]
        or event["provider"] != raw_frame["provider"]
        or event["feed"] != raw_frame["feed"]
        or event["entitlement"] != raw_frame["entitlement"]
        or _timestamp_nanoseconds(event["received_timestamp"])
        != _timestamp_nanoseconds(raw_frame["received_timestamp_utc"])
        or (
            raw_frame["source_numeric_encoding"] is not None
            and event["numeric_encoding"] != raw_frame["source_numeric_encoding"]
        )
        or event["symbol"] not in symbols
    ):
        raise ValueError("event row does not match the exact raw-frame capture key")


def _validate_source(provider: object, feed: object, entitlement: object) -> None:
    if (
        not isinstance(provider, str)
        or not valid_source_identity(provider)
        or not isinstance(feed, str)
        or not valid_source_identity(feed)
        or not isinstance(entitlement, str)
        or entitlement not in _ENTITLEMENTS
    ):
        raise ValueError("invalid source identity or entitlement spelling")
    mentions_synthetic = provider.lower() == "synthetic" or feed.lower() == "synthetic"
    if mentions_synthetic and (provider != "synthetic" or feed != "synthetic"):
        raise ValueError("synthetic provider and feed identities must be paired")


def _require_exact_row_fields(row: Mapping[str, object], schema_id: str) -> None:
    if not isinstance(row, Mapping):
        raise ValueError("Parquet row must be a mapping")
    descriptor = trusted_parquet_schema_descriptor(schema_id)
    expected = {field["name"] for field in descriptor["fields"]}
    if set(row) != expected:
        raise ValueError("Parquet row fields differ from the trusted schema descriptor")


def _positive_uint64(value: object, field_name: str) -> int:
    if type(value) is int:
        parsed = value
    elif isinstance(value, str):
        parsed = parse_uint64_json(value)
    else:
        raise ValueError(f"{field_name} must be an exact uint64 value")
    if not 0 < parsed <= UINT64_MAX:
        raise ValueError(f"{field_name} must be positive")
    return parsed


def _canonical_symbols(value: object) -> list[str]:
    if not isinstance(value, str):
        raise ValueError("symbols_json exceeds its bounded UTF-8 size")
    # UTF-8 uses at least one byte for every Python code point, so this cheap
    # lower bound rejects oversized inputs before allocating an encoded copy.
    if len(value) > MAX_RAW_FRAME_SYMBOLS_JSON_BYTES:
        raise ValueError("symbols_json exceeds its bounded UTF-8 size")
    try:
        encoded_size = len(value.encode("utf-8"))
    except UnicodeEncodeError as error:
        raise ValueError("symbols_json must contain valid Unicode scalar text") from error
    if encoded_size > MAX_RAW_FRAME_SYMBOLS_JSON_BYTES:
        raise ValueError("symbols_json exceeds its bounded UTF-8 size")
    try:
        symbols = json.loads(value)
    except json.JSONDecodeError as error:
        raise ValueError("symbols_json is malformed") from error
    if (
        not isinstance(symbols, list)
        or len(symbols) > MAX_RAW_FRAME_EVENT_COUNT
        or any(not isinstance(symbol, str) or not valid_market_symbol(symbol) for symbol in symbols)
        or any(left >= right for left, right in zip(symbols, symbols[1:]))
        or json.dumps(symbols, ensure_ascii=False, separators=(",", ":")) != value
    ):
        raise ValueError("symbols_json must be a canonical sorted unique symbol array")
    return symbols


def _timestamp_nanoseconds(value: object) -> int:
    if type(value) is int:
        if -(1 << 63) <= value <= (1 << 63) - 1:
            return value
        raise ValueError("timestamp nanoseconds exceed signed Arrow range")
    if not isinstance(value, str):
        raise ValueError("timestamps must be RFC3339 UTC text or signed epoch nanoseconds")
    match = _RFC3339_UTC.fullmatch(value)
    if match is None:
        raise ValueError("timestamp is not exact RFC3339 UTC")
    year, month, day, hour, minute, second, fraction, sign, offset_hour, offset_minute = (
        match.groups()
    )
    if sign is not None and (offset_hour != "00" or offset_minute != "00"):
        raise ValueError("market row timestamps must have a UTC offset")
    if int(hour) > 23 or int(minute) > 59 or int(second) > 59:
        raise ValueError("timestamp contains an invalid clock time or unsupported leap second")
    try:
        current = date(int(year), int(month), int(day))
    except ValueError as error:
        raise ValueError("timestamp contains an invalid Gregorian date") from error
    days = (current - date(1970, 1, 1)).days
    seconds = days * 86_400 + int(hour) * 3_600 + int(minute) * 60 + int(second)
    nanos = int((fraction or "").ljust(9, "0"))
    result = seconds * 1_000_000_000 + nanos
    if not -(1 << 63) <= result <= (1 << 63) - 1:
        raise ValueError("timestamp is outside signed Arrow nanosecond range")
    return result


def _validate_event_values(row: Mapping[str, object], event_kind: str) -> None:
    numeric_fields = ("price", "size", "bid", "ask", "bid_size", "ask_size")
    for name in numeric_fields:
        value = row[name]
        if value is None:
            continue
        if not isinstance(value, str) or len(value) > 128 or _DECIMAL.fullmatch(value) is None:
            raise ValueError(f"event row {name} must be a bounded exact decimal string")
        try:
            parsed = Decimal(value)
        except InvalidOperation as error:
            raise ValueError(f"event row {name} is not a finite decimal") from error
        if not parsed.is_finite():
            raise ValueError(f"event row {name} is not finite")
    if event_kind in {"stock_trade", "option_trade"}:
        if row["price"] is None or row["size"] is None:
            raise ValueError("trade row requires price and size")
        if Decimal(row["price"]) <= 0 or Decimal(row["size"]) <= 0:
            raise ValueError("trade row price and size must be positive")
        if any(row[name] is not None for name in ("bid", "ask", "bid_size", "ask_size")):
            raise ValueError("trade row cannot contain quote fields")
    else:
        if row["price"] is not None or row["size"] is not None:
            raise ValueError("quote row cannot contain trade fields")
        if all(row[name] is None for name in ("bid", "ask", "bid_size", "ask_size")):
            raise ValueError("quote row must contain at least one quote field")
        for name in ("bid", "ask", "bid_size", "ask_size"):
            if row[name] is not None and Decimal(row[name]) < 0:
                raise ValueError("quote fields must be non-negative")
