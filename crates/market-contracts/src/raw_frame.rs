//! Byte-exact market-data frame storage and event-correlation contracts.
//!
//! These types are in-memory validation helpers for Parquet rows. They intentionally do not
//! implement `Serialize`: raw provider bytes must not be copied into the public JSON/Proto event
//! envelopes. Adapters must exclude authentication and subscription frames before constructing a
//! [`RawFrameStorageRecordV1`].

use crate::{
    EntitlementState, MarketDataSourceV1, MarketEventEnvelopeV1, MarketEventV1, NumericEncodingV1,
    UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use thiserror::Error;

/// Raw-frame row schema version.
pub const MARKET_RAW_FRAME_SCHEMA_VERSION: u32 = 1;
/// Maximum uncompressed payload size for one received provider frame.
pub const MAX_RAW_FRAME_BYTES: usize = 1024 * 1024;
/// Maximum normalized market events expected from one raw frame.
pub const MAX_RAW_FRAME_EVENT_COUNT: u32 = 512;
/// Maximum distinct normalized symbols represented by one raw frame.
pub const MAX_RAW_FRAME_SYMBOLS: usize = MAX_RAW_FRAME_EVENT_COUNT as usize;
/// Worst-case compact JSON size for 512 identifiers of 256 bytes, escaped quotes/backslashes,
/// separators, and array brackets. The cap is checked before JSON parsing allocates the vector.
pub const MAX_RAW_FRAME_SYMBOLS_JSON_BYTES: usize =
    MAX_RAW_FRAME_SYMBOLS * (2 * crate::v1::MAX_INSTRUMENT_ID_BYTES + 3) + 1;

/// Known disposition for a captured inbound market-data frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawFrameDispositionV1 {
    /// A market-data frame with at least one normalizable quote or trade expected.
    MarketData,
    /// A control or heartbeat frame that does not produce market-event rows.
    Control,
    /// A frame type the decoder does not recognize.
    UnknownMessage,
    /// A frame that could not be decoded completely.
    MalformedMessage,
    /// A provider error frame; expected market-event count may remain non-zero.
    ProviderError,
}

impl RawFrameDispositionV1 {
    /// Canonical Parquet UTF-8 spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MarketData => "market_data",
            Self::Control => "control",
            Self::UnknownMessage => "unknown_message",
            Self::MalformedMessage => "malformed_message",
            Self::ProviderError => "provider_error",
        }
    }
}

/// One exact inbound MessagePack frame row in `lqepoch.market_raw_frame.v1`.
#[derive(Clone, Eq, PartialEq)]
pub struct RawFrameStorageRecordV1 {
    /// Raw-frame schema version, fixed at 1.
    pub schema_version: u32,
    /// Provider code shared with projected events.
    pub provider: String,
    /// Feed code shared with projected events.
    pub feed: String,
    /// Provider entitlement evidence; unknown remains explicit.
    pub entitlement: EntitlementState,
    /// Optional uniform numeric projection encoding observed within this raw frame.
    pub source_numeric_encoding: Option<NumericEncodingV1>,
    /// Canonical connection generation shared with projected events.
    pub generation: u64,
    /// Monotonic raw-frame sequence within this generation.
    pub frame_sequence: u64,
    /// Local UTC receive timestamp for the exact bytes.
    pub received_timestamp_utc: UtcTimestamp,
    /// SHA-256 of `frame_bytes`, recomputed during validation.
    pub frame_sha256: String,
    /// Exact received MessagePack application-frame bytes.
    pub frame_bytes: Vec<u8>,
    /// Expected number of normalized quote/trade records in the raw frame, not delivered count.
    pub event_count: u32,
    /// Decoder disposition retained even when no normalized event was produced.
    pub disposition: RawFrameDispositionV1,
    /// Compact JSON array of the frame's lexically sorted, unique normalizable symbols.
    pub symbols_json: String,
}

