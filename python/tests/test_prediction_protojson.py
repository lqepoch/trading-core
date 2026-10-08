from __future__ import annotations

import copy
import json
import unittest
from pathlib import Path

from google.protobuf import json_format
from lqepoch_contracts.protojson import (
    parse_prediction_envelope_protojson,
    parse_prediction_envelope_protojson_json,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES = REPO_ROOT / "schemas" / "fixtures"
_CROSS_LANGUAGE_INVALID_CASE_NAMES = {
    "numeric uint64",
    "leading-zero uint64",
    "uint64 overflow",
    "numeric quality enum",
    "unknown quality enum",
    "numeric horizon enum",
    "unknown horizon enum",
    "numeric numeric-encoding enum",
    "unknown numeric-encoding enum",
    "unspecified numeric encoding",
    "raw JSON encoding",
    "raw MessagePack encoding",
    "unknown field",
    "numeric timestamp",
    "timestamp over nanosecond precision",
    "leap-second timestamp",
}


def _fixture(name: str) -> object:
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


def _set_path(document: dict[str, object], path: list[str], value: object) -> None:
    current: dict[str, object] = document
    for segment in path[:-1]:
        nested = current[segment]
        if not isinstance(nested, dict):
            raise AssertionError(f"fixture path segment is not an object: {segment}")
        current = nested
    current[path[-1]] = value


class PredictionProtoJsonTest(unittest.TestCase):
    def test_quant_export_fixture_preserves_maximum_uint64_and_nanoseconds(self) -> None:
        base = _fixture("prediction-envelope-v1-quant-export.json")
        parsed = parse_prediction_envelope_protojson(base)
        self.assertEqual(parsed.forecast.sequence, (1 << 64) - 1)

        nanos = parse_prediction_envelope_protojson_json(
            (FIXTURES / "prediction-envelope-v1-nanosecond.json").read_bytes()
        )
        self.assertEqual(nanos.created_at.nanos, 123_456_789)

    def test_partial_wire_message_preserves_maximum_uint64(self) -> None:
        partial = _fixture("prediction-envelope-v1-wire-partial-max.json")
        parsed = parse_prediction_envelope_protojson(partial)
        self.assertEqual(parsed.forecast.sequence, (1 << 64) - 1)
        raw = json.dumps(partial, separators=(",", ":"))
        parsed_raw = parse_prediction_envelope_protojson_json(raw)
        self.assertEqual(parsed_raw.forecast.sequence, (1 << 64) - 1)

    def test_wire_message_preserves_absent_enum_defaults(self) -> None:
        parsed = parse_prediction_envelope_protojson(
            _fixture("prediction-envelope-v1-wire-default-enums.json")
        )
        self.assertEqual(parsed.forecast.forecast_horizon.unit, 0)
        self.assertEqual(parsed.horizon.unit, 0)
        self.assertEqual(parsed.quality.status, 0)
        self.assertEqual(parsed.source.numeric_encoding, 0)

    def test_snake_case_alias_fixture_is_accepted_and_preserves_identity(self) -> None:
        camel = parse_prediction_envelope_protojson(_fixture("prediction-envelope-v1-quant-export.json"))
        snake = parse_prediction_envelope_protojson(_fixture("prediction-envelope-v1-snake.json"))
        self.assertEqual(camel, snake)

    def test_shared_protojson_invalid_cases_are_rejected(self) -> None:
        fixture = _fixture("prediction-envelope-v1-quant-export.json")
        cases = _fixture("prediction-envelope-v1-protojson-cases.json")
        self.assertIsInstance(fixture, dict)
        self.assertIsInstance(cases, dict)
        invalid_mutations = cases["invalid_mutations"]
        self.assertIsInstance(invalid_mutations, list)
        for case in invalid_mutations:
            if case["name"] not in _CROSS_LANGUAGE_INVALID_CASE_NAMES:
                continue
            document = copy.deepcopy(fixture)
            _set_path(document, case["path"], case["value"])
            with self.subTest(case=case["name"]):
                with self.assertRaises((ValueError, json_format.ParseError)):
                    parse_prediction_envelope_protojson(document)

    def test_raw_text_parser_rejects_duplicate_keys_and_conflicting_aliases(self) -> None:
        source = (FIXTURES / "prediction-envelope-v1-quant-export.json").read_text(encoding="utf-8")
        cases = _fixture("prediction-envelope-v1-protojson-cases.json")
        replacements = cases["invalid_text_replacements"]
        for case in replacements:
            invalid = source.replace(case["needle"], case["replacement"], 1)
            self.assertNotEqual(invalid, source)
            with self.subTest(case=case["name"]):
                with self.assertRaises(ValueError):
                    parse_prediction_envelope_protojson_json(invalid)


if __name__ == "__main__":
    unittest.main()
