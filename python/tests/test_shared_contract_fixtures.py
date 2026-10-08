from __future__ import annotations

import hashlib
import json
import copy
import unittest
from pathlib import Path

from google.protobuf import json_format
from google.protobuf.json_format import MessageToDict

from lqepoch.dataset.v2 import manifest_pb2 as manifest_v2_pb2
from lqepoch_contracts.identities import (
    valid_dataset_id,
    valid_market_symbol,
    valid_object_id,
    valid_object_name,
    valid_sorted_symbols,
    valid_source_identity,
)
from lqepoch_contracts import (
    dataset_completion_evidence_v2_protojson_bytes,
    dataset_completion_evidence_v2_sha256,
    finite_batch_seal_receipt_protojson_bytes,
    finite_batch_seal_receipt_sha256,
    load_trusted_parquet_schema_registry,
    trusted_parquet_schema_descriptor,
    trusted_parquet_schema_sha256,
)
from lqepoch_contracts.parquet_schema import canonical_schema_json, fingerprint_schema_sha256
from lqepoch_contracts.protojson import (
    parse_dataset_manifest_protojson,
    parse_dataset_manifest_v2_json,
    parse_dataset_manifest_v2_protojson,
    parse_us_equity_trade_bar_v2_protojson,
    validate_bar_v2_completion_evidence_reference,
    validate_us_equity_trade_bar_v2_against_manifest,
    parse_market_event_protojson,
    parse_prediction_envelope_protojson,
)
from lqepoch_contracts.uint64_json import parse_uint64_json, validate_uint64_json_paths

REPO_ROOT = Path(__file__).resolve().parents[2]


def read_json_fixture(relative_path: str) -> object:
    return json.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))


def refresh_finite_batch_receipt_hash(document: dict[str, object]) -> None:
    finite_batch = document["completionEvidence"]["finiteBatch"]
    typed_finite_batch = json_format.ParseDict(
        finite_batch,
        manifest_v2_pb2.FiniteBatchCompletionV2(),
    )
    finite_batch["sealReceiptSha256"] = finite_batch_seal_receipt_sha256(typed_finite_batch)


