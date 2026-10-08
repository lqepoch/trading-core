"""Lossless validated ProtoJSON entry points for cross-language consumers."""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Mapping
from decimal import Decimal

from google.protobuf import json_format

from lqepoch.dataset.v1 import manifest_pb2
from lqepoch.dataset.v2 import manifest_pb2 as manifest_v2_pb2
from lqepoch.market.v1 import market_pb2
from lqepoch.market.v2 import trade_bar_pb2
from lqepoch.prediction.v1 import prediction_pb2

from .parquet_schema import trusted_parquet_schema_sha256
from .parquet_schema import validate_date_iso8601
from .identities import valid_dataset_id, valid_market_symbol, valid_object_id, valid_object_name, valid_source_identity, valid_sorted_symbols
from .uint64_json import validate_uint64_json_paths

MAX_DATASET_MANIFEST_V2_JSON_BYTES = 2 * 1024 * 1024
MAX_DATASET_MANIFEST_V2_SYMBOLS = 4096
MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS = 60_000_000_000
MAX_FINITE_BATCH_SEAL_RECEIPT_V2_JSON_BYTES = 16 * 1024
MAX_US_EQUITY_TRADE_BAR_V2_JSON_BYTES = 64 * 1024
_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
_EXACT_DECIMAL = re.compile(
    r"(-?)(0|[1-9][0-9]*)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?\Z"
)
_PROTO_TIMESTAMP_V2 = re.compile(
    r"([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})"
    r"(?:\.([0-9]{1,9}))?(?:Z|([+-])([0-9]{2}):([0-9]{2}))\Z"
)
_PROTO_TIMESTAMP_V2_FIELDS = frozenset(
    {
        "startInclusive",
        "start_inclusive",
        "endExclusive",
        "end_exclusive",
        "dataCutoffExclusive",
        "data_cutoff_exclusive",
        "sealedAt",
        "sealed_at",
        "completedAt",
        "completed_at",
        "completeUpToExclusive",
        "complete_up_to_exclusive",
        "localPolicyCutoff",
        "local_policy_cutoff",
        "observedMaxSourceTimestamp",
        "observed_max_source_timestamp",
        "barStartUtc",
        "bar_start_utc",
        "barEndExclusiveUtc",
        "bar_end_exclusive_utc",
        "availableAtUtc",
        "available_at_utc",
        "sessionStartUtc",
        "session_start_utc",
        "sessionEndExclusiveUtc",
        "session_end_exclusive_utc",
        "windowStartUtc",
        "window_start_utc",
        "windowEndExclusiveUtc",
        "window_end_exclusive_utc",
        "sourceStartUtc",
        "source_start_utc",
        "sourceEndExclusiveUtc",
        "source_end_exclusive_utc",
    }
)
_NUMERIC_ENCODING_V2_NAMES = frozenset(
    {
        "NUMERIC_ENCODING_DECIMAL_TOKEN",
        "NUMERIC_ENCODING_INTEGER_TOKEN",
        "NUMERIC_ENCODING_BINARY_FLOAT64_SHORTEST_DECIMAL",
        "NUMERIC_ENCODING_BINARY_FLOAT32_SHORTEST_DECIMAL",
        "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES",
        "NUMERIC_ENCODING_RAW_JSON_BYTES",
    }
)
_FINITE_SOURCE_KIND_V2_NAMES = frozenset(
    {
        "FINITE_BATCH_SOURCE_KIND_SYNTHETIC_REPLAY",
        "FINITE_BATCH_SOURCE_KIND_HISTORICAL_PAGED",
        "FINITE_BATCH_SOURCE_KIND_HISTORICAL_NON_PAGED",
        "FINITE_BATCH_SOURCE_KIND_LOCAL_ARCHIVE",
    }
)

RAW_ENCODING_SCHEMA_IDS = {
    "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES": "lqepoch.market_raw_frame.v1",
    "NUMERIC_ENCODING_RAW_JSON_BYTES": "lqepoch.market_raw_json_frame.v1",
    5: "lqepoch.market_raw_frame.v1",
    6: "lqepoch.market_raw_json_frame.v1",
}


def parse_market_event_protojson(document: Mapping[str, object]) -> market_pb2.MarketEventEnvelopeV1:
    validate_uint64_json_paths(document, (("generation",), ("sequence",)))
    _reject_duplicate_numeric_encoding_aliases(document)
    message = json_format.ParseDict(document, market_pb2.MarketEventEnvelopeV1())
    if _raw_schema_id_for_value(message.source.numeric_encoding) is not None:
        raise ValueError("raw byte-frame encodings are not normalized market-event encodings")
    return message


