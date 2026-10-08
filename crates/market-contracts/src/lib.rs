//! Versioned cross-language market data contracts.
//!
//! The legacy module preserves EqoBoard JSON DTOs. New integrations should use the v1
//! envelopes and keep provider evidence separate from contract qualification.

pub mod dataset;
pub mod dataset_v2;
pub mod legacy;
pub mod parquet_schema;
pub mod raw_frame;
pub mod v1;
pub mod wire_u64;

pub use dataset::{
    DATASET_MANIFEST_SCHEMA_VERSION, DatasetCompletionEvidenceV1, DatasetManifestError,
    DatasetManifestV1, DatasetObjectV1, DatasetTimeRangeV1, DatasetTransportV1,
};
pub use dataset_v2::{
    DATASET_MANIFEST_SCHEMA_VERSION_V2, DatasetCompletionEvidenceV2, DatasetManifestV2,
    DatasetManifestV2Error, DatasetObjectV2, DatasetSourceV2, DatasetStorageVerificationV2,
    DatasetTimeRangeV2, DiagnosticStreamCompletionV2, FiniteBatchCompletionV2,
    FiniteBatchSourceKindV2, MAX_DATASET_MANIFEST_V2_JSON_BYTES, MAX_DATASET_MANIFEST_V2_SYMBOLS,
    MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS, NumericEncodingProtoJsonV2, ProtoTimestampV2,
    ProviderWatermarkCompletionV2, parse_dataset_manifest_v2_json,
};
pub use legacy::{
    Bar, ContractError, MarketEvent, OccContract, OptionSnapshot, Right, StockSnapshot, parse_occ,
};
pub use parquet_schema::{
    MARKET_EVENT_PARQUET_SCHEMA_ID, MARKET_EVENT_PARQUET_SCHEMA_V2_ID,
    MARKET_RAW_FRAME_PARQUET_SCHEMA_ID, MARKET_RAW_JSON_FRAME_PARQUET_SCHEMA_ID,
    PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY, PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY,
    ParquetSchemaDescriptorV1, ParquetSchemaError, ParquetSchemaFieldV1, trusted_parquet_schema,
    trusted_parquet_schema_metadata, trusted_schema_fingerprint,
    validate_optional_parquet_schema_metadata,
};
pub use raw_frame::{
    MARKET_RAW_FRAME_SCHEMA_VERSION, MAX_RAW_FRAME_BYTES, MAX_RAW_FRAME_EVENT_COUNT,
    MarketEventParquetRowV2, RawFrameContractError, RawFrameDispositionV1, RawFrameReferenceV2,
    RawFrameStorageRecordV1, RawJsonFrameStorageRecordV1,
};
pub use v1::{
    ConnectionState, ControlEventEnvelopeV1, DecimalString, EntitlementState, EventMetadataV1,
    InstrumentRejection, MAX_CONTROL_INSTRUMENTS, MAX_INSTRUMENT_ID_BYTES, MAX_MARKET_FRAME_BYTES,
    MAX_SOURCE_ID_BYTES, MarketControlEnvelopeV1, MarketControlEventV1, MarketDataSourceV1,
    MarketEventEnvelopeV1, MarketEventV1, MarketWireError, NumericEncodingV1, RejectedInstrumentV1,
    UtcTimestamp, parse_occ_symbol_candidate, validate_market_frame_len,
};
