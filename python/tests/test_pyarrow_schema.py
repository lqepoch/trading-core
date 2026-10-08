from __future__ import annotations

import importlib.util
import unittest

from lqepoch_contracts.parquet_schema import (
    PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY,
    PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY,
    pyarrow_schema_for_trusted_schema,
    validate_date_iso8601,
    validate_raw_frame_bytes,
    verify_pyarrow_schema,
    trusted_parquet_schema_metadata,
    trusted_parquet_schema_sha256,
)

HAS_PYARROW = importlib.util.find_spec("pyarrow") is not None


@unittest.skipUnless(HAS_PYARROW, "install the pinned arrow optional extra to run Arrow checks")
class PyArrowSchemaTest(unittest.TestCase):
    def test_registry_maps_each_trusted_schema_to_its_physical_arrow_types(self) -> None:
        import pyarrow as pa

        for schema_id in (
            "lqepoch.market_event.v1",
            "lqepoch.us_equity_trade_bar_1m.v1",
            "lqepoch.market_raw_frame.v1",
            "lqepoch.market_raw_json_frame.v1",
            "lqepoch.market_event.v2",
        ):
            with self.subTest(schema_id=schema_id):
                schema = pyarrow_schema_for_trusted_schema(schema_id)
                verify_pyarrow_schema(schema, schema_id)

        raw_schema = pyarrow_schema_for_trusted_schema("lqepoch.market_raw_frame.v1")
        self.assertEqual(raw_schema.field("frame_bytes").type, pa.binary())
        self.assertEqual(
            raw_schema.field("received_timestamp_utc").type,
            pa.timestamp("ns", tz="UTC"),
        )

        raw_json_schema = pyarrow_schema_for_trusted_schema("lqepoch.market_raw_json_frame.v1")
        self.assertEqual(raw_json_schema.field("frame_bytes").type, pa.binary())

        bar_schema = pyarrow_schema_for_trusted_schema("lqepoch.us_equity_trade_bar_1m.v1")
        self.assertEqual(bar_schema.field("trade_date").type, pa.string())

    def test_arrow_schema_rejects_date32_for_the_utf8_trade_date_contract(self) -> None:
        import pyarrow as pa

        trusted = pyarrow_schema_for_trusted_schema("lqepoch.us_equity_trade_bar_1m.v1")
        fields = list(trusted)
        index = trusted.get_field_index("trade_date")
        fields[index] = pa.field("trade_date", pa.date32(), nullable=False)
        date32_schema = pa.schema(fields)
        with self.assertRaisesRegex(ValueError, "trade_date.*date_iso8601"):
            verify_pyarrow_schema(date32_schema, "lqepoch.us_equity_trade_bar_1m.v1")

    def test_utf8_dates_and_raw_bytes_obey_row_value_bounds(self) -> None:
        self.assertEqual(validate_date_iso8601("2024-02-29"), "2024-02-29")
        for invalid in ("2023-02-29", "2024-2-29", "2024-02-29T00:00:00Z", 20240229):
            with self.subTest(value=invalid), self.assertRaises(ValueError):
                validate_date_iso8601(invalid)

        self.assertEqual(validate_raw_frame_bytes(b"\x91\xa4TEST"), b"\x91\xa4TEST")
        with self.assertRaises(ValueError):
            validate_raw_frame_bytes(bytearray(b"not-bytes"))
        with self.assertRaises(ValueError):
            validate_raw_frame_bytes(b"x" * (1024 * 1024 + 1))

    def test_optional_arrow_metadata_allows_old_files_and_validates_present_registry_pair(self) -> None:
        raw_schema_id = "lqepoch.market_raw_frame.v1"
        schema = pyarrow_schema_for_trusted_schema(raw_schema_id)
        digest = trusted_parquet_schema_sha256(raw_schema_id)
        verify_pyarrow_schema(schema, raw_schema_id)
        verify_pyarrow_schema(schema.with_metadata({b"writer": b"legacy"}), raw_schema_id)

        metadata = trusted_parquet_schema_metadata(raw_schema_id)
        self.assertEqual(set(metadata), {
            PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY.encode("ascii"),
            PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.encode("ascii"),
        })
        self.assertEqual(
            metadata[PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.encode("ascii")].decode("ascii"),
            digest,
        )
        verify_pyarrow_schema(schema.with_metadata(metadata), raw_schema_id)
        self.assertEqual(trusted_parquet_schema_sha256(raw_schema_id), digest)

        descriptor_key = PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY.encode("ascii")
        fingerprint_key = PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.encode("ascii")
        with self.assertRaisesRegex(ValueError, "both registered"):
            verify_pyarrow_schema(schema.with_metadata({descriptor_key: metadata[descriptor_key]}), raw_schema_id)
        wrong_fingerprint = {**metadata, fingerprint_key: b"0" * 64}
        with self.assertRaisesRegex(ValueError, "differs from the registered"):
            verify_pyarrow_schema(schema.with_metadata(wrong_fingerprint), raw_schema_id)
        wrong_descriptor = {**metadata, descriptor_key: b"{}"}
        with self.assertRaisesRegex(ValueError, "differs from the registered"):
            verify_pyarrow_schema(schema.with_metadata(wrong_descriptor), raw_schema_id)


if __name__ == "__main__":
    unittest.main()
