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
use thiserror::Error;

/// Raw-frame row schema version.
pub const MARKET_RAW_FRAME_SCHEMA_VERSION: u32 = 1;
/// Maximum uncompressed payload size for one received provider frame.
pub const MAX_RAW_FRAME_BYTES: usize = 1024 * 1024;
/// Maximum normalized market events expected from one raw frame.
pub const MAX_RAW_FRAME_EVENT_COUNT: u32 = 512;

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
#[derive(Clone, Debug, Eq, PartialEq)]
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

impl RawFrameStorageRecordV1 {
    /// Validate the frame identity, content digest, projection metadata, and symbol list.
    pub fn validate(&self) -> Result<(), RawFrameContractError> {
        if self.schema_version != MARKET_RAW_FRAME_SCHEMA_VERSION {
            return Err(RawFrameContractError::UnsupportedVersion);
        }
        if self.generation == 0 || self.frame_sequence == 0 {
            return Err(RawFrameContractError::InvalidSequence);
        }
        if self.frame_bytes.len() > MAX_RAW_FRAME_BYTES {
            return Err(RawFrameContractError::FrameTooLarge);
        }
        if self.event_count > MAX_RAW_FRAME_EVENT_COUNT {
            return Err(RawFrameContractError::TooManyEvents);
        }
        if !crate::parquet_schema::validate_sha256_hex(&self.frame_sha256) {
            return Err(RawFrameContractError::InvalidHash);
        }
        if lower_hex(&Sha256::digest(&self.frame_bytes)) != self.frame_sha256 {
            return Err(RawFrameContractError::HashMismatch);
        }
        if self.source_numeric_encoding.is_some_and(|encoding| {
            matches!(
                encoding,
                NumericEncodingV1::Unspecified | NumericEncodingV1::RawMessagePackBytes
            )
        }) {
            return Err(RawFrameContractError::InvalidProjectionEncoding);
        }

        let source = MarketDataSourceV1 {
            provider: self.provider.clone(),
            feed: self.feed.clone(),
            entitlement: self.entitlement,
            numeric_encoding: NumericEncodingV1::RawMessagePackBytes,
            source_record_id: None,
        };
        source
            .validate_for_dataset_manifest()
            .map_err(|_| RawFrameContractError::InvalidSource)?;

        let symbols = parse_canonical_symbols_json(&self.symbols_json)?;
        match self.disposition {
            RawFrameDispositionV1::MarketData if self.event_count == 0 || symbols.is_empty() => {
                return Err(RawFrameContractError::InvalidDisposition);
            }
            RawFrameDispositionV1::Control if self.event_count != 0 || !symbols.is_empty() => {
                return Err(RawFrameContractError::InvalidDisposition);
            }
            _ => {}
        }
        Ok(())
    }

    /// Return the decoded symbol list after validating its canonical representation.
    pub fn symbols(&self) -> Result<Vec<String>, RawFrameContractError> {
        parse_canonical_symbols_json(&self.symbols_json)
    }
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
        self.validate()?;
        frame.validate()?;
        let reference = self
            .raw_frame_reference
            .ok_or(RawFrameContractError::MissingReference)?;
        if reference.raw_frame_generation != frame.generation
            || reference.raw_frame_sequence != frame.frame_sequence
            || reference.raw_frame_event_count != frame.event_count
            || self.event.metadata.raw_frame_sha256.as_deref() != Some(frame.frame_sha256.as_str())
            || self.event.metadata.source.provider != frame.provider
            || self.event.metadata.source.feed != frame.feed
            || self.event.metadata.source.entitlement != frame.entitlement
            || frame
                .source_numeric_encoding
                .is_some_and(|encoding| encoding != self.event.metadata.source.numeric_encoding)
            || !frame
                .symbols()?
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
    let symbols: Vec<String> =
        serde_json::from_str(value).map_err(|_| RawFrameContractError::InvalidSymbols)?;
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
        MAX_RAW_FRAME_BYTES, MAX_RAW_FRAME_EVENT_COUNT, MarketEventParquetRowV2,
        RawFrameContractError, RawFrameDispositionV1, RawFrameReferenceV2, RawFrameStorageRecordV1,
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
}
