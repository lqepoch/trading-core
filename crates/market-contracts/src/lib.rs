//! Versioned cross-language market data contracts.
//!
//! The legacy module preserves EqoBoard JSON DTOs. New integrations should use the v1
//! envelopes and keep provider evidence separate from contract qualification.

pub mod dataset;
pub mod legacy;
pub mod parquet_schema;
pub mod v1;
pub mod wire_u64;

pub use dataset::{
    DATASET_MANIFEST_SCHEMA_VERSION, DatasetCompletionEvidenceV1, DatasetManifestError,
    DatasetManifestV1, DatasetObjectV1, DatasetTimeRangeV1, DatasetTransportV1,
};
pub use legacy::{
    Bar, ContractError, MarketEvent, OccContract, OptionSnapshot, Right, StockSnapshot, parse_occ,
};
pub use v1::{
    ConnectionState, ControlEventEnvelopeV1, DecimalString, EntitlementState, EventMetadataV1,
    InstrumentRejection, MAX_CONTROL_INSTRUMENTS, MAX_INSTRUMENT_ID_BYTES, MAX_MARKET_FRAME_BYTES,
    MAX_SOURCE_ID_BYTES, MarketControlEnvelopeV1, MarketControlEventV1, MarketDataSourceV1,
    MarketEventEnvelopeV1, MarketEventV1, MarketWireError, NumericEncodingV1, RejectedInstrumentV1,
    UtcTimestamp, parse_occ_symbol_candidate, validate_market_frame_len,
};
