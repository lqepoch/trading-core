"""Lossless validated ProtoJSON entry points for cross-language consumers."""

from __future__ import annotations

from collections.abc import Mapping

from google.protobuf import json_format

from lqepoch.dataset.v1 import manifest_pb2
from lqepoch.market.v1 import market_pb2
from lqepoch.prediction.v1 import prediction_pb2

from .uint64_json import validate_uint64_json_paths


def parse_market_event_protojson(document: Mapping[str, object]) -> market_pb2.MarketEventEnvelopeV1:
    validate_uint64_json_paths(document, (("generation",), ("sequence",)))
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
    return json_format.ParseDict(document, manifest_pb2.DatasetManifestV1())


def parse_prediction_envelope_protojson(
    document: Mapping[str, object],
) -> prediction_pb2.PredictionEnvelopeV1:
    validate_uint64_json_paths(document, (("forecast", "sequence"),))
    return json_format.ParseDict(document, prediction_pb2.PredictionEnvelopeV1())