impl fmt::Debug for RawFrameStorageRecordV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawFrameStorageRecordV1")
            .field("schema_version", &self.schema_version)
            .field("provider", &self.provider)
            .field("feed", &self.feed)
            .field("entitlement", &self.entitlement)
            .field("source_numeric_encoding", &self.source_numeric_encoding)
            .field("generation", &self.generation)
            .field("frame_sequence", &self.frame_sequence)
            .field("received_timestamp_utc", &self.received_timestamp_utc)
            .field("frame_sha256", &self.frame_sha256)
            .field("frame_bytes_len", &self.frame_bytes.len())
            .field("event_count", &self.event_count)
            .field("disposition", &self.disposition)
            .field("symbols_json_bytes_len", &self.symbols_json.len())
            .finish()
    }
}

impl RawFrameStorageRecordV1 {
    /// Validate the frame identity, content digest, projection metadata, and symbol list.
    pub fn validate(&self) -> Result<(), RawFrameContractError> {
        validate_raw_frame(self.view(), NumericEncodingV1::RawMessagePackBytes)
    }

    /// Return the decoded symbol list after validating its canonical representation.
    pub fn symbols(&self) -> Result<Vec<String>, RawFrameContractError> {
        parse_canonical_symbols_json(&self.symbols_json)
    }

    fn view(&self) -> RawFrameView<'_> {
        RawFrameView {
            schema_version: self.schema_version,
            provider: &self.provider,
            feed: &self.feed,
            entitlement: self.entitlement,
            source_numeric_encoding: self.source_numeric_encoding,
            generation: self.generation,
            frame_sequence: self.frame_sequence,
            received_timestamp_utc: &self.received_timestamp_utc,
            frame_sha256: &self.frame_sha256,
            frame_bytes: &self.frame_bytes,
            event_count: self.event_count,
            disposition: self.disposition,
            symbols_json: &self.symbols_json,
        }
    }
}

/// One exact inbound JSON application frame row in `lqepoch.market_raw_json_frame.v1`.
///
/// It deliberately stores raw bytes without requiring the payload to be valid JSON: malformed
/// application frames must remain available to bounded quarantine tooling.
#[derive(Clone, Eq, PartialEq)]
pub struct RawJsonFrameStorageRecordV1 {
    /// Raw-frame schema version, fixed at 1.
    pub schema_version: u32,
    /// Provider code shared with projected events.
    pub provider: String,
    /// Feed code shared with projected events.
    pub feed: String,
    /// Provider entitlement evidence; unknown remains explicit.
    pub entitlement: EntitlementState,
    /// Optional uniform numeric projection encoding observed within this raw frame.
    pub source_numeric_encoding: Option<NumericEncodingV1>,
    /// Canonical connection generation shared with projected events.
    pub generation: u64,
    /// Monotonic raw-frame sequence within this generation.
    pub frame_sequence: u64,
    /// Local UTC receive timestamp for the exact bytes.
    pub received_timestamp_utc: UtcTimestamp,
    /// SHA-256 of `frame_bytes`, recomputed during validation.
    pub frame_sha256: String,
    /// Exact received JSON application-frame bytes.
    pub frame_bytes: Vec<u8>,
    /// Expected number of normalized quote/trade records in the raw frame, not delivered count.
    pub event_count: u32,
    /// Decoder disposition retained even when no normalized event was produced.
    pub disposition: RawFrameDispositionV1,
    /// Compact JSON array of the frame's lexically sorted, unique normalizable symbols.
    pub symbols_json: String,
}

impl fmt::Debug for RawJsonFrameStorageRecordV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawJsonFrameStorageRecordV1")
            .field("schema_version", &self.schema_version)
            .field("provider", &self.provider)
            .field("feed", &self.feed)
            .field("entitlement", &self.entitlement)
            .field("source_numeric_encoding", &self.source_numeric_encoding)
            .field("generation", &self.generation)
            .field("frame_sequence", &self.frame_sequence)
            .field("received_timestamp_utc", &self.received_timestamp_utc)
            .field("frame_sha256", &self.frame_sha256)
            .field("frame_bytes_len", &self.frame_bytes.len())
            .field("event_count", &self.event_count)
            .field("disposition", &self.disposition)
            .field("symbols_json_bytes_len", &self.symbols_json.len())
            .finish()
    }
}