class SharedContractFixturesTest(unittest.TestCase):
    def test_finite_batch_receipt_projection_matches_cross_language_bytes_and_sha256(self) -> None:
        manifest = parse_dataset_manifest_v2_protojson(
            read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        )
        finite_batch = manifest.completion_evidence.finite_batch
        payload = finite_batch_seal_receipt_protojson_bytes(finite_batch)
        expected = (REPO_ROOT / "schemas/fixtures/finite-batch-seal-receipt-v2.protojson").read_bytes()
        expected_sha256 = (
            REPO_ROOT / "schemas/fixtures/finite-batch-seal-receipt-v2.sha256"
        ).read_text(encoding="ascii").strip()

        self.assertEqual(payload, expected)
        self.assertFalse(payload.endswith(b"\n"))
        self.assertNotIn(b"sealReceiptSha256", payload)
        self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)
        self.assertEqual(finite_batch_seal_receipt_sha256(finite_batch), expected_sha256)
        self.assertEqual(finite_batch.seal_receipt_sha256, expected_sha256)

        nonpaged = type(finite_batch)()
        nonpaged.CopyFrom(finite_batch)
        nonpaged.source_kind = manifest_v2_pb2.FINITE_BATCH_SOURCE_KIND_HISTORICAL_NON_PAGED
        nonpaged.input_size_bytes = (1 << 64) - 1
        nonpaged.input_record_count = (1 << 64) - 1
        nonpaged.consumed_record_count = (1 << 64) - 1
        for field in ("page_count", "pages_exhausted", "page_set_sha256"):
            nonpaged.ClearField(field)
        nonpaged_payload = finite_batch_seal_receipt_protojson_bytes(nonpaged)
        nonpaged_expected = (
            REPO_ROOT / "schemas/fixtures/finite-batch-seal-receipt-v2-nonpaged-u64.protojson"
        ).read_bytes()
        nonpaged_expected_sha256 = (
            REPO_ROOT / "schemas/fixtures/finite-batch-seal-receipt-v2-nonpaged-u64.sha256"
        ).read_text(encoding="ascii").strip()
        self.assertEqual(nonpaged_payload, nonpaged_expected)
        self.assertEqual(hashlib.sha256(nonpaged_payload).hexdigest(), nonpaged_expected_sha256)
        self.assertNotIn(b"pageCount", nonpaged_payload)
        self.assertNotIn(b"pagesExhausted", nonpaged_payload)
        self.assertNotIn(b"pageSetSha256", nonpaged_payload)

        invalid = read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        invalid["completionEvidence"]["finiteBatch"]["sealReceiptSha256"] = "e" * 64
        with self.assertRaisesRegex(ValueError, "seal receipt hash"):
            parse_dataset_manifest_v2_protojson(invalid)

        malformed_receipt = type(finite_batch)()
        malformed_receipt.CopyFrom(finite_batch)
        malformed_receipt.page_count = 0
        with self.assertRaisesRegex(ValueError, "paged finite batch receipt"):
            finite_batch_seal_receipt_protojson_bytes(malformed_receipt)

    def test_completion_oneof_hashes_and_bar_v2_reference_match_shared_goldens(self) -> None:
        expected_hashes = read_json_fixture(
            "schemas/fixtures/dataset-completion-evidence-v2-sha256.json"
        )
        cases = (
            ("schemas/fixtures/dataset-manifest-v2.json", "finite_batch"),
            ("schemas/fixtures/dataset-manifest-v2-provider-watermark.json", "provider_watermark"),
            ("schemas/fixtures/dataset-manifest-v2-diagnostic-stream.json", "diagnostic_stream"),
        )
        for path, evidence_case in cases:
            with self.subTest(evidence_case=evidence_case):
                manifest = parse_dataset_manifest_v2_protojson(read_json_fixture(path))
                payload = dataset_completion_evidence_v2_protojson_bytes(manifest)
                self.assertFalse(payload.endswith(b"\n"))
                self.assertEqual(
                    dataset_completion_evidence_v2_sha256(manifest),
                    expected_hashes[evidence_case],
                )

        manifest = parse_dataset_manifest_v2_protojson(
            read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        )
        row = parse_us_equity_trade_bar_v2_protojson(
            read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        )
        validate_bar_v2_completion_evidence_reference(row, manifest)
        validate_us_equity_trade_bar_v2_against_manifest(row, manifest)
        snake_row = parse_us_equity_trade_bar_v2_protojson(
            read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2-snake.json")
        )
        self.assertEqual(row, snake_row)
        validate_us_equity_trade_bar_v2_against_manifest(snake_row, manifest)

        for case in ("provider-watermark", "diagnostic-stream"):
            with self.subTest(completion_case=case):
                case_manifest = parse_dataset_manifest_v2_protojson(
                    read_json_fixture(f"schemas/fixtures/dataset-manifest-v2-{case}.json")
                )
                case_row = parse_us_equity_trade_bar_v2_protojson(
                    read_json_fixture(f"schemas/fixtures/us-equity-trade-bar-v2-{case}.json")
                )
                validate_us_equity_trade_bar_v2_against_manifest(case_row, case_manifest)
        row.completion_evidence_sha256 = "e" * 64
        with self.assertRaisesRegex(ValueError, "does not match"):
            validate_us_equity_trade_bar_v2_against_manifest(row, manifest)

        invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        invalid_row["tradeCount"] = 4
        with self.assertRaisesRegex(ValueError, "uint64 JSON values must be canonical"):
            parse_us_equity_trade_bar_v2_protojson(invalid_row)

        for field, invalid in (
            ("high", "9.00"),
            ("open", "0"),
            ("volume", "-1"),
            ("barEndExclusiveUtc", "2026-10-08T14:31:00.000000001Z"),
        ):
            with self.subTest(field=field):
                invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
                invalid_row[field] = invalid
                with self.assertRaises(ValueError):
                    parse_us_equity_trade_bar_v2_protojson(invalid_row)

        invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        invalid_row["unrecognized"] = True
        with self.assertRaises(Exception):
            parse_us_equity_trade_bar_v2_protojson(invalid_row)

        invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        invalid_row["schema_version"] = 2
        with self.assertRaisesRegex(ValueError, "both camelCase and snake_case"):
            parse_us_equity_trade_bar_v2_protojson(invalid_row)

        invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        invalid_row["nbboInputStatus"] = "x" * 65_537
        with self.assertRaisesRegex(ValueError, "byte limit"):
            parse_us_equity_trade_bar_v2_protojson(invalid_row)

        invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        invalid_row["sourceProvider"] = "another-provider"
        with self.assertRaisesRegex(ValueError, "source fields do not match"):
            validate_us_equity_trade_bar_v2_against_manifest(
                parse_us_equity_trade_bar_v2_protojson(invalid_row), manifest
            )

        invalid_row = read_json_fixture("schemas/fixtures/us-equity-trade-bar-v2.json")
        invalid_row.pop("sourcePagesExhausted")
        with self.assertRaisesRegex(ValueError, "completion fields do not match"):
            validate_us_equity_trade_bar_v2_against_manifest(
                parse_us_equity_trade_bar_v2_protojson(invalid_row), manifest
            )

    def test_dataset_manifest_v2_protojson_is_bounded_and_keeps_completion_evidence_distinct(self) -> None:
        fixture = read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        manifest = parse_dataset_manifest_v2_protojson(fixture)
        self.assertEqual(manifest.schema_version, 2)
        self.assertEqual(manifest.row_count, 1)
        self.assertEqual(manifest.completion_evidence.WhichOneof("evidence"), "finite_batch")
        self.assertEqual(manifest.completion_evidence.finite_batch.completed_at.nanos, 123456789)
        self.assertEqual(MessageToDict(manifest)["completionEvidence"]["finiteBatch"]["completedAt"], "2026-10-08T14:31:02.123456789Z")
        diagnostic = parse_dataset_manifest_v2_protojson(
            read_json_fixture("schemas/fixtures/dataset-manifest-v2-diagnostic-stream.json")
        )
        self.assertEqual(diagnostic.completion_evidence.WhichOneof("evidence"), "diagnostic_stream")
        self.assertEqual(
            diagnostic.completion_evidence.diagnostic_stream.observed_max_source_timestamp.seconds,
            diagnostic.completion_evidence.diagnostic_stream.local_policy_cutoff.seconds + 5,
        )

        invalid = copy.deepcopy(fixture)
        invalid["completionEvidence"]["finiteBatch"]["consumedRecordCount"] = "0"
        with self.assertRaisesRegex(ValueError, "finite batch"):
            parse_dataset_manifest_v2_protojson(invalid)

        invalid = copy.deepcopy(fixture)
        invalid["completionEvidence"]["finiteBatch"]["pagesExhausted"] = False
        with self.assertRaisesRegex(ValueError, "paged finite batch"):
            parse_dataset_manifest_v2_protojson(invalid)

        invalid = copy.deepcopy(fixture)
        invalid["completionEvidence"]["finiteBatch"]["dataCutoffExclusive"] = (
            "2026-10-08T14:30:59.999999999Z"
        )
        refresh_finite_batch_receipt_hash(invalid)
        with self.assertRaisesRegex(ValueError, "cutoff precedes"):
            parse_dataset_manifest_v2_protojson(invalid)

        invalid = copy.deepcopy(fixture)
        invalid["completionEvidence"]["diagnosticStream"] = {
            "sourceInstanceId": "local-session-1",
            "generation": "1",
            "observedLastSequence": "1",
            "localPolicyCutoff": "2026-10-08T14:31:00Z",
            "diagnosticPolicySha256": "d" * 64,
            "diagnosticReceiptSha256": "e" * 64,
        }
        with self.assertRaisesRegex(ValueError, "exactly one"):
            parse_dataset_manifest_v2_protojson(invalid)

        invalid = copy.deepcopy(fixture)
        invalid["row_count"] = "1"
        with self.assertRaisesRegex(ValueError, "both camelCase"):
            parse_dataset_manifest_v2_protojson(invalid)

        invalid = copy.deepcopy(fixture)
        invalid["source"]["accountId"] = "must-not-be-accepted"
        with self.assertRaisesRegex(ValueError, "invalid dataset manifest v2"):
            parse_dataset_manifest_v2_protojson(invalid)

        invalid = copy.deepcopy(fixture)
        invalid["rowCount"] = 1
        with self.assertRaisesRegex(ValueError, "canonical decimal strings"):
            parse_dataset_manifest_v2_protojson(invalid)

        duplicate_key_json = json.dumps(fixture, separators=(",", ":")).replace(
            '"rowCount":"1"', '"rowCount":"1","rowCount":"1"'
        )
        with self.assertRaisesRegex(ValueError, "duplicate JSON object key"):
            parse_dataset_manifest_v2_json(duplicate_key_json)

        raw = copy.deepcopy(fixture)
        raw["source"]["numericEncoding"] = "NUMERIC_ENCODING_RAW_JSON_BYTES"
        raw["sourceTimestampMissingRows"] = "1"
        del raw["timeRange"]
        raw_schema_sha = trusted_parquet_schema_sha256("lqepoch.market_raw_json_frame.v1")
        raw["object"]["parquetSchemaSha256"] = raw_schema_sha
        self.assertEqual(
            parse_dataset_manifest_v2_protojson(raw).source.numeric_encoding,
            6,
        )
        raw["object"]["parquetSchemaSha256"] = "b" * 64
        with self.assertRaisesRegex(ValueError, "raw source encoding"):
            parse_dataset_manifest_v2_protojson(raw)

    def test_dataset_manifest_v2_accepts_complete_snake_case_and_rejects_dual_spellings(self) -> None:
        for path in (
            "schemas/fixtures/dataset-manifest-v2-snake.json",
            "schemas/fixtures/dataset-manifest-v2-provider-watermark-snake.json",
        ):
            with self.subTest(path=path):
                decoded = parse_dataset_manifest_v2_protojson(read_json_fixture(path))
                self.assertEqual(decoded.schema_version, 2)

        fixture = read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        duplicate_cases = []
        candidate = copy.deepcopy(fixture)
        candidate["schema_version"] = candidate["schemaVersion"]
        duplicate_cases.append(candidate)
        candidate = copy.deepcopy(fixture)
        candidate["object"]["object_name"] = candidate["object"]["objectName"]
        duplicate_cases.append(candidate)
        provider = read_json_fixture(
            "schemas/fixtures/dataset-manifest-v2-provider-watermark.json"
        )
        for camel, snake in (
            ("firstSequence", "first_sequence"),
            ("lastSequence", "last_sequence"),
            ("sequenceCount", "sequence_count"),
        ):
            candidate = copy.deepcopy(provider)
            watermark = candidate["completionEvidence"]["providerWatermark"]
            watermark[snake] = watermark[camel]
            duplicate_cases.append(candidate)
        for candidate in duplicate_cases:
            with self.subTest(candidate=candidate), self.assertRaisesRegex(
                ValueError, "both camelCase"
            ):
                parse_dataset_manifest_v2_protojson(candidate)

    def test_dataset_manifest_v2_timestamps_are_lossless_and_reject_leap_seconds(self) -> None:
        timestamp_cases = read_json_fixture("schemas/fixtures/proto-timestamp-v2.json")
        fixture = read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        for case in timestamp_cases["valid"]:
            candidate = copy.deepcopy(fixture)
            candidate["completionEvidence"]["finiteBatch"]["completedAt"] = case["value"]
            refresh_finite_batch_receipt_hash(candidate)
            with self.subTest(case=case["name"]):
                parse_dataset_manifest_v2_protojson(candidate)
        for case in timestamp_cases["invalid"]:
            candidate = copy.deepcopy(fixture)
            candidate["completionEvidence"]["finiteBatch"]["completedAt"] = case["value"]
            with self.subTest(case=case["name"]), self.assertRaises(ValueError):
                parse_dataset_manifest_v2_protojson(candidate)

    def test_dataset_manifest_v2_rejects_numeric_enum_encodings_and_synthetic_watermarks(self) -> None:
        enum_cases = read_json_fixture("schemas/fixtures/dataset-manifest-v2-enum-invalid.json")
        fixture = read_json_fixture("schemas/fixtures/dataset-manifest-v2.json")
        for case in enum_cases["source_numeric_encoding"]:
            candidate = copy.deepcopy(fixture)
            candidate["source"]["numericEncoding"] = case["value"]
            with self.subTest(case=case["name"]), self.assertRaisesRegex(ValueError, "enum fields"):
                parse_dataset_manifest_v2_protojson(candidate)
        for case in enum_cases["finite_source_kind"]:
            candidate = copy.deepcopy(fixture)
            candidate["completionEvidence"]["finiteBatch"]["sourceKind"] = case["value"]
            with self.subTest(case=case["name"]), self.assertRaisesRegex(ValueError, "enum fields"):
                parse_dataset_manifest_v2_protojson(candidate)

        synthetic = read_json_fixture(
            "schemas/fixtures/dataset-manifest-v2-provider-watermark.json"
        )
        synthetic["source"].update(
            provider="synthetic",
            feed="synthetic",
            numericEncoding="NUMERIC_ENCODING_DECIMAL_TOKEN",
        )
        synthetic["completionEvidence"]["providerWatermark"].update(
            provider="synthetic", feed="synthetic"
        )
        with self.assertRaisesRegex(ValueError, "synthetic source"):
            parse_dataset_manifest_v2_protojson(synthetic)

    def test_dataset_manifest_v2_provider_watermark_is_structural_not_authoritative(self) -> None:
        manifest = read_json_fixture("schemas/fixtures/dataset-manifest-v2-provider-watermark.json")
        decoded = parse_dataset_manifest_v2_protojson(manifest)
        self.assertEqual(decoded.completion_evidence.WhichOneof("evidence"), "provider_watermark")
        self.assertEqual(decoded.completion_evidence.provider_watermark.sequence_count, 5)

        manifest["completionEvidence"]["providerWatermark"]["sequenceCount"] = "3"
        with self.assertRaisesRegex(ValueError, "provider watermark"):
            parse_dataset_manifest_v2_protojson(manifest)
        manifest["completionEvidence"]["providerWatermark"]["sequenceCount"] = "4"
        manifest["completionEvidence"]["providerWatermark"]["allowedLatenessNs"] = "60000000001"
        with self.assertRaisesRegex(ValueError, "provider watermark"):
            parse_dataset_manifest_v2_protojson(manifest)

    def test_dataset_manifest_v2_provider_watermark_u64_boundaries(self) -> None:
        manifest = read_json_fixture(
            "schemas/fixtures/dataset-manifest-v2-provider-watermark-u64-boundary.json"
        )
        decoded = parse_dataset_manifest_v2_protojson(manifest)
        watermark = decoded.completion_evidence.provider_watermark
        maximum = (1 << 64) - 1
        self.assertEqual(watermark.generation, maximum)
        self.assertEqual(watermark.first_sequence, 1)
        self.assertEqual(watermark.last_sequence, maximum)
        self.assertEqual(watermark.sequence_count, maximum)
        self.assertEqual(watermark.allowed_lateness_ns, 0)

        for field, invalid in (
            ("generation", "0"),
            ("firstSequence", "0"),
            ("lastSequence", "0"),
        ):
            with self.subTest(field=field):
                candidate = copy.deepcopy(manifest)
                candidate["completionEvidence"]["providerWatermark"][field] = invalid
                with self.assertRaisesRegex(ValueError, "provider watermark"):
                    parse_dataset_manifest_v2_protojson(candidate)

        overflow = copy.deepcopy(manifest)
        evidence = overflow["completionEvidence"]["providerWatermark"]
        evidence["firstSequence"] = "0"
        evidence["lastSequence"] = str(maximum)
        with self.assertRaisesRegex(ValueError, "provider watermark"):
            parse_dataset_manifest_v2_protojson(overflow)

    def test_uint64_fixture_is_strict_and_matches_protojson_roundtrip(self) -> None:
        fixture = read_json_fixture("schemas/fixtures/uint64-json-v1.json")
        for case in fixture["valid"]:
            parsed = parse_uint64_json(case["value"])
            self.assertEqual(str(parsed), case["value"], case["name"])
        for case in fixture["invalid"]:
            with self.subTest(case=case["name"]), self.assertRaises(ValueError):
                parse_uint64_json(case["value"])
        validate_uint64_json_paths({"row_count": "1"}, (("rowCount",),))
        with self.assertRaises(ValueError):
            validate_uint64_json_paths({"row_count": 1}, (("rowCount",),))
        with self.assertRaisesRegex(ValueError, "multiple ProtoJSON spellings"):
            validate_uint64_json_paths(
                {"rowCount": "1", "row_count": "1"}, (("rowCount",),)
            )

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

    def test_raw_byte_encodings_are_bound_only_to_their_registered_frame_schemas(self) -> None:
        from lqepoch_contracts.parquet_schema import trusted_parquet_schema_sha256

        raw_encoding = "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES"
        raw_schema_hash = trusted_parquet_schema_sha256("lqepoch.market_raw_frame.v1")
        self.assertEqual(
            raw_schema_hash,
            "3dfcd21648d7a29e5717150f5470250c5a98db4e65f48f98a34594568fe01df6",
        )
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
        with self.assertRaisesRegex(ValueError, "not normalized market-event"):
            parse_market_event_protojson(event)

        for encoding_case in (
            "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES",
            "NUMERIC_ENCODING_RAW_JSON_BYTES",
        ):
            snake_source = {
                key: value
                for key, value in event["source"].items()
                if key != "numericEncoding"
            }
            snake_source["numeric_encoding"] = encoding_case
            with self.subTest(normalized_alias=encoding_case), self.assertRaisesRegex(
                ValueError, "not normalized market-event"
            ):
                parse_market_event_protojson({**event, "source": snake_source})
            with self.subTest(prediction_alias=encoding_case), self.assertRaisesRegex(
                ValueError, "not identify a normalized prediction source"
            ):
                parse_prediction_envelope_protojson(
                    {"source": snake_source, "forecast": {"sequence": "1"}}
                )

        ambiguous_source = {
            **event["source"],
            "numeric_encoding": "NUMERIC_ENCODING_RAW_JSON_BYTES",
        }
        with self.assertRaisesRegex(ValueError, "both ProtoJSON field spellings"):
            parse_market_event_protojson({**event, "source": ambiguous_source})

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

        raw_json_schema_hash = trusted_parquet_schema_sha256(
            "lqepoch.market_raw_json_frame.v1"
        )
        snake_source = {
            key: value
            for key, value in manifest["source"].items()
            if key != "numericEncoding"
        }
        snake_source["numeric_encoding"] = "NUMERIC_ENCODING_RAW_JSON_BYTES"
        snake_object = {
            key: value
            for key, value in manifest["object"].items()
            if key != "parquetSchemaSha256"
        }
        snake_object["parquet_schema_sha256"] = raw_json_schema_hash
        snake_manifest = {
            **manifest,
            "source": snake_source,
            "object": snake_object,
            "row_count": manifest["rowCount"],
            "source_timestamp_missing_rows": manifest["sourceTimestampMissingRows"],
        }
        del snake_manifest["rowCount"]
        del snake_manifest["sourceTimestampMissingRows"]
        self.assertEqual(
            parse_dataset_manifest_protojson(snake_manifest).source.numeric_encoding, 6
        )
        with self.assertRaisesRegex(ValueError, "registered Parquet schema"):
            parse_dataset_manifest_protojson(
                {
                    **snake_manifest,
                    "object": {
                        **snake_object,
                        "parquet_schema_sha256": raw_schema_hash,
                    },
                }
            )
        snake_messagepack_source = {
            key: value
            for key, value in manifest["source"].items()
            if key != "numericEncoding"
        }
        snake_messagepack_source["numeric_encoding"] = (
            "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES"
        )
        with self.assertRaisesRegex(ValueError, "registered Parquet schema"):
            parse_dataset_manifest_protojson(
                {
                    **snake_manifest,
                    "source": snake_messagepack_source,
                }
            )
        with self.assertRaisesRegex(ValueError, "both ProtoJSON field spellings"):
            parse_dataset_manifest_protojson(
                {
                    **manifest,
                    "source": {
                        **manifest["source"],
                        "numeric_encoding": "NUMERIC_ENCODING_RAW_JSON_BYTES",
                    },
                }
            )
        with self.assertRaises(ValueError):
            parse_dataset_manifest_protojson(
                {**snake_manifest, "row_count": 1}
            )

        for invalid_raw_manifest in (
            {**manifest, "rowCount": "0", "sourceTimestampMissingRows": "0"},
            {key: value for key, value in manifest.items() if key != "rowCount"},
            {**manifest, "sourceTimestampMissingRows": "0"},
            {**manifest, "timeRange": {"startInclusive": "2026-10-08T00:00:00Z"}},
        ):
            with self.subTest(invalid_raw_manifest=invalid_raw_manifest), self.assertRaises(ValueError):
                parse_dataset_manifest_protojson(invalid_raw_manifest)

        wrong_hash = {**manifest, "object": {**manifest["object"], "parquetSchemaSha256": "b" * 64}}
        with self.assertRaisesRegex(ValueError, "registered Parquet schema"):
            parse_dataset_manifest_protojson(wrong_hash)

        wrong_encoding = {
            **manifest,
            "source": {
                **manifest["source"],
                "numericEncoding": "NUMERIC_ENCODING_DECIMAL_TOKEN",
            },
        }
        with self.assertRaisesRegex(ValueError, "raw byte-frame schema fingerprint"):
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

        raw_json_schema_sha256 = trusted_parquet_schema_sha256(
            "lqepoch.market_raw_json_frame.v1"
        )
        raw_json_manifest = {
            **manifest,
            "source": {
                **manifest["source"],
                "numericEncoding": "NUMERIC_ENCODING_RAW_JSON_BYTES",
            },
            "object": {**manifest["object"], "parquetSchemaSha256": raw_json_schema_sha256},
        }
        raw_json_message = parse_dataset_manifest_protojson(raw_json_manifest)
        self.assertEqual(raw_json_message.source.numeric_encoding, 6)

        crossed_schema = {
            **raw_json_manifest,
            "object": {**raw_json_manifest["object"], "parquetSchemaSha256": raw_schema_hash},
        }
        with self.assertRaisesRegex(ValueError, "registered Parquet schema"):
            parse_dataset_manifest_protojson(crossed_schema)

        for raw_json_encoding in ("NUMERIC_ENCODING_RAW_JSON_BYTES", 6):
            raw_json_event = {
                **event,
                "source": {**event["source"], "numericEncoding": raw_json_encoding},
            }
            with self.subTest(raw_json_encoding=raw_json_encoding), self.assertRaisesRegex(
                ValueError, "not normalized market-event"
            ):
                parse_market_event_protojson(raw_json_event)

            raw_json_prediction = {
                **prediction,
                "source": {**prediction["source"], "numericEncoding": raw_json_encoding},
            }
            with self.subTest(raw_json_encoding=raw_json_encoding), self.assertRaisesRegex(
                ValueError, "normalized prediction"
            ):
                parse_prediction_envelope_protojson(raw_json_prediction)


if __name__ == "__main__":
    unittest.main()
