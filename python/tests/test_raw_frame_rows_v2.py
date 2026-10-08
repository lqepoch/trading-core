from __future__ import annotations

import copy
import hashlib
import json
import unittest
from pathlib import Path

from lqepoch_contracts import (
    MARKET_EVENT_V3_SCHEMA_ID,
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
    MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID,
    validate_capture_instance_id_v2,
    validate_event_against_raw_frame_row_v2,
    validate_market_event_row_v3,
    validate_raw_event_chunk_v2,
    validate_raw_frame_row_v2,
)
from lqepoch_contracts.parquet_schema import trusted_parquet_schema_sha256
from lqepoch_contracts.raw_frame import (
    MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES,
    MAX_RAW_FRAME_EVENT_COUNT,
    MAX_RAW_FRAME_SYMBOLS_JSON_BYTES,
)
from lqepoch_contracts.protojson import (
    parse_dataset_manifest_protojson,
    parse_dataset_manifest_v2_protojson,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURE = REPO_ROOT / "schemas/fixtures/raw-frame-capture-v2.json"
TIMESTAMP_FIXTURE = REPO_ROOT / "schemas/fixtures/raw-frame-timestamp-ns-v2.json"
SOURCE_INVALID_FIXTURE = REPO_ROOT / "schemas/fixtures/raw-frame-event-source-invalid-v3.json"
UNICODE_SCALAR_FIXTURE = REPO_ROOT / "schemas/fixtures/raw-frame-unicode-scalar-v2-v3.json"
MAX_FRAME_BYTES = 1024 * 1024


def read_fixture() -> dict[str, object]:
    value = json.loads(FIXTURE.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError("raw-frame fixture root must be an object")
    return value


def materialize_frame(value: dict[str, object]) -> dict[str, object]:
    row = copy.deepcopy(value["row"])
    row["frame_bytes"] = bytes.fromhex(value["frame_bytes_hex"])
    return row


class RawFrameRowsV2Test(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = read_fixture()
        self.messagepack_frames = [
            materialize_frame(frame) for frame in self.fixture["messagepack_frames"]
        ]
        self.messagepack_events = copy.deepcopy(self.fixture["messagepack_events"])
        json_frame = self.fixture["json_frame"]
        self.json_frame = materialize_frame(json_frame)
        self.json_event = copy.deepcopy(self.fixture["json_event"])

    def test_shared_synthetic_fixture_validates_in_both_wire_formats(self) -> None:
        self.assertEqual(self.fixture["messagepack_schema_id"], MARKET_RAW_FRAME_V2_SCHEMA_ID)
        self.assertEqual(self.fixture["json_schema_id"], MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID)
        validate_raw_event_chunk_v2(
            self.messagepack_frames,
            self.messagepack_events,
            MARKET_RAW_FRAME_V2_SCHEMA_ID,
        )
        validate_raw_frame_row_v2(self.json_frame, MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID)
        validate_event_against_raw_frame_row_v2(
            self.json_event,
            self.json_frame,
            MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID,
        )
        validate_market_event_row_v3(self.json_event)

        # The old descriptors remain the exact published schemas; new rows use additive IDs.
        self.assertEqual(
            trusted_parquet_schema_sha256("lqepoch.market_raw_frame.v1"),
            "3dfcd21648d7a29e5717150f5470250c5a98db4e65f48f98a34594568fe01df6",
        )
        self.assertEqual(
            trusted_parquet_schema_sha256("lqepoch.market_event.v2"),
            "45f3fe7b1a2de83cb183a0417dea6cc5559580c77679941581433886ae315133",
        )
        self.assertEqual(
            trusted_parquet_schema_sha256(MARKET_RAW_FRAME_V2_SCHEMA_ID),
            "85ad0ee5a29ce165a48f459fae6d85c58a12a5c4ed387f6844884af1ef0351ea",
        )
        self.assertEqual(
            trusted_parquet_schema_sha256(MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID),
            "bbb0b248a36a820a3587793ff4da41dc3e3e5852a75ce1b05401b45d3245385e",
        )
        self.assertEqual(
            trusted_parquet_schema_sha256(MARKET_EVENT_V3_SCHEMA_ID),
            "2df41366161a20007593a5f1dc908acb9e593c06f3d1d54b68a10a1baf7291fb",
        )

    def test_raw_v2_and_event_v3_timestamps_match_arrow_signed_nanosecond_range(self) -> None:
        boundaries = json.loads(TIMESTAMP_FIXTURE.read_text(encoding="utf-8"))
        for test_case in boundaries["valid"]:
            timestamp = test_case["timestamp_utc"]
            with self.subTest(case=test_case["name"]):
                frame = copy.deepcopy(self.messagepack_frames[0])
                frame["received_timestamp_utc"] = timestamp
                validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

                event = copy.deepcopy(self.messagepack_events[0])
                event["received_timestamp"] = timestamp
                event["source_timestamp"] = timestamp
                validate_market_event_row_v3(event)

        for test_case in boundaries["invalid"]:
            timestamp = test_case["timestamp_utc"]
            with self.subTest(case=test_case["name"]):
                frame = copy.deepcopy(self.messagepack_frames[0])
                frame["received_timestamp_utc"] = timestamp
                with self.assertRaises(ValueError):
                    validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

                invalid_received = copy.deepcopy(self.messagepack_events[0])
                invalid_received["received_timestamp"] = timestamp
                with self.assertRaises(ValueError):
                    validate_market_event_row_v3(invalid_received)

                invalid_source = copy.deepcopy(self.messagepack_events[0])
                invalid_source["source_timestamp"] = timestamp
                with self.assertRaises(ValueError):
                    validate_market_event_row_v3(invalid_source)

    def test_event_v3_source_encoding_matches_the_v1_source_metadata_rules(self) -> None:
        cases = json.loads(SOURCE_INVALID_FIXTURE.read_text(encoding="utf-8"))["invalid"]
        for test_case in cases:
            with self.subTest(case=test_case["name"]):
                event = copy.deepcopy(self.messagepack_events[0])
                event.update(
                    {
                        "provider": test_case["provider"],
                        "feed": test_case["feed"],
                        "numeric_encoding": test_case["numeric_encoding"],
                        "raw_frame_sha256": test_case["raw_frame_sha256"],
                    }
                )
                for field in (
                    "raw_frame_capture_instance_id",
                    "raw_frame_source_generation",
                    "raw_frame_generation",
                    "raw_frame_sequence",
                    "raw_frame_event_ordinal",
                    "raw_frame_event_count",
                ):
                    event[field] = None
                with self.assertRaises(ValueError):
                    validate_market_event_row_v3(event)

    def test_raw_and_event_rows_reject_unpaired_surrogate_code_points(self) -> None:
        fixture = json.loads(UNICODE_SCALAR_FIXTURE.read_text(encoding="utf-8"))
        frame = copy.deepcopy(self.messagepack_frames[0])
        frame["symbols_json"] = fixture["valid_unicode_symbols_json"]
        validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)
        self.assertEqual(
            len(frame["symbols_json"].encode("utf-8")),
            fixture["valid_unicode_symbols_json_utf8_bytes"],
        )

        frame["symbols_json"] = fixture["invalid_symbols_json"]
        with self.assertRaisesRegex(ValueError, "canonical sorted unique"):
            validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        frame["symbols_json"] = fixture["invalid_symbols_json"].replace("\\ud800", "\ud800")
        with self.assertRaisesRegex(ValueError, "valid Unicode scalar"):
            validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        event = copy.deepcopy(self.messagepack_events[0])
        event["symbol"] = json.loads(fixture["invalid_event_symbol_json"])
        with self.assertRaisesRegex(ValueError, "invalid normalized event symbol"):
            validate_market_event_row_v3(event)

        event["source_record_id"] = "record-\ud800"
        with self.assertRaisesRegex(ValueError, "source record identity"):
            validate_market_event_row_v3(event)

    def test_symbols_json_size_bound_precedes_utf8_encoding(self) -> None:
        class EncodeTrackingString(str):
            encode_calls = 0

            def encode(self, *args: object, **kwargs: object) -> bytes:
                self.encode_calls += 1
                return super().encode(*args, **kwargs)

        oversized = EncodeTrackingString("x" * (MAX_RAW_FRAME_SYMBOLS_JSON_BYTES + 1))
        frame = copy.deepcopy(self.messagepack_frames[0])
        frame["symbols_json"] = oversized
        with self.assertRaisesRegex(ValueError, "bounded UTF-8 size"):
            validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)
        self.assertEqual(oversized.encode_calls, 0)

        lone_surrogate = EncodeTrackingString('["\ud800"]')
        frame["symbols_json"] = lone_surrogate
        with self.assertRaisesRegex(ValueError, "valid Unicode scalar"):
            validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)
        self.assertEqual(lone_surrogate.encode_calls, 1)

        byte_oversized_value = '["' + "é" * ((MAX_RAW_FRAME_SYMBOLS_JSON_BYTES - 4) // 2 + 1) + '"]'
        self.assertLessEqual(len(byte_oversized_value), MAX_RAW_FRAME_SYMBOLS_JSON_BYTES)
        self.assertGreater(
            len(byte_oversized_value.encode("utf-8")), MAX_RAW_FRAME_SYMBOLS_JSON_BYTES
        )
        frame["symbols_json"] = byte_oversized_value
        with self.assertRaisesRegex(ValueError, "bounded UTF-8 size"):
            validate_raw_frame_row_v2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

    def test_v3_identity_bounds_precede_utf8_encoding(self) -> None:
        class EncodeTrackingString(str):
            encode_calls = 0

            def encode(self, *args: object, **kwargs: object) -> bytes:
                self.encode_calls += 1
                return super().encode(*args, **kwargs)

        source_record_id = EncodeTrackingString("x" * 129)
        event = copy.deepcopy(self.messagepack_events[0])
        event["source_record_id"] = source_record_id
        with self.assertRaisesRegex(ValueError, "source record identity"):
            validate_market_event_row_v3(event)
        self.assertEqual(source_record_id.encode_calls, 0)

        symbol = EncodeTrackingString("X" * 257)
        event = copy.deepcopy(self.messagepack_events[0])
        event["symbol"] = symbol
        with self.assertRaisesRegex(ValueError, "invalid normalized event symbol"):
            validate_market_event_row_v3(event)
        self.assertEqual(symbol.encode_calls, 0)

        event = copy.deepcopy(self.messagepack_events[0])
        event["source_record_id"] = "é" * 65
        with self.assertRaisesRegex(ValueError, "source record identity"):
            validate_market_event_row_v3(event)

        event = copy.deepcopy(self.messagepack_events[0])
        event["symbol"] = "é" * 129
        with self.assertRaisesRegex(ValueError, "invalid normalized event symbol"):
            validate_market_event_row_v3(event)

    def test_capture_id_and_raw_bytes_are_strictly_bound(self) -> None:
        self.assertEqual(
            validate_capture_instance_id_v2("0123456789ab4def8123456789abcdef"),
            "0123456789ab4def8123456789abcdef",
        )
        for invalid in (
            "0123456789AB4def8123456789abcdef",
            "0123456789ab3def8123456789abcdef",
            "0123456789ab4def7123456789abcdef",
            "01234567-89ab-4def-8123-456789abcdef",
        ):
            with self.subTest(capture_id=invalid), self.assertRaises(ValueError):
                validate_capture_instance_id_v2(invalid)

        bad_id = copy.deepcopy(self.messagepack_frames[0])
        bad_id["capture_instance_id"] = "0123456789ab4def7123456789abcdef"
        with self.assertRaises(ValueError):
            validate_raw_frame_row_v2(bad_id, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        bad_hash = copy.deepcopy(self.messagepack_frames[0])
        bad_hash["frame_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "does not match"):
            validate_raw_frame_row_v2(bad_hash, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        oversized = copy.deepcopy(self.messagepack_frames[0])
        oversized["frame_bytes"] = b"x" * (MAX_FRAME_BYTES + 1)
        oversized["frame_sha256"] = hashlib.sha256(oversized["frame_bytes"]).hexdigest()
        with self.assertRaisesRegex(ValueError, "1 MiB"):
            validate_raw_frame_row_v2(oversized, MARKET_RAW_FRAME_V2_SCHEMA_ID)

    def test_raw_event_join_checks_both_generations_time_and_full_key(self) -> None:
        frame = self.messagepack_frames[0]
        event = self.messagepack_events[0]
        validate_event_against_raw_frame_row_v2(event, frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        for field, value in (
            ("capture_instance_id", "fedcba9876544def8123456789abcdef"),
            ("source_generation", "8"),
            ("canonical_generation", "20"),
        ):
            invalid_frame = copy.deepcopy(frame)
            invalid_frame[field] = value
            with self.subTest(raw_field=field), self.assertRaises(ValueError):
                validate_event_against_raw_frame_row_v2(
                    event, invalid_frame, MARKET_RAW_FRAME_V2_SCHEMA_ID
                )

        invalid_event = copy.deepcopy(event)
        invalid_event["received_timestamp"] = "2026-10-08T14:30:00.000000001Z"
        with self.assertRaisesRegex(ValueError, "exact raw-frame capture key"):
            validate_event_against_raw_frame_row_v2(
                invalid_event, frame, MARKET_RAW_FRAME_V2_SCHEMA_ID
            )

        invalid_frame = copy.deepcopy(frame)
        invalid_frame["received_timestamp_utc"] = "2026-10-08T25:00:00Z"
        with self.assertRaisesRegex(ValueError, "invalid clock time"):
            validate_raw_frame_row_v2(invalid_frame, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        invalid_event = copy.deepcopy(event)
        invalid_event["raw_frame_capture_instance_id"] = None
        with self.assertRaisesRegex(ValueError, "all present or all absent"):
            validate_market_event_row_v3(invalid_event)

    def test_chunk_rejects_gaps_mixed_capture_keys_and_incomplete_projection(self) -> None:
        frames = self.messagepack_frames
        events = self.messagepack_events

        for field, value in (
            ("capture_instance_id", "fedcba9876544def8123456789abcdef"),
            ("source_generation", "8"),
        ):
            invalid_frames = copy.deepcopy(frames)
            invalid_frames[1][field] = value
            invalid_events = copy.deepcopy(events)
            invalid_events[1]["raw_frame_capture_instance_id"] = invalid_frames[1][
                "capture_instance_id"
            ]
            invalid_events[1]["raw_frame_source_generation"] = invalid_frames[1][
                "source_generation"
            ]
            with self.subTest(mixed_field=field), self.assertRaises(ValueError):
                validate_raw_event_chunk_v2(
                    invalid_frames, invalid_events, MARKET_RAW_FRAME_V2_SCHEMA_ID
                )

        gap_frames = copy.deepcopy(frames)
        gap_frames[1]["source_frame_sequence"] = "13"
        with self.assertRaisesRegex(ValueError, "contiguous"):
            validate_raw_event_chunk_v2(gap_frames, events, MARKET_RAW_FRAME_V2_SCHEMA_ID)

        with self.assertRaisesRegex(ValueError, "every expected"):
            validate_raw_event_chunk_v2(frames, events[:1], MARKET_RAW_FRAME_V2_SCHEMA_ID)

        duplicate_ordinal_frame = copy.deepcopy(frames[:1])
        duplicate_ordinal_frame[0]["event_count"] = 2
        duplicate_events = [copy.deepcopy(events[0]), copy.deepcopy(events[0])]
        duplicate_events[1]["sequence"] = "112"
        duplicate_events[0]["raw_frame_event_count"] = 2
        duplicate_events[1]["raw_frame_event_count"] = 2
        with self.assertRaisesRegex(ValueError, "duplicate event ordinals"):
            validate_raw_event_chunk_v2(
                duplicate_ordinal_frame,
                duplicate_events,
                MARKET_RAW_FRAME_V2_SCHEMA_ID,
            )

    def test_chunk_frame_count_and_total_bytes_are_bounded(self) -> None:
        too_many_frames = [copy.deepcopy(self.messagepack_frames[0]) for _ in range(1025)]
        with self.assertRaisesRegex(ValueError, "1024-frame"):
            validate_raw_event_chunk_v2(
                too_many_frames, [], MARKET_RAW_FRAME_V2_SCHEMA_ID
            )

        payload = b"x" * MAX_FRAME_BYTES
        digest = hashlib.sha256(payload).hexdigest()
        large_chunk = []
        for sequence in range(1, 18):
            frame = copy.deepcopy(self.messagepack_frames[0])
            frame.update(
                {
                    "source_frame_sequence": str(sequence),
                    "frame_bytes": payload,
                    "frame_sha256": digest,
                    "event_count": 0,
                    "disposition": "unknown_message",
                    "symbols_json": "[]",
                }
            )
            large_chunk.append(frame)
        with self.assertRaisesRegex(ValueError, "16 MiB"):
            validate_raw_event_chunk_v2(
                large_chunk, [], MARKET_RAW_FRAME_V2_SCHEMA_ID
            )

    def test_chunk_caps_aggregate_symbols_metadata(self) -> None:
        symbols = ["A" + '"' * 250 + f"{index:05x}" for index in range(MAX_RAW_FRAME_EVENT_COUNT)]
        symbols_json = json.dumps(symbols, separators=(",", ":"), ensure_ascii=False)
        self.assertLessEqual(len(symbols_json.encode("utf-8")), MAX_RAW_FRAME_SYMBOLS_JSON_BYTES)
        self.assertGreater(len(symbols_json.encode("utf-8")) * 65, MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES)
        frames = []
        for sequence in range(1, 66):
            frame = copy.deepcopy(self.messagepack_frames[0])
            frame.update(
                {
                    "source_frame_sequence": str(sequence),
                    "event_count": MAX_RAW_FRAME_EVENT_COUNT,
                    "symbols_json": symbols_json,
                }
            )
            frames.append(frame)
        with self.assertRaisesRegex(ValueError, "symbols metadata bound"):
            validate_raw_event_chunk_v2(frames, [], MARKET_RAW_FRAME_V2_SCHEMA_ID)

    def test_dataset_manifests_accept_matching_additive_raw_schema_versions(self) -> None:
        manifest_v1 = {
            "schemaVersion": 1,
            "datasetId": "synthetic-raw-v2-schema",
            "source": {
                "provider": "synthetic",
                "feed": "synthetic",
                "entitlement": "unknown",
                "numericEncoding": "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES",
            },
            "symbols": ["QQQ"],
            "sourceTimestampMissingRows": "1",
            "rowCount": "1",
            "object": {
                "objectName": "raw.parquet",
                "objectId": "local-test:raw-frame-v2",
                "sizeBytes": "10",
                "contentSha256": "a" * 64,
                "parquetSchemaSha256": trusted_parquet_schema_sha256(
                    MARKET_RAW_FRAME_V2_SCHEMA_ID
                ),
                "parquetFooterRows": "1",
                "transport": "local_test",
            },
            "completion": {
                "inputEof": True,
                "readbackSha256": "a" * 64,
                "verifiedBeforePublish": True,
            },
        }
        self.assertEqual(parse_dataset_manifest_protojson(manifest_v1).source.numeric_encoding, 5)
        with self.assertRaisesRegex(ValueError, "registered Parquet schema"):
            parse_dataset_manifest_protojson(
                {
                    **manifest_v1,
                    "object": {
                        **manifest_v1["object"],
                        "parquetSchemaSha256": trusted_parquet_schema_sha256(
                            MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID
                        ),
                    },
                }
            )

        manifest_v2 = json.loads(
            (REPO_ROOT / "schemas/fixtures/dataset-manifest-v2.json").read_text(
                encoding="utf-8"
            )
        )
        manifest_v2["source"]["numericEncoding"] = "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES"
        manifest_v2["object"]["parquetSchemaSha256"] = trusted_parquet_schema_sha256(
            MARKET_RAW_FRAME_V2_SCHEMA_ID
        )
        manifest_v2["sourceTimestampMissingRows"] = manifest_v2["rowCount"]
        manifest_v2.pop("timeRange", None)
        self.assertEqual(
            parse_dataset_manifest_v2_protojson(manifest_v2).source.numeric_encoding,
            5,
        )
        with self.assertRaisesRegex(ValueError, "registered Parquet schema"):
            parse_dataset_manifest_v2_protojson(
                {
                    **manifest_v2,
                    "object": {
                        **manifest_v2["object"],
                        "parquetSchemaSha256": trusted_parquet_schema_sha256(
                            MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID
                        ),
                    },
                }
            )


if __name__ == "__main__":
    unittest.main()