impl RawJsonFrameStorageRecordV1 {
    /// Validate the frame identity, content digest, projection metadata, and symbol list.
    pub fn validate(&self) -> Result<(), RawFrameContractError> {
        validate_raw_frame(self.view(), NumericEncodingV1::RawJsonBytes)
    }

    /// Return the decoded symbol list after validating its canonical representation.
    pub fn symbols(&self) -> Result<Vec<String>, RawFrameContractError> {
        parse_canonical_symbols_json(&self.symbols_json)
    }

    fn view(&self) -> RawFrameView<'_> {
        RawFrameView {
            schema_version: self.schema_version,
            provider: &self.provider,
            feed: &self.feed,
            entitlement: self.entitlement,
            source_numeric_encoding: self.source_numeric_encoding,
            generation: self.generation,
            frame_sequence: self.frame_sequence,
            received_timestamp_utc: &self.received_timestamp_utc,
            frame_sha256: &self.frame_sha256,
            frame_bytes: &self.frame_bytes,
            event_count: self.event_count,
            disposition: self.disposition,
            symbols_json: &self.symbols_json,
        }
    }
}

#[derive(Clone, Copy)]
struct RawFrameView<'a> {
    schema_version: u32,
    provider: &'a str,
    feed: &'a str,
    entitlement: EntitlementState,
    source_numeric_encoding: Option<NumericEncodingV1>,
    generation: u64,
    frame_sequence: u64,
    received_timestamp_utc: &'a UtcTimestamp,
    frame_sha256: &'a str,
    frame_bytes: &'a [u8],
    event_count: u32,
    disposition: RawFrameDispositionV1,
    symbols_json: &'a str,
}

fn validate_raw_frame(
    frame: RawFrameView<'_>,
    dataset_encoding: NumericEncodingV1,
) -> Result<(), RawFrameContractError> {
    if frame.schema_version != MARKET_RAW_FRAME_SCHEMA_VERSION {
        return Err(RawFrameContractError::UnsupportedVersion);
    }
    if frame.generation == 0 || frame.frame_sequence == 0 {
        return Err(RawFrameContractError::InvalidSequence);
    }
    if frame.frame_bytes.len() > MAX_RAW_FRAME_BYTES {
        return Err(RawFrameContractError::FrameTooLarge);
    }
    if frame.event_count > MAX_RAW_FRAME_EVENT_COUNT {
        return Err(RawFrameContractError::TooManyEvents);
    }
    if !crate::parquet_schema::validate_sha256_hex(frame.frame_sha256) {
        return Err(RawFrameContractError::InvalidHash);
    }
    if lower_hex(&Sha256::digest(frame.frame_bytes)) != frame.frame_sha256 {
        return Err(RawFrameContractError::HashMismatch);
    }
    if frame.source_numeric_encoding.is_some_and(|encoding| {
        matches!(encoding, NumericEncodingV1::Unspecified) || encoding.is_raw_bytes()
    }) {
        return Err(RawFrameContractError::InvalidProjectionEncoding);
    }

    let source = MarketDataSourceV1 {
        provider: frame.provider.to_owned(),
        feed: frame.feed.to_owned(),
        entitlement: frame.entitlement,
        numeric_encoding: dataset_encoding,
        source_record_id: None,
    };
    source
        .validate_for_dataset_manifest()
        .map_err(|_| RawFrameContractError::InvalidSource)?;

    let symbols = parse_canonical_symbols_json(frame.symbols_json)?;
    match frame.disposition {
        RawFrameDispositionV1::MarketData if frame.event_count == 0 || symbols.is_empty() => {
            return Err(RawFrameContractError::InvalidDisposition);
        }
        RawFrameDispositionV1::Control if frame.event_count != 0 || !symbols.is_empty() => {
            return Err(RawFrameContractError::InvalidDisposition);
        }
        _ => {}
    }
    if frame.event_count > 0 && (symbols.is_empty() || symbols.len() > frame.event_count as usize) {
        return Err(RawFrameContractError::InvalidDisposition);
    }
    Ok(())
}

