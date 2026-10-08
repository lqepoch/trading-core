"""Lossless validated ProtoJSON entry points for cross-language consumers."""

from __future__ import annotations

from collections.abc import Mapping

from google.protobuf import json_format

from lqepoch.dataset.v1 import manifest_pb2
from lqepoch.market.v1 import market_pb2
from lqepoch.prediction.v1 import prediction_pb2

from .parquet_schema import trusted_parquet_schema_sha256
from .uint64_json import validate_uint64_json_paths

RAW_MESSAGEPACK_ENCODING = "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES"
RAW_MESSAGEPACK_ENCODING_NUMBER = 5
RAW_FRAME_SCHEMA_ID = "lqepoch.market_raw_frame.v1"


def parse_market_event_protojson(document: Mapping[str, object]) -> market_pb2.MarketEventEnvelopeV1:
    validate_uint64_json_paths(document, (("generation",), ("sequence",)))
    if _has_raw_messagepack_encoding(document):
        raise ValueError("raw_messagepack_bytes is not a normalized market-event encoding")
    return json_format.ParseDict(document, market_pb2.MarketEventEnvelopeV1())


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
    raw_schema_sha256 = trusted_parquet_schema_sha256(RAW_FRAME_SCHEMA_ID)
    is_raw_encoding = _has_raw_messagepack_encoding(document)
    object_document = document.get("object")
    schema_sha256 = (
        object_document.get("parquetSchemaSha256")
        if isinstance(object_document, Mapping)
        else None
    )
    if is_raw_encoding != (schema_sha256 == raw_schema_sha256):
        raise ValueError(
            "raw_messagepack_bytes must be bound to the registered market_raw_frame.v1 schema"
        )
    if is_raw_encoding:
        row_count = document.get("rowCount")
        missing_rows = document.get("sourceTimestampMissingRows")
        if (
            "timeRange" in document
            or "time_range" in document
            or row_count == "0"
            or row_count != missing_rows
        ):
            raise ValueError(
                "raw-frame manifests require rows, no source-time range, and every frame timestamp missing"
            )
    return json_format.ParseDict(document, manifest_pb2.DatasetManifestV1())


def parse_prediction_envelope_protojson(
    document: Mapping[str, object],
) -> prediction_pb2.PredictionEnvelopeV1:
    validate_uint64_json_paths(document, (("forecast", "sequence"),))
    source = document.get("source")
    if isinstance(source, Mapping) and _is_raw_encoding(source.get("numericEncoding")):
        raise ValueError("raw_messagepack_bytes cannot identify a normalized prediction source")
    return json_format.ParseDict(document, prediction_pb2.PredictionEnvelopeV1())


def _has_raw_messagepack_encoding(document: Mapping[str, object]) -> bool:
    source = document.get("source")
    return isinstance(source, Mapping) and _is_raw_encoding(source.get("numericEncoding"))


def _is_raw_encoding(value: object) -> bool:
    return value in (RAW_MESSAGEPACK_ENCODING, RAW_MESSAGEPACK_ENCODING_NUMBER)
