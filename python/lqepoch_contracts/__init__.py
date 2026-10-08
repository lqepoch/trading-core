"""Shared Python validation and generated ProtoJSON consumers for trading-core."""

from .parquet_schema import (
    PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY,
    PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY,
    load_trusted_parquet_schema_registry,
    trusted_parquet_schema_descriptor,
    trusted_parquet_schema_metadata,
    trusted_parquet_schema_sha256,
    validate_optional_parquet_schema_metadata,
)

__all__ = [
    "load_trusted_parquet_schema_registry",
    "PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY",
    "PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY",
    "trusted_parquet_schema_descriptor",
    "trusted_parquet_schema_metadata",
    "trusted_parquet_schema_sha256",
    "validate_optional_parquet_schema_metadata",
]