/// Correlation fields appended by the `lqepoch.market_event.v2` storage schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawFrameReferenceV2 {
    /// Must equal the normalized event's generation.
    pub raw_frame_generation: u64,
    /// Raw-frame sequence within the same generation.
    pub raw_frame_sequence: u64,
    /// One-based event ordinal within the source frame's expected normalized event sequence.
    pub raw_frame_event_ordinal: u32,
    /// Expected normalized quote/trade event count for the source frame.
    pub raw_frame_event_count: u32,
}

/// Event DTO plus nullable v2 raw-frame correlation tuple.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketEventParquetRowV2 {
    /// Existing, unchanged v1 market event envelope.
    pub event: MarketEventEnvelopeV1,
    /// All four v2 storage columns are represented together, so partial tuples cannot be built.
    pub raw_frame_reference: Option<RawFrameReferenceV2>,
}

impl MarketEventParquetRowV2 {
    /// Validate the event and any correlation columns without claiming the raw object was read.
    pub fn validate(&self) -> Result<(), RawFrameContractError> {
        self.event
            .validate()
            .map_err(|_| RawFrameContractError::InvalidEvent)?;
        if let Some(reference) = self.raw_frame_reference
            && (reference.raw_frame_generation == 0
                || reference.raw_frame_sequence == 0
                || reference.raw_frame_generation != self.event.metadata.generation
                || reference.raw_frame_event_count == 0
                || reference.raw_frame_event_count > MAX_RAW_FRAME_EVENT_COUNT
                || reference.raw_frame_event_ordinal == 0
                || reference.raw_frame_event_ordinal > reference.raw_frame_event_count
                || self.event.metadata.raw_frame_sha256.is_none())
        {
            return Err(RawFrameContractError::InvalidReference);
        }
        Ok(())
    }

    /// Cross-check this event against the exact frame row it claims to derive from.
    pub fn validate_against_frame(
        &self,
        frame: &RawFrameStorageRecordV1,
    ) -> Result<(), RawFrameContractError> {
        self.validate_against_raw_frame(frame.view(), NumericEncodingV1::RawMessagePackBytes)
    }

    /// Cross-check this event against the exact JSON frame row it claims to derive from.
    pub fn validate_against_json_frame(
        &self,
        frame: &RawJsonFrameStorageRecordV1,
    ) -> Result<(), RawFrameContractError> {
        self.validate_against_raw_frame(frame.view(), NumericEncodingV1::RawJsonBytes)
    }

    fn validate_against_raw_frame(
        &self,
        frame: RawFrameView<'_>,
        dataset_encoding: NumericEncodingV1,
    ) -> Result<(), RawFrameContractError> {
        self.validate()?;
        validate_raw_frame(frame, dataset_encoding)?;
        let reference = self
            .raw_frame_reference
            .ok_or(RawFrameContractError::MissingReference)?;
        if reference.raw_frame_generation != frame.generation
            || reference.raw_frame_sequence != frame.frame_sequence
            || reference.raw_frame_event_count != frame.event_count
            || self.event.metadata.raw_frame_sha256.as_deref() != Some(frame.frame_sha256)
            || self.event.metadata.source.provider != frame.provider
            || self.event.metadata.source.feed != frame.feed
            || self.event.metadata.source.entitlement != frame.entitlement
            || self.event.metadata.received_timestamp != *frame.received_timestamp_utc
            || frame
                .source_numeric_encoding
                .is_some_and(|encoding| encoding != self.event.metadata.source.numeric_encoding)
            || !parse_canonical_symbols_json(frame.symbols_json)?
                .iter()
                .any(|symbol| symbol == event_symbol(&self.event.event))
        {
            return Err(RawFrameContractError::ReferenceMismatch);
        }
        Ok(())
    }
}

