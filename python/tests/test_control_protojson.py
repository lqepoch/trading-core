from __future__ import annotations

import json
import unittest
from pathlib import Path

from lqepoch_contracts import parse_control_event_protojson, parse_control_event_protojson_json


class ControlProtojsonTests(unittest.TestCase):
    def setUp(self):
        self.document = json.loads((Path(__file__).resolve().parents[2] / "schemas/fixtures/control-ack-v1.json").read_text())

    def test_exact_uint64_roundtrip_and_both_spellings(self):
        for snake in (False, True):
            document = dict(self.document)
            if snake:
                document["schema_version"] = document.pop("schemaVersion")
                document["received_timestamp"] = document.pop("receivedTimestamp")
            message = parse_control_event_protojson_json(json.dumps(document))
            self.assertEqual(message.generation, (1 << 64) - 1)
            self.assertEqual(message.sequence, (1 << 53) + 1)

    def test_numeric_missing_noncanonical_overflow_rejected(self):
        for field in ("generation", "sequence"):
            for value in (0, (1 << 53), "01", "-1", str(1 << 64), None):
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    parse_control_event_protojson({**self.document, field: value})
            missing = dict(self.document)
            del missing[field]
            with self.assertRaises(ValueError):
                parse_control_event_protojson(missing)

    def test_wire_duplicates_aliases_nonobject_and_size_rejected(self):
        raw = json.dumps(self.document, separators=(",", ":"))
        for invalid in (
            raw.replace('"sequence":"9007199254740993"', '"sequence":"0","sequence":"9007199254740993"'),
            raw.replace('"schemaVersion":1', '"schemaVersion":1,"schema_version":1'),
            raw.replace('"sequence":"9007199254740993"', '"sequence":9007199254740993'),
            raw.replace('"generation":"18446744073709551615"', '"generation":18446744073709551615'),
            "[{}]", b"x" * (2 * 1024 * 1024 + 1),
        ):
            with self.subTest(kind=type(invalid)), self.assertRaises(ValueError):
                parse_control_event_protojson_json(invalid)
