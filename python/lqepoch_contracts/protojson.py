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
    if _raw_encoding_schema_id(document) is not None:
        raise ValueError("raw byte-frame encodings are not normalized market-event encodings")
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
    raw_schema_id = _raw_encoding_schema_id(document)
    object_document = document.get("object")
    schema_sha256 = (
        object_document.get("parquetSchemaSha256")
        if isinstance(object_document, Mapping)
        else None
    )
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
    if isinstance(source, Mapping) and _raw_schema_id_for_value(source.get("numericEncoding")):
        raise ValueError("raw byte-frame encodings cannot identify a normalized prediction source")
    return json_format.ParseDict(document, prediction_pb2.PredictionEnvelopeV1())


def _raw_encoding_schema_id(document: Mapping[str, object]) -> str | None:
    source = document.get("source")
    if not isinstance(source, Mapping):
        return None
    return _raw_schema_id_for_value(source.get("numericEncoding"))


def _raw_schema_id_for_value(value: object) -> str | None:
    if isinstance(value, str) or type(value) is int:
        return RAW_ENCODING_SCHEMA_IDS.get(value)
    return None