/// Stable validation failures for raw-frame storage and v2 event correlation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum RawFrameContractError {
    #[error("unsupported raw-frame schema version")]
    UnsupportedVersion,
    #[error("invalid provider/feed identity")]
    InvalidSource,
    #[error("raw-frame generation and sequence must be positive")]
    InvalidSequence,
    #[error("raw-frame bytes exceed the 1 MiB bound")]
    FrameTooLarge,
    #[error("raw-frame event count exceeds the per-frame bound")]
    TooManyEvents,
    #[error("raw-frame SHA-256 is invalid")]
    InvalidHash,
    #[error("raw-frame SHA-256 does not match the exact bytes")]
    HashMismatch,
    #[error("numeric projection encoding is invalid for a raw-frame row")]
    InvalidProjectionEncoding,
    #[error("raw-frame symbols are not a canonical sorted unique JSON array")]
    InvalidSymbols,
    #[error("raw-frame disposition conflicts with expected events or symbols")]
    InvalidDisposition,
    #[error("normalized event is invalid")]
    InvalidEvent,
    #[error("event raw-frame correlation is absent or inconsistent")]
    InvalidReference,
    #[error("event row has no raw-frame correlation tuple")]
    MissingReference,
    #[error("event and raw-frame record do not identify the same source data")]
    ReferenceMismatch,
}

fn parse_canonical_symbols_json(value: &str) -> Result<Vec<String>, RawFrameContractError> {
    if value.len() > MAX_RAW_FRAME_SYMBOLS_JSON_BYTES {
        return Err(RawFrameContractError::InvalidSymbols);
    }
    let symbols: Vec<String> =
        serde_json::from_str(value).map_err(|_| RawFrameContractError::InvalidSymbols)?;
    if symbols.len() > MAX_RAW_FRAME_SYMBOLS {
        return Err(RawFrameContractError::InvalidSymbols);
    }
    let canonical =
        serde_json::to_string(&symbols).map_err(|_| RawFrameContractError::InvalidSymbols)?;
    if canonical != value
        || symbols
            .iter()
            .any(|symbol| !crate::v1::valid_identifier(symbol, crate::v1::MAX_INSTRUMENT_ID_BYTES))
        || symbols.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(RawFrameContractError::InvalidSymbols);
    }
    Ok(symbols)
}

