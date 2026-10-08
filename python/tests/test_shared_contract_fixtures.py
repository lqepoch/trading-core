from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path

from google.protobuf.json_format import MessageToDict

from lqepoch_contracts.identities import (
    valid_dataset_id,
    valid_market_symbol,
    valid_object_id,
    valid_object_name,
    valid_sorted_symbols,
    valid_source_identity,
)
from lqepoch_contracts import (
    load_trusted_parquet_schema_registry,
    trusted_parquet_schema_descriptor,
    trusted_parquet_schema_sha256,
)
from lqepoch_contracts.parquet_schema import canonical_schema_json, fingerprint_schema_sha256
from lqepoch_contracts.protojson import (
    parse_dataset_manifest_protojson,
    parse_market_event_protojson,
    parse_prediction_envelope_protojson,
)
from lqepoch_contracts.uint64_json import parse_uint64_json

REPO_ROOT = Path(__file__).resolve().parents[2]


def read_json_fixture(relative_path: str) -> object:
    return json.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))


class SharedContractFixturesTest(unittest.TestCase):
    def test_uint64_fixture_is_strict_and_matches_protojson_roundtrip(self) -> None:
        fixture = read_json_fixture("schemas/fixtures/uint64-json-v1.json")
        for case in fixture["valid"]:
            parsed = parse_uint64_json(case["value"])
            self.assertEqual(str(parsed), case["value"], case["name"])
        for case in fixture["invalid"]:
            with self.subTest(case=case["name"]), self.assertRaises(ValueError):
                parse_uint64_json(case["value"])

        market_json = {
            "schemaVersion": 1,
            "source": {
                "provider": "synthetic",
                "feed": "synthetic",
                "entitlement": "unknown",
                "numericEncoding": "NUMERIC_ENCODING_DECIMAL_TOKEN",
            },
            "generation": "9007199254740993",
            "sequence": "18446744073709551615",
            "receivedTimestamp": "2026-10-08T14:30:00Z",
            "event": {"optionQuote": {"symbol": "QQQ261016C00600000", "bid": "1.25"}},
        }
        message = parse_market_event_protojson(market_json)
        self.assertEqual(message.generation, 9007199254740993)
        self.assertEqual(message.sequence, (1 << 64) - 1)
        roundtrip = MessageToDict(message)
        self.assertEqual(roundtrip["generation"], market_json["generation"])
        self.assertEqual(roundtrip["sequence"], market_json["sequence"])

        for case in fixture["invalid"]:
            invalid = {**market_json, "generation": case["value"]}
            with self.subTest(proto_case=case["name"]), self.assertRaises(ValueError):
                parse_market_event_protojson(invalid)

        max_u64 = "18446744073709551615"
        dataset_json = {
            "schemaVersion": 1,
            "datasetId": "dataset:version-1",
            "source": {
                "provider": "synthetic",
                "feed": "synthetic",
                "entitlement": "unknown",
                "numericEncoding": "NUMERIC_ENCODING_DECIMAL_TOKEN",
            },
            "symbols": ["QQQ"],
            "sourceTimestampMissingRows": max_u64,
            "rowCount": max_u64,
            "object": {
                "objectName": "fixture.parquet",
                "objectId": "local-test:fixture-1",
                "sizeBytes": max_u64,
                "contentSha256": "a" * 64,
                "parquetSchemaSha256": "b" * 64,
                "parquetFooterRows": max_u64,
                "transport": "local_test",
            },
            "completion": {
                "inputEof": True,
                "sourcePagesExhausted": True,
                "readbackSha256": "a" * 64,
                "verifiedBeforePublish": True,
            },
        }
        manifest = parse_dataset_manifest_protojson(dataset_json)
        self.assertEqual(manifest.row_count, (1 << 64) - 1)
        self.assertEqual(manifest.object.size_bytes, (1 << 64) - 1)
        for case in fixture["invalid"]:
            invalid = {
                **dataset_json,
                "object": {**dataset_json["object"], "sizeBytes": case["value"]},
            }
            with self.subTest(dataset_case=case["name"]), self.assertRaises(ValueError):
                parse_dataset_manifest_protojson(invalid)

        prediction = parse_prediction_envelope_protojson(
            {
                "createdAt": "2026-10-08T14:30:00.000000123Z",
                "forecast": {"sequence": max_u64},
            }
        )
        self.assertEqual(prediction.forecast.sequence, (1 << 64) - 1)
        self.assertEqual(prediction.created_at.nanos, 123)
        for case in fixture["invalid"]:
            with self.subTest(prediction_case=case["name"]), self.assertRaises(ValueError):
                parse_prediction_envelope_protojson(
                    {"forecast": {"sequence": case["value"]}}
                )

    def test_identity_fixture_matches_shared_utf8_and_path_boundaries(self) -> None:
        fixture = read_json_fixture("schemas/fixtures/dataset-manifest-v1-identities.json")
        positive = fixture["positive"]
        negative = fixture["negative"]

        for field in ("provider", "feed", "source_record_id"):
            self.assertTrue(valid_source_identity(positive[field]))
        self.assertFalse(valid_source_identity(negative["provider_utf8_over_limit"]))
        self.assertTrue(valid_dataset_id(positive["dataset_id"]))
        self.assertFalse(valid_dataset_id(negative["dataset_id_latest_selector"]))
        self.assertFalse(valid_dataset_id(negative["dataset_id_parent_token"]))
        self.assertTrue(valid_market_symbol(positive["padded_occ_symbol"]))
        self.assertTrue(valid_sorted_symbols([positive["padded_occ_symbol"]]))
        self.assertFalse(valid_market_symbol(negative["symbol_utf8_over_limit"]))
        self.assertTrue(valid_object_name(positive["object_name"]))
        self.assertFalse(valid_object_name(negative["object_name_wildcard"]))
        self.assertFalse(valid_object_name(negative["object_name_dot"]))
        self.assertTrue(valid_object_id(positive["drive_object_id"], local_test=False))
        self.assertTrue(valid_object_id(positive["local_test_object_id"], local_test=True))
        self.assertFalse(valid_object_id(negative["local_test_path_traversal"], local_test=True))

    def test_parquet_fingerprint_fixture_matches_the_shared_golden_bytes(self) -> None:
        fixture = read_json_fixture("schemas/fixtures/parquet-schema-registry.json")
        self.assertEqual(load_trusted_parquet_schema_registry(), fixture)
        self.assertEqual(fixture["fingerprint_prefix"], "LQEpoch-Parquet-Schema-v1\n")
        for golden in [fixture["generic_golden"], *fixture["schemas"]]:
            with self.subTest(schema_id=golden["descriptor"]["schema_id"]):
                schema_id = golden["descriptor"]["schema_id"]
                if schema_id != "test.v1":
                    self.assertEqual(
                        trusted_parquet_schema_descriptor(schema_id), golden["descriptor"]
                    )
                    self.assertEqual(trusted_parquet_schema_sha256(schema_id), golden["sha256"])
                self.assertEqual(
                    canonical_schema_json(golden["descriptor"]), golden["canonical_json"]
                )
                self.assertEqual(
                    fingerprint_schema_sha256(golden["descriptor"]), golden["sha256"]
                )

        for invalid in fixture["invalid_descriptors"]:
            with self.subTest(schema_case=invalid["name"]), self.assertRaises(ValueError):
                canonical_schema_json(invalid["descriptor"])
        with self.assertRaises(ValueError):
            canonical_schema_json(
                {
                    "schema_version": 1.0,
                    "schema_id": "test.v1",
                    "fields": [{"name": "symbol", "type": "utf8", "nullable": False}],
                }
            )

        generic = fixture["generic_golden"]
        self.assertEqual(
            hashlib.sha256(
                (fixture["fingerprint_prefix"] + generic["canonical_json"]).encode("utf-8")
            ).hexdigest(),
            generic["sha256"],
        )

        openapi = read_json_fixture("schemas/openapi.json")
        manifest_schema = openapi["components"]["schemas"]["DatasetManifestV1"]
        raw_schema_sha = trusted_parquet_schema_sha256("lqepoch.market_raw_frame.v1")
        raw_hash_condition = manifest_schema["allOf"][0]["then"]["properties"]["object"][
            "properties"
        ]["parquet_schema_sha256"]["const"]
        self.assertEqual(raw_hash_condition, raw_schema_sha)

    def test_raw_messagepack_encoding_is_bound_only_to_raw_frame_dataset_schema(self) -> None:
        from lqepoch_contracts.parquet_schema import trusted_parquet_schema_sha256

        raw_encoding = "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES"
        raw_schema_hash = trusted_parquet_schema_sha256("lqepoch.market_raw_frame.v1")
        event = {
            "schemaVersion": 1,
            "source": {
                "provider": "synthetic",
                "feed": "synthetic",
                "entitlement": "unknown",
                "numericEncoding": raw_encoding,
            },
            "generation": "1",
            "sequence": "1",
            "receivedTimestamp": "2026-10-08T14:30:00Z",
            "event": {"stockTrade": {"symbol": "QQQ", "price": "1.25", "size": "1"}},
        }
        with self.assertRaisesRegex(ValueError, "not a normalized market-event"):
            parse_market_event_protojson(event)

        manifest = {
            "schemaVersion": 1,
            "datasetId": "synthetic-raw-frame-v1",
            "source": {
                "provider": "synthetic",
                "feed": "synthetic",
                "entitlement": "unknown",
                "numericEncoding": raw_encoding,
            },
            "symbols": ["QQQ"],
            "sourceTimestampMissingRows": "1",
            "rowCount": "1",
            "object": {
                "objectName": "raw.parquet",
                "objectId": "local-test:raw-frame",
                "sizeBytes": "10",
                "contentSha256": "a" * 64,
                "parquetSchemaSha256": raw_schema_hash,
                "parquetFooterRows": "1",
                "transport": "local_test",
            },
            "completion": {
                "inputEof": True,
                "readbackSha256": "a" * 64,
                "verifiedBeforePublish": True,
            },
        }
        self.assertEqual(parse_dataset_manifest_protojson(manifest).source.numeric_encoding, 5)

        for invalid_raw_manifest in (
            {**manifest, "rowCount": "0", "sourceTimestampMissingRows": "0"},
            {key: value for key, value in manifest.items() if key != "rowCount"},
            {**manifest, "sourceTimestampMissingRows": "0"},
            {**manifest, "timeRange": {"startInclusive": "2026-10-08T00:00:00Z"}},
        ):
            with self.subTest(invalid_raw_manifest=invalid_raw_manifest), self.assertRaises(ValueError):
                parse_dataset_manifest_protojson(invalid_raw_manifest)

        wrong_hash = {**manifest, "object": {**manifest["object"], "parquetSchemaSha256": "b" * 64}}
        with self.assertRaisesRegex(ValueError, "must be bound to the registered"):
            parse_dataset_manifest_protojson(wrong_hash)

        wrong_encoding = {
            **manifest,
            "source": {
                **manifest["source"],
                "numericEncoding": "NUMERIC_ENCODING_DECIMAL_TOKEN",
            },
        }
        with self.assertRaisesRegex(ValueError, "must be bound to the registered"):
            parse_dataset_manifest_protojson(wrong_encoding)

        prediction = {
            "source": {
                "provider": "synthetic",
                "feed": "synthetic",
                "entitlement": "unknown",
                "datasetId": "synthetic-raw-frame-v1",
                "manifestSha256": "a" * 64,
                "datasetSha256": "b" * 64,
                "numericEncoding": raw_encoding,
            },
            "forecast": {"sequence": "1"},
        }
        with self.assertRaisesRegex(ValueError, "cannot identify a normalized prediction"):
            parse_prediction_envelope_protojson(prediction)


if __name__ == "__main__":
    unittest.main()