def parse_dataset_manifest_protojson(document: Mapping[str, object]) -> manifest_pb2.DatasetManifestV1:
    validate_uint64_json_paths(
        document,
        (
            ("sourceTimestampMissingRows",),
            ("rowCount",),
            ("object", "sizeBytes"),
            ("object", "parquetFooterRows"),
        ),
    )
    _reject_duplicate_numeric_encoding_aliases(document)
    message = json_format.ParseDict(document, manifest_pb2.DatasetManifestV1())
    raw_schema_id = _raw_schema_id_for_value(message.source.numeric_encoding)
    schema_sha256 = message.object.parquet_schema_sha256
    raw_schema_hashes = {
        trusted_parquet_schema_sha256(schema_id)
        for schema_id in RAW_ENCODING_SCHEMA_IDS.values()
    }
    if raw_schema_id is None:
        if schema_sha256 in raw_schema_hashes:
            raise ValueError("raw byte-frame schema fingerprint requires its matching raw encoding")
    elif schema_sha256 != trusted_parquet_schema_sha256(raw_schema_id):
        raise ValueError(
            f"{raw_schema_id} encoding must be bound to its registered Parquet schema"
        )
    if raw_schema_id is not None:
        if (
            message.HasField("time_range")
            or message.row_count == 0
            or message.row_count != message.source_timestamp_missing_rows
        ):
            raise ValueError(
                "raw-frame manifests require rows, no source-time range, and every frame timestamp missing"
            )
    return message


def parse_dataset_manifest_v2_protojson(
    document: Mapping[str, object],
) -> manifest_v2_pb2.DatasetManifestV2:
    """Parse and validate the bounded DatasetManifestV2 ProtoJSON projection.

    This validates structure and cross-field bindings. It does not trust receipt hashes or mint
    provider-completeness authority; a fixed composition-root verifier must inspect receipt bytes.
    """
    if not isinstance(document, Mapping):
        raise ValueError("dataset manifest v2 JSON must be an object")
    try:
        encoded_size = len(json.dumps(document, separators=(",", ":")).encode("utf-8"))
    except (TypeError, ValueError) as error:
        raise ValueError("dataset manifest v2 JSON must contain only JSON values") from error
    if encoded_size > MAX_DATASET_MANIFEST_V2_JSON_BYTES:
        raise ValueError("dataset manifest v2 JSON exceeds the configured byte limit")

    _reject_duplicate_proto_field_spellings(document)
    _validate_proto_timestamp_v2_fields(document)
    _validate_dataset_manifest_v2_enum_names(document)
    completion = _mapping_field(document, "completionEvidence", "completion_evidence")
    if completion is None:
        raise ValueError("dataset manifest v2 requires completion evidence")
    evidence_case = _exactly_one_mapping_case(
        completion, ("finiteBatch", "providerWatermark", "diagnosticStream")
    )
    if evidence_case is None:
        raise ValueError("completion evidence must contain exactly one oneof case")

    uint64_paths: list[tuple[str, ...]] = [
        ("sourceTimestampMissingRows",),
        ("rowCount",),
        ("object", "sizeBytes"),
        ("object", "parquetFooterRows"),
    ]
    if evidence_case == "finiteBatch":
        uint64_paths.extend(
            (
                ("completionEvidence", "finiteBatch", "inputSizeBytes"),
                ("completionEvidence", "finiteBatch", "inputRecordCount"),
                ("completionEvidence", "finiteBatch", "consumedRecordCount"),
            )
        )
        finite = _mapping_field(completion, "finiteBatch", "finite_batch")
        if finite is not None and _has_field(finite, "pageCount", "page_count"):
            uint64_paths.append(("completionEvidence", "finiteBatch", "pageCount"))
    elif evidence_case == "providerWatermark":
        uint64_paths.extend(
            ("completionEvidence", "providerWatermark", field)
            for field in (
                "generation",
                "firstSequence",
                "lastSequence",
                "sequenceCount",
                "allowedLatenessNs",
            )
        )
    else:
        uint64_paths.extend(
            (
                ("completionEvidence", "diagnosticStream", "generation"),
                ("completionEvidence", "diagnosticStream", "observedLastSequence"),
            )
        )
    validate_uint64_json_paths(document, uint64_paths)

    try:
        message = json_format.ParseDict(document, manifest_v2_pb2.DatasetManifestV2())
    except json_format.ParseError as error:
        raise ValueError("invalid dataset manifest v2 ProtoJSON") from error
    _validate_dataset_manifest_v2(message)
    return message