fn event_symbol(event: &MarketEventV1) -> &str {
    match event {
        MarketEventV1::StockQuote { symbol, .. }
        | MarketEventV1::StockTrade { symbol, .. }
        | MarketEventV1::OptionQuote { symbol, .. }
        | MarketEventV1::OptionTrade { symbol, .. } => symbol,
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RAW_FRAME_BYTES, MAX_RAW_FRAME_EVENT_COUNT, MAX_RAW_FRAME_SYMBOLS,
        MAX_RAW_FRAME_SYMBOLS_JSON_BYTES, MarketEventParquetRowV2, RawFrameContractError,
        RawFrameDispositionV1, RawFrameReferenceV2, RawFrameStorageRecordV1,
        RawJsonFrameStorageRecordV1,
    };
    use crate::{
        DecimalString, EntitlementState, EventMetadataV1, MarketDataSourceV1,
        MarketEventEnvelopeV1, MarketEventV1, NumericEncodingV1, UtcTimestamp,
    };
    use sha2::{Digest, Sha256};

    fn sha256(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    // Valid synthetic MessagePack array ["trade", "1.25"], not captured provider data.
    fn synthetic_frame_bytes() -> Vec<u8> {
        vec![
            0x92, 0xa5, b't', b'r', b'a', b'd', b'e', 0xa4, b'1', b'.', b'2', b'5',
        ]
    }

    fn frame(bytes: Vec<u8>) -> RawFrameStorageRecordV1 {
        RawFrameStorageRecordV1 {
            schema_version: 1,
            provider: "synthetic".to_owned(),
            feed: "synthetic".to_owned(),
            entitlement: EntitlementState::Unknown,
            source_numeric_encoding: Some(NumericEncodingV1::DecimalToken),
            generation: 17,
            frame_sequence: 23,
            received_timestamp_utc: UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap(),
            frame_sha256: sha256(&bytes),
            frame_bytes: bytes,
            event_count: 1,
            disposition: RawFrameDispositionV1::MarketData,
            symbols_json: r#"["QQQ   261016C00600000"]"#.to_owned(),
        }
    }

    fn json_frame(bytes: Vec<u8>) -> RawJsonFrameStorageRecordV1 {
        RawJsonFrameStorageRecordV1 {
            schema_version: 1,
            provider: "synthetic".to_owned(),
            feed: "synthetic".to_owned(),
            entitlement: EntitlementState::Unknown,
            source_numeric_encoding: Some(NumericEncodingV1::DecimalToken),
            generation: 17,
            frame_sequence: 24,
            received_timestamp_utc: UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap(),
            frame_sha256: sha256(&bytes),
            frame_bytes: bytes,
            event_count: 1,
            disposition: RawFrameDispositionV1::MarketData,
            symbols_json: r#"["QQQ   261016C00600000"]"#.to_owned(),
        }
    }

    fn event() -> MarketEventParquetRowV2 {
        MarketEventParquetRowV2 {
            event: MarketEventEnvelopeV1 {
                metadata: EventMetadataV1 {
                    schema_version: 1,
                    source: MarketDataSourceV1::new(
                        "synthetic",
                        "synthetic",
                        EntitlementState::Unknown,
                        NumericEncodingV1::DecimalToken,
                        None,
                    )
                    .unwrap(),
                    generation: 17,
                    sequence: 88,
                    raw_frame_sha256: Some(sha256(&synthetic_frame_bytes())),
                    source_timestamp: Some(UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap()),
                    received_timestamp: UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap(),
                },
                event: MarketEventV1::OptionTrade {
                    symbol: "QQQ   261016C00600000".to_owned(),
                    price: DecimalString::new("1.25").unwrap(),
                    size: DecimalString::new("1").unwrap(),
                },
            },
            raw_frame_reference: Some(RawFrameReferenceV2 {
                raw_frame_generation: 17,
                raw_frame_sequence: 23,
                raw_frame_event_ordinal: 1,
                raw_frame_event_count: 1,
            }),
        }
    }

    #[test]
    fn exact_synthetic_frame_and_event_reference_validate_together() {
        let frame = frame(synthetic_frame_bytes());
        let event = event();
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(event.validate_against_frame(&frame), Ok(()));
        assert_eq!(frame.disposition.as_str(), "market_data");
    }

    #[test]
    fn raw_json_frame_uses_shared_bounds_hash_symbols_and_event_correlation() {
        let bytes = br#"{"T":"t","S":"QQQ","p":1.25}"#.to_vec();
        let mut raw = json_frame(bytes);
        assert_eq!(raw.validate(), Ok(()));
        assert_eq!(
            raw.symbols().unwrap(),
            vec!["QQQ   261016C00600000".to_owned()]
        );

        let mut projected = event();
        projected.event.metadata.raw_frame_sha256 = Some(raw.frame_sha256.clone());
        projected
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_sequence = raw.frame_sequence;
        projected
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_generation = raw.generation;
        assert_eq!(projected.validate_against_json_frame(&raw), Ok(()));

        raw.frame_sha256 = "0".repeat(64);
        assert_eq!(raw.validate(), Err(RawFrameContractError::HashMismatch));

        let mut invalid_projection = json_frame(b"{}".to_vec());
        invalid_projection.source_numeric_encoding = Some(NumericEncodingV1::RawJsonBytes);
        assert_eq!(
            invalid_projection.validate(),
            Err(RawFrameContractError::InvalidProjectionEncoding)
        );

        let mut malformed = json_frame(b"{not-json".to_vec());
        malformed.event_count = 0;
        malformed.disposition = RawFrameDispositionV1::MalformedMessage;
        malformed.symbols_json = "[]".to_owned();
        assert_eq!(malformed.validate(), Ok(()));

        let mut oversized = json_frame(vec![0; MAX_RAW_FRAME_BYTES + 1]);
        oversized.event_count = 0;
        oversized.disposition = RawFrameDispositionV1::MalformedMessage;
        oversized.symbols_json = "[]".to_owned();
        assert_eq!(
            oversized.validate(),
            Err(RawFrameContractError::FrameTooLarge)
        );
    }

    #[test]
    fn raw_json_debug_does_not_reveal_payload_or_symbols() {
        let payload = br#"{"symbol":"PRIVATE_SYMBOL_VALUE","token":"PRIVATE_TOKEN"}"#.to_vec();
        let mut value = json_frame(payload.clone());
        value.symbols_json = r#"["PRIVATE_SYMBOL_VALUE"]"#.to_owned();
        let debug = format!("{value:?}");
        assert!(!debug.contains("PRIVATE_SYMBOL_VALUE"));
        assert!(!debug.contains("PRIVATE_TOKEN"));
        assert!(!debug.contains(&format!("{:?}", payload)));
        assert!(debug.contains("frame_bytes_len"));
    }

    #[test]
    fn raw_bytes_are_hashed_and_bounded_before_being_accepted() {
        let mut value = frame(synthetic_frame_bytes());
        value.frame_sha256 = "0".repeat(64);
        assert_eq!(value.validate(), Err(RawFrameContractError::HashMismatch));

        let oversized = frame(vec![0; MAX_RAW_FRAME_BYTES + 1]);
        assert_eq!(
            oversized.validate(),
            Err(RawFrameContractError::FrameTooLarge)
        );
    }

    #[test]
    fn raw_frame_rows_reject_invalid_counts_encodings_and_noncanonical_symbols() {
        let mut too_many = frame(synthetic_frame_bytes());
        too_many.event_count = MAX_RAW_FRAME_EVENT_COUNT + 1;
        assert_eq!(
            too_many.validate(),
            Err(RawFrameContractError::TooManyEvents)
        );

        let mut raw_projection = frame(synthetic_frame_bytes());
        raw_projection.source_numeric_encoding = Some(NumericEncodingV1::RawMessagePackBytes);
        assert_eq!(
            raw_projection.validate(),
            Err(RawFrameContractError::InvalidProjectionEncoding)
        );

        for symbols in [
            r#"[ "QQQ   261016C00600000"]"#,
            r#"["QQQ   261016C00600000","QQQ   261016C00600000"]"#,
            r#"["SPY261016C00600000","QQQ   261016C00600000"]"#,
            r#"["QQQ   261016C00600000", "SPY261016C00600000"]"#,
        ] {
            let mut invalid = frame(synthetic_frame_bytes());
            invalid.symbols_json = symbols.to_owned();
            assert_eq!(
                invalid.validate(),
                Err(RawFrameContractError::InvalidSymbols)
            );
        }

        let mut malformed_market = frame(synthetic_frame_bytes());
        malformed_market.event_count = 0;
        assert_eq!(
            malformed_market.validate(),
            Err(RawFrameContractError::InvalidDisposition)
        );

        let mut too_many_symbols = frame(synthetic_frame_bytes());
        too_many_symbols.event_count = 2;
        too_many_symbols.symbols_json =
            r#"["IWM   261016C00600000","QQQ   261016C00600000","SPY   261016C00600000"]"#
                .to_owned();
        assert_eq!(
            too_many_symbols.validate(),
            Err(RawFrameContractError::InvalidDisposition)
        );

        let mut missing_symbols = frame(synthetic_frame_bytes());
        missing_symbols.disposition = RawFrameDispositionV1::UnknownMessage;
        missing_symbols.symbols_json = "[]".to_owned();
        assert_eq!(
            missing_symbols.validate(),
            Err(RawFrameContractError::InvalidDisposition)
        );

        let mut diagnostic_empty = frame(synthetic_frame_bytes());
        diagnostic_empty.disposition = RawFrameDispositionV1::UnknownMessage;
        diagnostic_empty.event_count = 0;
        diagnostic_empty.symbols_json = "[]".to_owned();
        assert_eq!(diagnostic_empty.validate(), Ok(()));

        let too_many = format!(
            "[{}]",
            (0..=MAX_RAW_FRAME_SYMBOLS)
                .map(|index| format!("\"S{index:03}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut oversized_symbol_list = frame(synthetic_frame_bytes());
        oversized_symbol_list.symbols_json = too_many;
        assert_eq!(
            oversized_symbol_list.validate(),
            Err(RawFrameContractError::InvalidSymbols)
        );
        assert_eq!(
            oversized_symbol_list.symbols(),
            Err(RawFrameContractError::InvalidSymbols)
        );

        let mut oversized_json = frame(synthetic_frame_bytes());
        oversized_json.symbols_json = " ".repeat(MAX_RAW_FRAME_SYMBOLS_JSON_BYTES + 1);
        assert_eq!(
            oversized_json.validate(),
            Err(RawFrameContractError::InvalidSymbols)
        );
        assert_eq!(
            oversized_json.symbols(),
            Err(RawFrameContractError::InvalidSymbols)
        );

        let mut control = frame(synthetic_frame_bytes());
        control.disposition = RawFrameDispositionV1::Control;
        control.event_count = 0;
        control.symbols_json = "[]".to_owned();
        assert_eq!(control.validate(), Ok(()));
        control.event_count = 1;
        assert_eq!(
            control.validate(),
            Err(RawFrameContractError::InvalidDisposition)
        );
    }

    #[test]
    fn v2_reference_is_all_or_none_and_must_match_frame_identity_and_digest() {
        let frame = frame(synthetic_frame_bytes());
        let mut event = event();
        assert_eq!(event.validate_against_frame(&frame), Ok(()));

        let mut mismatch = event.clone();
        mismatch
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_sequence += 1;
        assert_eq!(
            mismatch.validate_against_frame(&frame),
            Err(RawFrameContractError::ReferenceMismatch)
        );

        let mut mismatch_digest = event.clone();
        mismatch_digest.event.metadata.raw_frame_sha256 = Some("a".repeat(64));
        assert_eq!(
            mismatch_digest.validate_against_frame(&frame),
            Err(RawFrameContractError::ReferenceMismatch)
        );

        let mut mismatch_received_timestamp = event.clone();
        mismatch_received_timestamp
            .event
            .metadata
            .received_timestamp = UtcTimestamp::parse("2026-10-08T14:30:00.000000001Z").unwrap();
        assert_eq!(
            mismatch_received_timestamp.validate_against_frame(&frame),
            Err(RawFrameContractError::ReferenceMismatch)
        );

        let mut invalid_ordinal = event.clone();
        invalid_ordinal
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_event_ordinal = 2;
        assert_eq!(
            invalid_ordinal.validate(),
            Err(RawFrameContractError::InvalidReference)
        );

        event.raw_frame_reference = None;
        assert_eq!(event.validate(), Ok(()));
        assert_eq!(
            event.validate_against_frame(&frame),
            Err(RawFrameContractError::MissingReference)
        );
    }

    #[test]
    fn raw_frame_debug_hides_payload_and_symbols() {
        let payload = b"PRIVATE_RAW_PAYLOAD_AND_SYMBOL".to_vec();
        let mut value = frame(payload.clone());
        value.symbols_json = r#"["PRIVATE_SYMBOL_VALUE"]"#.to_owned();

        let debug = format!("{value:?}");
        assert!(!debug.contains("PRIVATE_RAW_PAYLOAD_AND_SYMBOL"));
        assert!(!debug.contains("PRIVATE_SYMBOL_VALUE"));
        assert!(!debug.contains(&format!("{:?}", payload)));
        assert!(debug.contains("frame_bytes_len"));
        assert!(debug.contains("symbols_json_bytes_len"));
    }
}
