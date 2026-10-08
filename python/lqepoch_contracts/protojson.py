"""Lossless validated ProtoJSON entry points for cross-language consumers."""

from __future__ import annotations

from collections.abc import Mapping

from google.protobuf import json_format

from lqepoch.dataset.v1 import manifest_pb2
from lqepoch.market.v1 import market_pb2
from lqepoch.prediction.v1 import prediction_pb2

from .parquet_schema import trusted_parquet_schema_sha256
from .uint64_json import validate_uint64_json_paths

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