def parse_dataset_manifest_v2_json(
    document: bytes | str,
) -> manifest_v2_pb2.DatasetManifestV2:
    """Parse raw JSON bytes while rejecting duplicate keys before ProtoJSON decoding."""
    raw = document.encode("utf-8") if isinstance(document, str) else document
    if not isinstance(raw, bytes):
        raise ValueError("dataset manifest v2 JSON must be bytes or text")
    if len(raw) > MAX_DATASET_MANIFEST_V2_JSON_BYTES:
        raise ValueError("dataset manifest v2 JSON exceeds the configured byte limit")
    try:
        value = json.loads(raw, object_pairs_hook=_unique_object_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("dataset manifest v2 JSON is malformed") from error
    if not isinstance(value, Mapping):
        raise ValueError("dataset manifest v2 JSON must be an object")
    return parse_dataset_manifest_v2_protojson(value)


def finite_batch_seal_receipt_protojson_bytes(
    value: manifest_v2_pb2.FiniteBatchCompletionV2,
) -> bytes:
    """Return core's compact, non-circular ProtoJSON projection with no trailing newline.

    The projection omits ``seal_receipt_sha256``. Matching its SHA only binds these exact fields;
    it does not authenticate an issuer or establish provider-history completeness.
    """
    if not isinstance(value, manifest_v2_pb2.FiniteBatchCompletionV2):
        raise ValueError("finite batch receipt requires the generated FiniteBatchCompletionV2 message")
    _validate_finite_batch_receipt_fields(value)
    projection = manifest_v2_pb2.FiniteBatchSealReceiptV2(
        source_kind=value.source_kind,
        input_identity=value.input_identity,
        input_sha256=value.input_sha256,
        input_size_bytes=value.input_size_bytes,
        input_record_count=value.input_record_count,
        consumed_record_count=value.consumed_record_count,
        reviewed_policy_sha256=value.reviewed_policy_sha256,
    )
    projection.data_cutoff_exclusive.CopyFrom(value.data_cutoff_exclusive)
    projection.sealed_at.CopyFrom(value.sealed_at)
    projection.completed_at.CopyFrom(value.completed_at)
    for field in ("page_count", "pages_exhausted", "page_set_sha256"):
        if value.HasField(field):
            setattr(projection, field, getattr(value, field))

    document = json_format.MessageToDict(projection, preserving_proto_field_name=False)
    encoded = json.dumps(
        document, ensure_ascii=False, separators=(",", ":"), allow_nan=False
    ).encode("utf-8")
    if len(encoded) > MAX_FINITE_BATCH_SEAL_RECEIPT_V2_JSON_BYTES:
        raise ValueError("finite batch seal receipt exceeds the configured byte limit")
    return encoded


def finite_batch_seal_receipt_sha256(
    value: manifest_v2_pb2.FiniteBatchCompletionV2,
) -> str:
    """Return lowercase SHA-256 over the shared finite-batch receipt projection."""
    return hashlib.sha256(finite_batch_seal_receipt_protojson_bytes(value)).hexdigest()


def dataset_completion_evidence_v2_protojson_bytes(
    value: manifest_v2_pb2.DatasetManifestV2,
) -> bytes:
    """Return canonical compact ProtoJSON bytes for a validated manifest completion oneof."""
    if not isinstance(value, manifest_v2_pb2.DatasetManifestV2):
        raise ValueError("completion evidence projection requires DatasetManifestV2")
    _validate_dataset_manifest_v2(value)
    document = json_format.MessageToDict(
        value.completion_evidence, preserving_proto_field_name=False
    )
    encoded = json.dumps(document, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode(
        "utf-8"
    )
    if len(encoded) > MAX_FINITE_BATCH_SEAL_RECEIPT_V2_JSON_BYTES:
        raise ValueError("completion evidence projection exceeds the configured byte limit")
    return encoded


def dataset_completion_evidence_v2_sha256(
    value: manifest_v2_pb2.DatasetManifestV2,
) -> str:
    """Return lowercase SHA-256 over a validated manifest's canonical completion oneof."""
    return hashlib.sha256(dataset_completion_evidence_v2_protojson_bytes(value)).hexdigest()


def parse_us_equity_trade_bar_v2_protojson(
    document: Mapping[str, object],
) -> trade_bar_pb2.UsEquityTradeBarV2:
    """Parse the bounded lossless BarV2 ProtoJSON projection and enforce required fields."""
    if not isinstance(document, Mapping):
        raise ValueError("BarV2 JSON must be an object")
    try:
        encoded_size = len(
            json.dumps(document, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")
        )
    except (TypeError, ValueError) as error:
        raise ValueError("BarV2 JSON must contain only JSON values") from error
    if encoded_size > MAX_US_EQUITY_TRADE_BAR_V2_JSON_BYTES:
        raise ValueError("BarV2 JSON exceeds the configured byte limit")
    _reject_duplicate_proto_field_spellings(document)
    _validate_proto_timestamp_v2_fields(document)
    required_fields = (
        "schemaVersion", "sourceProvider", "sourceFeed", "sourceEntitlement",
        "sourceNumericEncoding", "symbol", "barStartUtc", "barEndExclusiveUtc",
        "availableAtUtc", "tradeDate", "sessionId", "sessionTimezone", "sessionPolicyId",
        "sessionPolicySha256", "sessionStartUtc", "sessionEndExclusiveUtc", "windowStartUtc",
        "windowEndExclusiveUtc", "open", "high", "low", "close", "volume", "tradeCount",
        "quoteEventsExcluded", "sourceTimestampMissingRows", "sequenceGapCount", "lateEventCount",
        "windowExpectedMinutes", "windowEmptyTradeMinutes", "sourceStartUtc", "sourceEndExclusiveUtc",
        "windowInputEof", "completionMode", "nbboInputStatus", "completionEvidenceSha256",
    )
    for field in required_fields:
        snake = re.sub(r"[A-Z]", lambda match: "_" + match.group(0).lower(), field)
        if field not in document and snake not in document:
            raise ValueError(f"BarV2 missing required field: {field}")
    validate_uint64_json_paths(
        document,
        tuple(
            (field,)
            for field in (
                "tradeCount", "quoteEventsExcluded", "sourceTimestampMissingRows",
                "sequenceGapCount", "lateEventCount", "windowExpectedMinutes",
                "windowEmptyTradeMinutes",
            )
        ),
    )
    version = document.get("schemaVersion", document.get("schema_version"))
    if type(version) is not int or version != 2:
        raise ValueError("BarV2 schema_version must be the integer 2")
    message = json_format.ParseDict(document, trade_bar_pb2.UsEquityTradeBarV2())
    if not _valid_sha256(message.completion_evidence_sha256):
        raise ValueError("BarV2 completion evidence digest must be lowercase SHA-256")
    if message.source_entitlement not in {"unknown", "authorized", "unauthorized"}:
        raise ValueError("BarV2 source entitlement is not a supported observation")
    if message.source_numeric_encoding not in {
        "decimal_token", "integer_token", "binary_float64_shortest_decimal",
        "binary_float32_shortest_decimal",
    }:
        raise ValueError("BarV2 source numeric encoding must describe normalized values")
    if not valid_market_symbol(message.symbol):
        raise ValueError("BarV2 symbol is invalid")
    validate_date_iso8601(message.trade_date)
    _validate_us_equity_trade_bar_v2_shape(message)
    return message


def validate_bar_v2_completion_evidence_reference(
    row: trade_bar_pb2.UsEquityTradeBarV2,
    manifest: manifest_v2_pb2.DatasetManifestV2,
) -> None:
    """Require BarV2's evidence reference to equal the validated manifest completion hash."""
    if not isinstance(row, trade_bar_pb2.UsEquityTradeBarV2) or row.schema_version != 2:
        raise ValueError("bar completion reference requires a UsEquityTradeBarV2 row")
    expected = dataset_completion_evidence_v2_sha256(manifest)
    if not _valid_sha256(row.completion_evidence_sha256) or row.completion_evidence_sha256 != expected:
        raise ValueError("bar completion evidence hash does not match the dataset manifest")


def validate_us_equity_trade_bar_v2_against_manifest(
    row: trade_bar_pb2.UsEquityTradeBarV2,
    manifest: manifest_v2_pb2.DatasetManifestV2,
) -> None:
    """Validate a BarV2 row's source, time, completion, and evidence binding to its manifest."""
    if not isinstance(row, trade_bar_pb2.UsEquityTradeBarV2):
        raise ValueError("BarV2 manifest validation requires a UsEquityTradeBarV2 row")
    _validate_us_equity_trade_bar_v2_shape(row)
    _validate_dataset_manifest_v2(manifest)
    source = manifest.source
    encoding = {
        1: "decimal_token",
        2: "integer_token",
        3: "binary_float64_shortest_decimal",
        4: "binary_float32_shortest_decimal",
    }.get(source.numeric_encoding)
    if (
        row.source_provider != source.provider
        or row.source_feed != source.feed
        or row.source_entitlement != source.entitlement
        or row.source_numeric_encoding != encoding
        or row.symbol not in manifest.symbols
        or row.source_timestamp_missing_rows != manifest.source_timestamp_missing_rows
    ):
        raise ValueError("BarV2 source fields do not match the dataset manifest")

    evidence_case = manifest.completion_evidence.WhichOneof("evidence")
    if evidence_case == "finite_batch":
        finite = manifest.completion_evidence.finite_batch
        expected_mode = "finite_batch"
        expected_eof = True
        expected_pages = finite.source_kind == 2
    elif evidence_case == "provider_watermark":
        expected_mode = "provider_watermark"
        expected_eof = False
        expected_pages = None
    elif evidence_case == "diagnostic_stream":
        expected_mode = "diagnostic_stream"
        expected_eof = False
        expected_pages = None
    else:
        raise ValueError("dataset manifest has no completion evidence oneof")
    has_pages = row.HasField("source_pages_exhausted")
    if (
        row.completion_mode != expected_mode
        or row.window_input_eof != expected_eof
        or has_pages != (expected_pages is not None)
        or (expected_pages is not None and row.source_pages_exhausted != expected_pages)
    ):
        raise ValueError("BarV2 completion fields do not match the dataset manifest")

    validate_bar_v2_completion_evidence_reference(row, manifest)
    if not manifest.HasField("time_range"):
        raise ValueError("BarV2 source bounds require a manifest time range")
    manifest_start = _timestamp_key(manifest.time_range.start_inclusive)
    manifest_end = _timestamp_key(manifest.time_range.end_exclusive)
    if (
        _timestamp_key(row.source_start_utc) < manifest_start
        or _timestamp_key(row.source_end_exclusive_utc) > manifest_end
    ):
        raise ValueError("BarV2 source bounds exceed the manifest time range")


def _validate_us_equity_trade_bar_v2_shape(row: trade_bar_pb2.UsEquityTradeBarV2) -> None:
    timestamp_fields = (
        "bar_start_utc", "bar_end_exclusive_utc", "available_at_utc",
        "session_start_utc", "session_end_exclusive_utc", "window_start_utc",
        "window_end_exclusive_utc", "source_start_utc", "source_end_exclusive_utc",
    )
    if (
        row.schema_version != 2
        or any(not row.HasField(field) for field in timestamp_fields)
        or not valid_source_identity(row.source_provider)
        or not valid_source_identity(row.source_feed)
        or row.source_entitlement not in {"unknown", "authorized", "unauthorized"}
        or row.source_numeric_encoding not in {
            "decimal_token", "integer_token", "binary_float64_shortest_decimal",
            "binary_float32_shortest_decimal",
        }
        or not valid_market_symbol(row.symbol)
        or not _valid_core_identity(row.session_id, 128)
        or not valid_source_identity(row.session_timezone)
        or not _valid_core_identity(row.session_policy_id, 256)
        or not _valid_sha256(row.session_policy_sha256)
        or row.completion_mode not in {"finite_batch", "provider_watermark", "diagnostic_stream"}
        or not 1 <= len(row.nbbo_input_status.encode("utf-8")) <= 128
        or row.nbbo_input_status.strip() != row.nbbo_input_status
        or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in row.nbbo_input_status)
        or row.trade_count == 0
        or row.window_expected_minutes == 0
        or row.window_empty_trade_minutes > row.window_expected_minutes
        or row.HasField("source_pages_exhausted") and not row.source_pages_exhausted
    ):
        raise ValueError("BarV2 row identity, count, or completion fields are invalid")

    open_value = _parse_exact_decimal(row.open)
    high_value = _parse_exact_decimal(row.high)
    low_value = _parse_exact_decimal(row.low)
    close_value = _parse_exact_decimal(row.close)
    volume_value = _parse_exact_decimal(row.volume)
    if (
        min(open_value, high_value, low_value, close_value) <= 0
        or volume_value < 0
        or high_value < max(open_value, close_value, low_value)
        or low_value > min(open_value, close_value)
    ):
        raise ValueError("BarV2 exact OHLCV values are inconsistent")

    instants = {
        field: _timestamp_key(getattr(row, field))
        for field in (
            "bar_start_utc", "bar_end_exclusive_utc", "available_at_utc",
            "session_start_utc", "session_end_exclusive_utc", "window_start_utc",
            "window_end_exclusive_utc", "source_start_utc", "source_end_exclusive_utc",
        )
    }
    minute_ns = 60_000_000_000
    if (
        instants["session_start_utc"] >= instants["session_end_exclusive_utc"]
        or instants["window_start_utc"] >= instants["window_end_exclusive_utc"]
        or instants["window_start_utc"] < instants["session_start_utc"]
        or instants["window_end_exclusive_utc"] > instants["session_end_exclusive_utc"]
        or _timestamp_nanoseconds(instants["bar_end_exclusive_utc"])
        - _timestamp_nanoseconds(instants["bar_start_utc"])
        != minute_ns
        or instants["bar_start_utc"] < instants["window_start_utc"]
        or instants["bar_end_exclusive_utc"] > instants["window_end_exclusive_utc"]
        or instants["bar_start_utc"] < instants["session_start_utc"]
        or instants["bar_end_exclusive_utc"] > instants["session_end_exclusive_utc"]
        or instants["available_at_utc"] < instants["bar_end_exclusive_utc"]
        or instants["source_start_utc"] >= instants["source_end_exclusive_utc"]
    ):
        raise ValueError("BarV2 timestamps are inconsistent")


def _timestamp_key(value: object) -> tuple[int, int]:
    if (
        not -62_135_596_800 <= value.seconds <= 253_402_300_799
        or not 0 <= value.nanos <= 999_999_999
    ):
        raise ValueError("BarV2 timestamp is outside the Protobuf Timestamp range")
    return value.seconds, value.nanos


def _timestamp_nanoseconds(value: tuple[int, int]) -> int:
    return value[0] * 1_000_000_000 + value[1]


def _valid_core_identity(value: str, max_bytes: int) -> bool:
    return (
        0 < len(value.encode("utf-8")) <= max_bytes
        and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]*", value) is not None
        and ".." not in value
        and not any(token.lower() in {"latest", "fallback"} for token in re.split(r"[._:-]", value))
    )


def _parse_exact_decimal(value: str) -> Decimal:
    match = _EXACT_DECIMAL.fullmatch(value) if isinstance(value, str) and value.isascii() and len(value) <= 128 else None
    if match is None:
        raise ValueError("BarV2 decimal is not a bounded JSON number")
    sign, integer, fraction, exponent_text = match.groups()
    exponent = int(exponent_text or "0")
    if abs(exponent) > 38:
        raise ValueError("BarV2 decimal exponent is outside the exact-decimal bound")
    digits = (integer + (fraction or "")).lstrip("0")
    if not digits:
        return Decimal(0)
    scale = len(fraction or "") - exponent
    if scale < 0:
        digits += "0" * -scale
        if len(digits) > 128:
            raise ValueError("BarV2 decimal coefficient is outside the exact-decimal bound")
        scale = 0
    while scale > 0 and digits.endswith("0"):
        digits = digits[:-1]
        scale -= 1
    if scale > 28:
        raise ValueError("BarV2 decimal scale is outside the exact-decimal bound")
    coefficient = int(digits) * (-1 if sign else 1)
    if not -(1 << 127) <= coefficient <= (1 << 127) - 1:
        raise ValueError("BarV2 decimal coefficient overflows signed 128-bit")
    return Decimal((1 if coefficient < 0 else 0, tuple(int(digit) for digit in str(abs(coefficient))), -scale))


def _validate_dataset_manifest_v2(message: manifest_v2_pb2.DatasetManifestV2) -> None:
    if message.schema_version != 2:
        raise ValueError("unsupported dataset manifest v2 schema version")
    if not valid_dataset_id(message.dataset_id):
        raise ValueError("invalid dataset v2 identity")
    if len(message.symbols) == 0 or len(message.symbols) > MAX_DATASET_MANIFEST_V2_SYMBOLS:
        raise ValueError("dataset v2 symbol count is empty or exceeds its bound")
    if not valid_sorted_symbols(message.symbols):
        raise ValueError("dataset v2 symbols must be valid, sorted, and unique")
    if not message.HasField("source") or not message.HasField("object"):
        raise ValueError("dataset v2 source and object are required")

    source = message.source
    if (
        not valid_source_identity(source.provider)
        or not valid_source_identity(source.feed)
        or source.numeric_encoding == 0
        or source.entitlement not in {"unknown", "authorized", "unauthorized"}
        or source.HasField("source_record_id")
        and not valid_source_identity(source.source_record_id)
    ):
        raise ValueError("invalid dataset v2 source identity")
    synthetic_mentioned = source.provider.lower() == "synthetic" or source.feed.lower() == "synthetic"
    if synthetic_mentioned and (
        source.provider != "synthetic"
        or source.feed != "synthetic"
        or source.numeric_encoding != 1
    ):
        raise ValueError("synthetic source identity is inconsistent")

    if message.row_count == 0 or message.source_timestamp_missing_rows > message.row_count:
        raise ValueError("dataset v2 row counts are inconsistent")
    has_time_range = message.HasField("time_range")
    if message.source.numeric_encoding in {5, 6}:
        if has_time_range or message.source_timestamp_missing_rows != message.row_count:
            raise ValueError("raw-frame manifests require no source time and all timestamps missing")
    elif message.source_timestamp_missing_rows == message.row_count:
        if has_time_range:
            raise ValueError("dataset v2 with all source timestamps missing cannot have a time range")
    elif not has_time_range:
        raise ValueError("dataset v2 with source timestamps requires a time range")
    if has_time_range:
        start = message.time_range.start_inclusive
        end = message.time_range.end_exclusive
        if not message.time_range.HasField("start_inclusive") or not message.time_range.HasField(
            "end_exclusive"
        ):
            raise ValueError("dataset v2 time range requires both bounds")
        if (start.seconds, start.nanos) >= (end.seconds, end.nanos):
            raise ValueError("dataset v2 time range must be non-empty and half-open")

    obj = message.object
    if (
        not valid_object_name(obj.object_name)
        or obj.size_bytes == 0
        or obj.parquet_footer_rows != message.row_count
        or not _valid_sha256(obj.content_sha256)
        or not _valid_sha256(obj.parquet_schema_sha256)
    ):
        raise ValueError("invalid dataset v2 object")
    if obj.transport not in {"local_test", "rclone_google_drive"}:
        raise ValueError("unsupported dataset v2 object transport")
    has_object_id = obj.HasField("object_id")
    if not has_object_id or not valid_object_id(
        obj.object_id, local_test=obj.transport == "local_test"
    ):
        raise ValueError("invalid dataset v2 object identity")
    raw_schema_id = _raw_schema_id_for_value(source.numeric_encoding)
    registered_raw_hashes = {
        trusted_parquet_schema_sha256(schema_id)
        for schema_id in RAW_ENCODING_SCHEMA_IDS.values()
    }
    if raw_schema_id is None:
        if obj.parquet_schema_sha256 in registered_raw_hashes:
            raise ValueError("raw-frame schema fingerprint requires its matching raw source encoding")
    elif obj.parquet_schema_sha256 != trusted_parquet_schema_sha256(raw_schema_id):
        raise ValueError("raw source encoding must match its registered Parquet schema")

    if not message.HasField("storage_verification"):
        raise ValueError("dataset v2 requires storage verification evidence")
    storage = message.storage_verification
    if (
        not storage.verified_before_publish
        or not _valid_sha256(storage.readback_sha256)
        or storage.readback_sha256 != obj.content_sha256
    ):
        raise ValueError("dataset v2 object readback verification is inconsistent")

    evidence = message.completion_evidence
    evidence_case = evidence.WhichOneof("evidence")
    if evidence_case == "finite_batch":
        _validate_finite_batch_v2(evidence.finite_batch, source, message)
    elif evidence_case == "provider_watermark":
        _validate_provider_watermark_v2(evidence.provider_watermark, source, message)
    elif evidence_case == "diagnostic_stream":
        _validate_diagnostic_stream_v2(evidence.diagnostic_stream)
    else:
        raise ValueError("dataset v2 requires exactly one completion evidence case")


def _validate_finite_batch_v2(value: object, source: object, manifest: object) -> None:
    kind = value.source_kind
    if kind == 0 or not valid_dataset_id(value.input_identity):
        raise ValueError("finite batch identity or source kind is invalid")
    if (
        not _valid_sha256(value.input_sha256)
        or value.input_size_bytes == 0
        or value.input_record_count == 0
        or value.input_record_count != value.consumed_record_count
        or not _valid_sha256(value.reviewed_policy_sha256)
        or not _valid_sha256(value.seal_receipt_sha256)
    ):
        raise ValueError("finite batch input receipt is incomplete or inconsistent")
    if finite_batch_seal_receipt_sha256(value) != value.seal_receipt_sha256:
        raise ValueError("finite batch seal receipt hash does not match the shared projection")
    timestamps = (value.data_cutoff_exclusive, value.sealed_at, value.completed_at)
    if any(not value.HasField(field) for field in ("data_cutoff_exclusive", "sealed_at", "completed_at")):
        raise ValueError("finite batch requires cutoff, seal, and completion timestamps")
    if not _timestamps_ordered(timestamps):
        raise ValueError("finite batch timestamps must satisfy cutoff <= seal <= completion")
    if manifest.HasField("time_range") and (
        manifest.time_range.end_exclusive.seconds,
        manifest.time_range.end_exclusive.nanos,
    ) > (value.data_cutoff_exclusive.seconds, value.data_cutoff_exclusive.nanos):
        raise ValueError("finite batch cutoff precedes manifest source-time coverage")
    paged = kind == 2
    page_presence = (value.HasField("page_count"), value.HasField("pages_exhausted"), value.HasField("page_set_sha256"))
    if paged:
        if (
            not all(page_presence)
            or value.page_count == 0
            or not value.pages_exhausted
            or not _valid_sha256(value.page_set_sha256)
        ):
            raise ValueError("paged finite batches require positive count, exhaustion, and page hash")
    elif any(page_presence):
        raise ValueError("non-paged finite batches must omit page evidence")
    synthetic = source.provider == "synthetic" and source.feed == "synthetic"
    if kind == 1 and not synthetic or kind in {2, 3} and synthetic:
        raise ValueError("finite batch source kind does not match source identity")


def _validate_finite_batch_receipt_fields(value: object) -> None:
    kind = value.source_kind
    if (
        kind not in {1, 2, 3, 4}
        or not valid_dataset_id(value.input_identity)
        or not _valid_sha256(value.input_sha256)
        or value.input_size_bytes == 0
        or value.input_record_count == 0
        or value.input_record_count != value.consumed_record_count
        or not _valid_sha256(value.reviewed_policy_sha256)
        or not all(
            value.HasField(field)
            for field in ("data_cutoff_exclusive", "sealed_at", "completed_at")
        )
        or not _timestamps_ordered(
            (value.data_cutoff_exclusive, value.sealed_at, value.completed_at)
        )
    ):
        raise ValueError("finite batch receipt projection fields are incomplete or inconsistent")
    page_presence = (
        value.HasField("page_count"),
        value.HasField("pages_exhausted"),
        value.HasField("page_set_sha256"),
    )
    if kind == 2:
        if (
            not all(page_presence)
            or value.page_count == 0
            or not value.pages_exhausted
            or not _valid_sha256(value.page_set_sha256)
        ):
            raise ValueError("paged finite batch receipt is incomplete")
    elif any(page_presence):
        raise ValueError("non-paged finite batch receipt must omit page evidence")


def _validate_provider_watermark_v2(value: object, source: object, manifest: object) -> None:
    if (
        value.provider != source.provider
        or value.feed != source.feed
        or not valid_dataset_id(value.subscription_instance_id)
        or value.generation == 0
        or value.first_sequence == 0
        or value.last_sequence < value.first_sequence
        or value.sequence_count != value.last_sequence - value.first_sequence + 1
        or not _valid_sha256(value.continuity_receipt_sha256)
        or value.allowed_lateness_ns > MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS
        or not _valid_sha256(value.reviewed_policy_sha256)
        or not _valid_sha256(value.source_receipt_sha256)
        or manifest.source_timestamp_missing_rows != 0
        or not manifest.HasField("time_range")
        or not value.HasField("complete_up_to_exclusive")
    ):
        raise ValueError("provider watermark evidence is inconsistent")
    if (manifest.time_range.end_exclusive.seconds, manifest.time_range.end_exclusive.nanos) > (
        value.complete_up_to_exclusive.seconds,
        value.complete_up_to_exclusive.nanos,
    ):
        raise ValueError("provider watermark does not cover manifest source time")
    if source.provider == "synthetic" or source.feed == "synthetic":
        raise ValueError("synthetic source cannot claim a provider watermark")


def _validate_diagnostic_stream_v2(value: object) -> None:
    if (
        not valid_dataset_id(value.source_instance_id)
        or value.generation == 0
        or value.observed_last_sequence == 0
        or not value.HasField("local_policy_cutoff")
        or not _valid_sha256(value.diagnostic_policy_sha256)
        or not _valid_sha256(value.diagnostic_receipt_sha256)
    ):
        raise ValueError("diagnostic stream evidence is inconsistent")


def _timestamps_ordered(values: tuple[object, object, object]) -> bool:
    return all(
        (left.seconds, left.nanos) <= (right.seconds, right.nanos)
        for left, right in zip(values, values[1:])
    )


def _valid_sha256(value: str) -> bool:
    return _SHA256.fullmatch(value) is not None


def _validate_proto_timestamp_v2_fields(value: object) -> None:
    if isinstance(value, Mapping):
        for key, nested in value.items():
            if key in _PROTO_TIMESTAMP_V2_FIELDS and nested is not None:
                if not isinstance(nested, str):
                    raise ValueError("protobuf timestamp must be an RFC3339 string")
                match = _PROTO_TIMESTAMP_V2.fullmatch(nested)
                if match is None:
                    raise ValueError("protobuf timestamp must preserve at most 9 fractional digits")
                year, _, _, hour, minute, second, _, _, offset_hour, offset_minute = match.groups()
                if (
                    int(year) == 0
                    or int(hour) > 23
                    or int(minute) > 59
                    or int(second) > 59
                    or offset_hour is not None
                    and (int(offset_hour) > 23 or int(offset_minute) > 59)
                ):
                    raise ValueError("protobuf timestamp contains an out-of-range time component")
            else:
                _validate_proto_timestamp_v2_fields(nested)
    elif isinstance(value, list):
        for nested in value:
            _validate_proto_timestamp_v2_fields(nested)


def _validate_dataset_manifest_v2_enum_names(document: Mapping[str, object]) -> None:
    source = _mapping_field(document, "source", "source")
    if source is not None:
        numeric_encoding = source.get("numericEncoding", source.get("numeric_encoding"))
        if not isinstance(numeric_encoding, str) or numeric_encoding not in _NUMERIC_ENCODING_V2_NAMES:
            raise ValueError("dataset v2 enum fields must use supported ProtoJSON names")

    completion = _mapping_field(document, "completionEvidence", "completion_evidence")
    if completion is None:
        return
    finite = _mapping_field(completion, "finiteBatch", "finite_batch")
    if finite is not None:
        source_kind = finite.get("sourceKind", finite.get("source_kind"))
        if not isinstance(source_kind, str) or source_kind not in _FINITE_SOURCE_KIND_V2_NAMES:
            raise ValueError("dataset v2 enum fields must use supported ProtoJSON names")


def _mapping_field(document: Mapping[str, object], camel: str, snake: str) -> Mapping[str, object] | None:
    value = document.get(camel, document.get(snake))
    return value if isinstance(value, Mapping) else None


def _has_field(document: Mapping[str, object], camel: str, snake: str) -> bool:
    return camel in document or snake in document


def _exactly_one_mapping_case(
    document: Mapping[str, object], cases: tuple[str, ...]
) -> str | None:
    normalized = {_snake_to_camel(key): key for key in document}
    present = [case for case in cases if case in normalized]
    if len(present) != 1:
        return None
    value = document[normalized[present[0]]]
    return present[0] if isinstance(value, Mapping) else None


def _reject_duplicate_proto_field_spellings(value: object) -> None:
    if isinstance(value, Mapping):
        keys = tuple(key for key in value if isinstance(key, str))
        canonical = {_snake_to_camel(key) for key in keys}
        if len(canonical) != len(keys):
            raise ValueError("ProtoJSON field must not use both camelCase and snake_case spellings")
        for nested in value.values():
            _reject_duplicate_proto_field_spellings(nested)
    elif isinstance(value, list):
        for nested in value:
            _reject_duplicate_proto_field_spellings(nested)


def _snake_to_camel(value: str) -> str:
    return re.sub(r"_([a-z])", lambda match: match.group(1).upper(), value)


def _unique_object_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON object key: {key}")
        result[key] = value
    return result


def parse_prediction_envelope_protojson(
    document: Mapping[str, object],
) -> prediction_pb2.PredictionEnvelopeV1:
    validate_uint64_json_paths(document, (("forecast", "sequence"),))
    _reject_duplicate_numeric_encoding_aliases(document)
    message = json_format.ParseDict(document, prediction_pb2.PredictionEnvelopeV1())
    if _raw_schema_id_for_value(message.source.numeric_encoding) is not None:
        raise ValueError("raw byte-frame encodings cannot identify a normalized prediction source")
    return message


def _reject_duplicate_numeric_encoding_aliases(document: Mapping[str, object]) -> None:
    source = document.get("source")
    if (
        isinstance(source, Mapping)
        and "numericEncoding" in source
        and "numeric_encoding" in source
    ):
        raise ValueError("source numeric encoding must not use both ProtoJSON field spellings")


def _raw_schema_id_for_value(value: object) -> str | None:
    if isinstance(value, str) or type(value) is int:
        return RAW_ENCODING_SCHEMA_IDS.get(value)
    return None
