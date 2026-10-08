from __future__ import annotations

import json
import unittest
from pathlib import Path

from google.protobuf import json_format
from lqepoch.engine.v1 import offline_preview_pb2


REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES = REPO_ROOT / "schemas" / "fixtures"


def _fixture(name: str) -> dict[str, object]:
    value = json.loads((FIXTURES / name).read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"fixture must be a JSON object: {name}")
    return value


class EngineOfflinePreviewProtoJsonTest(unittest.TestCase):
    def test_status_fixture_matches_generated_wire_message(self) -> None:
        value = _fixture("engine-status-response-v1.json")
        message = json_format.ParseDict(value, offline_preview_pb2.EngineStatusResponseV1())
        self.assertEqual(message.api_version, "v1")
        self.assertEqual(message.service, "offline-persist-preview")
        self.assertFalse(message.execution_enabled)
        self.assertFalse(message.mutation_routes_enabled)
        self.assertEqual(message.schema_version, 15)
        self.assertEqual(message.pending_unknown_count, 1)
        self.assertEqual(message.pending_unknown_consumed_risk_count, 1)
        self.assertEqual(message.pending_unknown_unverified_risk_count, 0)
        self.assertEqual(
            {field.json_name for field in message.DESCRIPTOR.fields},
            set(value),
        )

    def test_preview_fixture_matches_generated_wire_message(self) -> None:
        value = _fixture("synthetic-offline-preview-v1.json")
        message = json_format.ParseDict(value, offline_preview_pb2.SyntheticOfflinePreviewV1())
        self.assertEqual(message.preview_kind, "synthetic_session_state")
        self.assertEqual(message.source_provenance, "synthetic_only")
        self.assertFalse(message.execution_enabled)
        self.assertFalse(message.order_mutations_enabled)
        self.assertFalse(message.account_data_loaded)
        self.assertFalse(message.market_data_connected)
        self.assertEqual(message.source_schema_version, 15)
        self.assertEqual(message.disposition, "reconciliation_required")
        self.assertEqual(
            {field.json_name for field in message.DESCRIPTOR.fields},
            set(value),
        )

    def test_generated_messages_reject_unknown_wire_fields(self) -> None:
        status = _fixture("engine-status-response-v1.json")
        status["account_id"] = "synthetic-forbidden"
        with self.assertRaises(json_format.ParseError):
            json_format.ParseDict(status, offline_preview_pb2.EngineStatusResponseV1())


if __name__ == "__main__":
    unittest.main()
