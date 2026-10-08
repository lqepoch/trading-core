//! Capture-instance-scoped raw-frame and normalized-event storage contracts.
//!
//! These rows keep the pre-decode source identity separate from the post-decode canonical
//! generation. They are in-memory validation helpers and deliberately do not serialize raw bytes
//! into market-event JSON or ProtoJSON envelopes.

use crate::raw_frame::{
    RawFrameContractError, RawFrameDispositionV1, RawFrameView, event_symbol,
    parse_canonical_symbols_json, validate_raw_frame,
};
use crate::{EntitlementState, MarketEventEnvelopeV1, NumericEncodingV1, UtcTimestamp};
use std::collections::HashMap;
use std::fmt;

/// Raw-frame row schema version for capture-instance-scoped storage.
pub const MARKET_RAW_FRAME_SCHEMA_VERSION_V2: u32 = 2;
/// Maximum number of complete frames in one raw/event Parquet pair chunk.
pub const MAX_RAW_CAPTURE_CHUNK_FRAMES_V2: usize = 1024;
/// Maximum sum of raw payload bytes in one raw/event Parquet pair chunk.
pub const MAX_RAW_CAPTURE_CHUNK_BYTES_V2: usize = 16 * 1024 * 1024;

/// Stable capture identity, encoded as 32 lowercase hexadecimal UUIDv4/RFC-variant digits.
///
/// This identifier correlates spool input with post-decode rows. It is opaque and must never be
/// used as a filesystem path or treated as a provider/session authority token.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RawFrameCaptureInstanceIdV2(String);

impl RawFrameCaptureInstanceIdV2 {
    /// Parse an exact 32-character lowercase UUIDv4 with the RFC variant bits.
    pub fn parse(value: impl Into<String>) -> Result<Self, RawFrameContractError> {
        let value = value.into();
        if !valid_capture_instance_id(&value) {
            return Err(RawFrameContractError::InvalidCaptureInstanceId);
        }
        Ok(Self(value))
    }

    /// Return the canonical lowercase hexadecimal UUID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RawFrameCaptureInstanceIdV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RawFrameCaptureInstanceIdV2")
            .field(&self.0)
            .finish()
    }
}

fn valid_capture_instance_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 32
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        && bytes[12] == b'4'
        && matches!(bytes[16], b'8' | b'9' | b'a' | b'b')
}

macro_rules! raw_frame_record_v2 {
    ($name:ident, $encoding:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Eq, PartialEq)]
        pub struct $name {
            /// Row schema version, fixed at 2.
            pub schema_version: u32,
            /// Provider identity inherited from the input frame.
            pub provider: String,
            /// Exact provider feed identity inherited from the input frame.
            pub feed: String,
            /// Entitlement remains an observation and defaults to unknown when unevidenced.
            pub entitlement: EntitlementState,
            /// Optional uniform numeric projection encoding; this does not describe raw bytes.
            pub source_numeric_encoding: Option<NumericEncodingV1>,
            /// Opaque UUIDv4 identity of the capture instance across reconnect generations.
            pub capture_instance_id: RawFrameCaptureInstanceIdV2,
            /// Source-side generation from the pre-decode spool identity.
            pub source_generation: u64,
            /// Source-side frame sequence within `source_generation`.
            pub source_frame_sequence: u64,
            /// Canonical generation assigned by the post-decode broker projector.
            pub canonical_generation: u64,
            /// Local UTC receive time attached to the exact inbound bytes.
            pub received_timestamp_utc: UtcTimestamp,
            /// SHA-256 recomputed from `frame_bytes` during validation.
            pub frame_sha256: String,
            /// Exact received application-frame bytes.
            pub frame_bytes: Vec<u8>,
            /// Expected normalized quote/trade count, not successful-delivery count.
            pub event_count: u32,
            /// Decoder disposition retained for diagnostic and quarantine rows.
            pub disposition: RawFrameDispositionV1,
            /// Compact JSON of sorted unique normalizable symbols in this frame.
            pub symbols_json: String,
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("schema_version", &self.schema_version)
                    .field("provider", &self.provider)
                    .field("feed", &self.feed)
                    .field("entitlement", &self.entitlement)
                    .field("source_numeric_encoding", &self.source_numeric_encoding)
                    .field("capture_instance_id", &self.capture_instance_id)
                    .field("source_generation", &self.source_generation)
                    .field("source_frame_sequence", &self.source_frame_sequence)
                    .field("canonical_generation", &self.canonical_generation)
                    .field("received_timestamp_utc", &self.received_timestamp_utc)
                    .field("frame_sha256", &self.frame_sha256)
                    .field("frame_bytes_len", &self.frame_bytes.len())
                    .field("event_count", &self.event_count)
                    .field("disposition", &self.disposition)
                    .field("symbols_json_bytes_len", &self.symbols_json.len())
                    .finish()
            }
        }

        impl $name {
            /// Validate row identity, exact bytes, projection encoding, and symbols.
            pub fn validate(&self) -> Result<(), RawFrameContractError> {
                if self.schema_version != MARKET_RAW_FRAME_SCHEMA_VERSION_V2 {
                    return Err(RawFrameContractError::UnsupportedVersion);
                }
                if self.source_generation == 0
                    || self.source_frame_sequence == 0
                    || self.canonical_generation == 0
                {
                    return Err(RawFrameContractError::InvalidSequence);
                }
                validate_raw_frame(self.view(), $encoding, MARKET_RAW_FRAME_SCHEMA_VERSION_V2)
            }

            /// Decode symbols after enforcing the canonical bounded JSON representation.
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
                    generation: self.canonical_generation,
                    frame_sequence: self.source_frame_sequence,
                    received_timestamp_utc: &self.received_timestamp_utc,
                    frame_sha256: &self.frame_sha256,
                    frame_bytes: &self.frame_bytes,
                    event_count: self.event_count,
                    disposition: self.disposition,
                    symbols_json: &self.symbols_json,
                }
            }
        }

        impl RawFrameV2Row for $name {
            fn validate_row(&self) -> Result<(), RawFrameContractError> {
                self.validate()
            }

            fn view(&self) -> RawFrameView<'_> {
                self.view()
            }

            fn capture_instance_id(&self) -> &RawFrameCaptureInstanceIdV2 {
                &self.capture_instance_id
            }

            fn source_generation(&self) -> u64 {
                self.source_generation
            }

            fn source_frame_sequence(&self) -> u64 {
                self.source_frame_sequence
            }
        }
    };
}

trait RawFrameV2Row {
    fn validate_row(&self) -> Result<(), RawFrameContractError>;
    fn view(&self) -> RawFrameView<'_>;
    fn capture_instance_id(&self) -> &RawFrameCaptureInstanceIdV2;
    fn source_generation(&self) -> u64;
    fn source_frame_sequence(&self) -> u64;
}

raw_frame_record_v2!(
    RawFrameStorageRecordV2,
    NumericEncodingV1::RawMessagePackBytes,
    "One exact MessagePack application-frame row in `lqepoch.market_raw_frame.v2`."
);

raw_frame_record_v2!(
    RawJsonFrameStorageRecordV2,
    NumericEncodingV1::RawJsonBytes,
    "One exact JSON application-frame row in `lqepoch.market_raw_json_frame.v2`."
);

/// All nullable raw-capture columns appended by the `lqepoch.market_event.v3` descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawFrameReferenceV3 {
    /// Capture UUID used by the pre-decode input spool.
    pub raw_frame_capture_instance_id: RawFrameCaptureInstanceIdV2,
    /// Source generation from the capture key; never inferred from the canonical generation.
    pub raw_frame_source_generation: u64,
    /// Post-decode canonical generation, equal to the event's generation.
    pub raw_frame_generation: u64,
    /// Source-side frame sequence within `raw_frame_source_generation`.
    pub raw_frame_sequence: u64,
    /// One-based event ordinal in the exact source frame.
    pub raw_frame_event_ordinal: u32,
    /// Expected normalized quote/trade count in the exact source frame.
    pub raw_frame_event_count: u32,
}

/// Event envelope with the additive capture-scoped V3 Parquet correlation columns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketEventParquetRowV3 {
    /// Existing event wire DTO; its schema version remains 1.
    pub event: MarketEventEnvelopeV1,
    /// The six storage correlation columns are all present or all absent.
    pub raw_frame_reference: Option<RawFrameReferenceV3>,
}

impl MarketEventParquetRowV3 {
    /// Validate an event and its optional all-or-none capture key without reading raw bytes.
    pub fn validate(&self) -> Result<(), RawFrameContractError> {
        self.event
            .validate()
            .map_err(|_| RawFrameContractError::InvalidEvent)?;
        if let Some(reference) = &self.raw_frame_reference
            && (reference.raw_frame_source_generation == 0
                || reference.raw_frame_generation == 0
                || reference.raw_frame_sequence == 0
                || reference.raw_frame_generation != self.event.metadata.generation
                || reference.raw_frame_event_count == 0
                || reference.raw_frame_event_count > crate::MAX_RAW_FRAME_EVENT_COUNT
                || reference.raw_frame_event_ordinal == 0
                || reference.raw_frame_event_ordinal > reference.raw_frame_event_count
                || self.event.metadata.raw_frame_sha256.is_none())
        {
            return Err(RawFrameContractError::InvalidReference);
        }
        Ok(())
    }

    /// Cross-check this event against one exact MessagePack storage row.
    pub fn validate_against_frame(
        &self,
        frame: &RawFrameStorageRecordV2,
    ) -> Result<(), RawFrameContractError> {
        frame.validate()?;
        validate_event_against_validated_frame(
            self,
            frame.view(),
            &frame.capture_instance_id,
            frame.source_generation,
        )
    }

    /// Cross-check this event against one exact JSON storage row.
    pub fn validate_against_json_frame(
        &self,
        frame: &RawJsonFrameStorageRecordV2,
    ) -> Result<(), RawFrameContractError> {
        frame.validate()?;
        validate_event_against_validated_frame(
            self,
            frame.view(),
            &frame.capture_instance_id,
            frame.source_generation,
        )
    }
}

fn validate_event_against_validated_frame(
    row: &MarketEventParquetRowV3,
    frame: RawFrameView<'_>,
    capture_instance_id: &RawFrameCaptureInstanceIdV2,
    source_generation: u64,
) -> Result<(), RawFrameContractError> {
    row.validate()?;
    let reference = row
        .raw_frame_reference
        .as_ref()
        .ok_or(RawFrameContractError::MissingReference)?;
    let symbols = parse_canonical_symbols_json(frame.symbols_json)?;
    if reference.raw_frame_capture_instance_id != *capture_instance_id
        || reference.raw_frame_source_generation != source_generation
        || reference.raw_frame_generation != frame.generation
        || reference.raw_frame_sequence != frame.frame_sequence
        || reference.raw_frame_event_count != frame.event_count
        || row.event.metadata.raw_frame_sha256.as_deref() != Some(frame.frame_sha256)
        || row.event.metadata.source.provider != frame.provider
        || row.event.metadata.source.feed != frame.feed
        || row.event.metadata.source.entitlement != frame.entitlement
        || row.event.metadata.received_timestamp != *frame.received_timestamp_utc
        || frame
            .source_numeric_encoding
            .is_some_and(|encoding| encoding != row.event.metadata.source.numeric_encoding)
        || !symbols
            .iter()
            .any(|symbol| symbol == event_symbol(&row.event.event))
    {
        return Err(RawFrameContractError::ReferenceMismatch);
    }
    Ok(())
}

/// Validate a bounded MessagePack raw/event chunk with full frame coverage.
pub fn validate_messagepack_capture_chunk_v2(
    frames: &[RawFrameStorageRecordV2],
    events: &[MarketEventParquetRowV3],
) -> Result<(), RawFrameContractError> {
    validate_capture_chunk_v2(frames, events)
}

/// Validate a bounded JSON raw/event chunk with full frame coverage.
pub fn validate_json_capture_chunk_v2(
    frames: &[RawJsonFrameStorageRecordV2],
    events: &[MarketEventParquetRowV3],
) -> Result<(), RawFrameContractError> {
    validate_capture_chunk_v2(frames, events)
}

fn validate_capture_chunk_v2<T: RawFrameV2Row>(
    frames: &[T],
    events: &[MarketEventParquetRowV3],
) -> Result<(), RawFrameContractError> {
    if frames.is_empty() || frames.len() > MAX_RAW_CAPTURE_CHUNK_FRAMES_V2 {
        return Err(RawFrameContractError::CaptureChunkTooLarge);
    }

    let capture_id = frames[0].capture_instance_id();
    let source_generation = frames[0].source_generation();
    let mut payload_bytes = 0_usize;
    let mut expected_event_count = 0_usize;
    let mut frame_index = HashMap::with_capacity(frames.len());
    for (index, frame) in frames.iter().enumerate() {
        frame.validate_row()?;
        if frame.capture_instance_id() != capture_id
            || frame.source_generation() != source_generation
        {
            return Err(RawFrameContractError::InvalidCaptureChunk);
        }
        if index > 0 {
            let expected = frames[index - 1]
                .source_frame_sequence()
                .checked_add(1)
                .ok_or(RawFrameContractError::InvalidCaptureChunk)?;
            if frame.source_frame_sequence() != expected {
                return Err(RawFrameContractError::InvalidCaptureChunk);
            }
        }
        if frame_index
            .insert(frame.source_frame_sequence(), index)
            .is_some()
        {
            return Err(RawFrameContractError::InvalidCaptureChunk);
        }
        payload_bytes = payload_bytes
            .checked_add(frame.view().frame_bytes.len())
            .ok_or(RawFrameContractError::CaptureChunkTooLarge)?;
        expected_event_count = expected_event_count
            .checked_add(frame.view().event_count as usize)
            .ok_or(RawFrameContractError::IncompleteProjection)?;
        if payload_bytes > MAX_RAW_CAPTURE_CHUNK_BYTES_V2 {
            return Err(RawFrameContractError::CaptureChunkTooLarge);
        }
    }
    if events.len() != expected_event_count {
        return Err(RawFrameContractError::IncompleteProjection);
    }

    let mut events_by_frame: Vec<Vec<&MarketEventParquetRowV3>> =
        (0..frames.len()).map(|_| Vec::new()).collect();
    for event in events {
        event.validate()?;
        let reference = event
            .raw_frame_reference
            .as_ref()
            .ok_or(RawFrameContractError::MissingReference)?;
        if reference.raw_frame_capture_instance_id != *capture_id
            || reference.raw_frame_source_generation != source_generation
        {
            return Err(RawFrameContractError::ReferenceMismatch);
        }
        let index = frame_index
            .get(&reference.raw_frame_sequence)
            .copied()
            .ok_or(RawFrameContractError::ReferenceMismatch)?;
        events_by_frame[index].push(event);
    }

    for (frame, frame_events) in frames.iter().zip(events_by_frame) {
        validate_complete_frame_projection(
            frame.view(),
            frame.capture_instance_id(),
            frame.source_generation(),
            &frame_events,
        )?;
    }
    Ok(())
}

fn validate_complete_frame_projection(
    frame: RawFrameView<'_>,
    capture_instance_id: &RawFrameCaptureInstanceIdV2,
    source_generation: u64,
    events: &[&MarketEventParquetRowV3],
) -> Result<(), RawFrameContractError> {
    if events.len() != frame.event_count as usize {
        return Err(RawFrameContractError::IncompleteProjection);
    }
    let mut seen_ordinals = vec![false; frame.event_count as usize];
    for event in events {
        validate_event_against_validated_frame(
            event,
            frame,
            capture_instance_id,
            source_generation,
        )?;
        let reference = event
            .raw_frame_reference
            .as_ref()
            .ok_or(RawFrameContractError::MissingReference)?;
        let ordinal = reference.raw_frame_event_ordinal as usize - 1;
        if std::mem::replace(&mut seen_ordinals[ordinal], true) {
            return Err(RawFrameContractError::InvalidReference);
        }
    }
    if seen_ordinals.into_iter().any(|seen| !seen) {
        return Err(RawFrameContractError::IncompleteProjection);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RAW_CAPTURE_CHUNK_BYTES_V2, MAX_RAW_CAPTURE_CHUNK_FRAMES_V2, MarketEventParquetRowV3,
        RawFrameCaptureInstanceIdV2, RawFrameReferenceV3, RawFrameStorageRecordV2,
        RawJsonFrameStorageRecordV2, validate_json_capture_chunk_v2,
        validate_messagepack_capture_chunk_v2,
    };
    use crate::{
        DecimalString, EntitlementState, EventMetadataV1, MarketDataSourceV1,
        MarketEventEnvelopeV1, MarketEventV1, NumericEncodingV1, RawFrameContractError,
        RawFrameDispositionV1, UtcTimestamp,
    };
    use sha2::{Digest, Sha256};

    const CAPTURE_ID: &str = "0123456789ab4def8123456789abcdef";
    const OTHER_CAPTURE_ID: &str = "fedcba9876544def8123456789abcdef";
    const OPTION_SYMBOL: &str = "QQQ   261016C00600000";
    const RECEIVED_AT: &str = "2026-10-08T14:30:00Z";

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn messagepack_frame(sequence: u64, bytes: Vec<u8>) -> RawFrameStorageRecordV2 {
        RawFrameStorageRecordV2 {
            schema_version: 2,
            provider: "synthetic".to_owned(),
            feed: "synthetic".to_owned(),
            entitlement: EntitlementState::Unknown,
            source_numeric_encoding: Some(NumericEncodingV1::DecimalToken),
            capture_instance_id: RawFrameCaptureInstanceIdV2::parse(CAPTURE_ID).unwrap(),
            source_generation: 7,
            source_frame_sequence: sequence,
            canonical_generation: 19,
            received_timestamp_utc: UtcTimestamp::parse(RECEIVED_AT).unwrap(),
            frame_sha256: digest(&bytes),
            frame_bytes: bytes,
            event_count: 1,
            disposition: RawFrameDispositionV1::MarketData,
            symbols_json: format!("[\"{OPTION_SYMBOL}\"]"),
        }
    }

    fn json_frame(sequence: u64, bytes: Vec<u8>) -> RawJsonFrameStorageRecordV2 {
        RawJsonFrameStorageRecordV2 {
            schema_version: 2,
            provider: "synthetic".to_owned(),
            feed: "synthetic".to_owned(),
            entitlement: EntitlementState::Unknown,
            source_numeric_encoding: Some(NumericEncodingV1::DecimalToken),
            capture_instance_id: RawFrameCaptureInstanceIdV2::parse(CAPTURE_ID).unwrap(),
            source_generation: 7,
            source_frame_sequence: sequence,
            canonical_generation: 19,
            received_timestamp_utc: UtcTimestamp::parse(RECEIVED_AT).unwrap(),
            frame_sha256: digest(&bytes),
            frame_bytes: bytes,
            event_count: 1,
            disposition: RawFrameDispositionV1::MarketData,
            symbols_json: format!("[\"{OPTION_SYMBOL}\"]"),
        }
    }

    fn event_for_frame(
        capture_id: &RawFrameCaptureInstanceIdV2,
        source_generation: u64,
        source_sequence: u64,
        canonical_generation: u64,
        frame_sha256: &str,
    ) -> MarketEventParquetRowV3 {
        MarketEventParquetRowV3 {
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
                    generation: canonical_generation,
                    sequence: source_sequence + 100,
                    raw_frame_sha256: Some(frame_sha256.to_owned()),
                    source_timestamp: None,
                    received_timestamp: UtcTimestamp::parse(RECEIVED_AT).unwrap(),
                },
                event: MarketEventV1::OptionTrade {
                    symbol: OPTION_SYMBOL.to_owned(),
                    price: DecimalString::new("2.75").unwrap(),
                    size: DecimalString::new("1").unwrap(),
                },
            },
            raw_frame_reference: Some(RawFrameReferenceV3 {
                raw_frame_capture_instance_id: capture_id.clone(),
                raw_frame_source_generation: source_generation,
                raw_frame_generation: canonical_generation,
                raw_frame_sequence: source_sequence,
                raw_frame_event_ordinal: 1,
                raw_frame_event_count: 1,
            }),
        }
    }

    fn event_for_raw_frame(frame: &RawFrameStorageRecordV2) -> MarketEventParquetRowV3 {
        event_for_frame(
            &frame.capture_instance_id,
            frame.source_generation,
            frame.source_frame_sequence,
            frame.canonical_generation,
            &frame.frame_sha256,
        )
    }

    fn event_for_json_frame(frame: &RawJsonFrameStorageRecordV2) -> MarketEventParquetRowV3 {
        event_for_frame(
            &frame.capture_instance_id,
            frame.source_generation,
            frame.source_frame_sequence,
            frame.canonical_generation,
            &frame.frame_sha256,
        )
    }

    fn empty_diagnostic_frame(sequence: u64, bytes: Vec<u8>) -> RawFrameStorageRecordV2 {
        let mut frame = messagepack_frame(sequence, bytes);
        frame.event_count = 0;
        frame.disposition = RawFrameDispositionV1::UnknownMessage;
        frame.symbols_json = "[]".to_owned();
        frame
    }

    fn fixture_string<'a>(value: &'a serde_json::Value, field: &str) -> &'a str {
        value[field].as_str().unwrap()
    }

    fn fixture_u64(value: &serde_json::Value, field: &str) -> u64 {
        fixture_string(value, field).parse().unwrap()
    }

    fn fixture_hex(value: &str) -> Vec<u8> {
        let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
        assert!(remainder.is_empty());
        pairs
            .iter()
            .map(|pair| {
                let high = (pair[0] as char).to_digit(16).unwrap();
                let low = (pair[1] as char).to_digit(16).unwrap();
                ((high << 4) | low) as u8
            })
            .collect()
    }

    fn fixture_encoding(value: &str) -> NumericEncodingV1 {
        match value {
            "decimal_token" => NumericEncodingV1::DecimalToken,
            "integer_token" => NumericEncodingV1::IntegerToken,
            "binary_float64_shortest_decimal" => NumericEncodingV1::BinaryFloat64ShortestDecimal,
            "binary_float32_shortest_decimal" => NumericEncodingV1::BinaryFloat32ShortestDecimal,
            other => panic!("unexpected synthetic fixture numeric encoding: {other}"),
        }
    }

    fn fixture_entitlement(value: &str) -> EntitlementState {
        match value {
            "unknown" => EntitlementState::Unknown,
            "authorized" => EntitlementState::Authorized,
            "unauthorized" => EntitlementState::Unauthorized,
            other => panic!("unexpected synthetic fixture entitlement: {other}"),
        }
    }

    fn fixture_disposition(value: &str) -> RawFrameDispositionV1 {
        match value {
            "market_data" => RawFrameDispositionV1::MarketData,
            "control" => RawFrameDispositionV1::Control,
            "unknown_message" => RawFrameDispositionV1::UnknownMessage,
            "malformed_message" => RawFrameDispositionV1::MalformedMessage,
            "provider_error" => RawFrameDispositionV1::ProviderError,
            other => panic!("unexpected synthetic fixture disposition: {other}"),
        }
    }

    struct FixtureRawFrameFields {
        schema_version: u32,
        provider: String,
        feed: String,
        entitlement: EntitlementState,
        source_numeric_encoding: Option<NumericEncodingV1>,
        capture_instance_id: RawFrameCaptureInstanceIdV2,
        source_generation: u64,
        source_frame_sequence: u64,
        canonical_generation: u64,
        received_timestamp_utc: UtcTimestamp,
        frame_sha256: String,
        frame_bytes: Vec<u8>,
        event_count: u32,
        disposition: RawFrameDispositionV1,
        symbols_json: String,
    }

    fn fixture_raw_frame_values(fixture_frame: &serde_json::Value) -> FixtureRawFrameFields {
        let row = &fixture_frame["row"];
        FixtureRawFrameFields {
            schema_version: row["schema_version"].as_u64().unwrap() as u32,
            provider: fixture_string(row, "provider").to_owned(),
            feed: fixture_string(row, "feed").to_owned(),
            entitlement: fixture_entitlement(fixture_string(row, "entitlement")),
            source_numeric_encoding: row["source_numeric_encoding"]
                .as_str()
                .map(fixture_encoding),
            capture_instance_id: RawFrameCaptureInstanceIdV2::parse(fixture_string(
                row,
                "capture_instance_id",
            ))
            .unwrap(),
            source_generation: fixture_u64(row, "source_generation"),
            source_frame_sequence: fixture_u64(row, "source_frame_sequence"),
            canonical_generation: fixture_u64(row, "canonical_generation"),
            received_timestamp_utc: UtcTimestamp::parse(fixture_string(
                row,
                "received_timestamp_utc",
            ))
            .unwrap(),
            frame_sha256: fixture_string(row, "frame_sha256").to_owned(),
            frame_bytes: fixture_hex(fixture_string(fixture_frame, "frame_bytes_hex")),
            event_count: row["event_count"].as_u64().unwrap() as u32,
            disposition: fixture_disposition(fixture_string(row, "disposition")),
            symbols_json: fixture_string(row, "symbols_json").to_owned(),
        }
    }

    fn raw_frame_from_fixture(fixture_frame: &serde_json::Value) -> RawFrameStorageRecordV2 {
        let fields = fixture_raw_frame_values(fixture_frame);
        RawFrameStorageRecordV2 {
            schema_version: fields.schema_version,
            provider: fields.provider,
            feed: fields.feed,
            entitlement: fields.entitlement,
            source_numeric_encoding: fields.source_numeric_encoding,
            capture_instance_id: fields.capture_instance_id,
            source_generation: fields.source_generation,
            source_frame_sequence: fields.source_frame_sequence,
            canonical_generation: fields.canonical_generation,
            received_timestamp_utc: fields.received_timestamp_utc,
            frame_sha256: fields.frame_sha256,
            frame_bytes: fields.frame_bytes,
            event_count: fields.event_count,
            disposition: fields.disposition,
            symbols_json: fields.symbols_json,
        }
    }

    fn json_frame_from_fixture(fixture_frame: &serde_json::Value) -> RawJsonFrameStorageRecordV2 {
        let fields = fixture_raw_frame_values(fixture_frame);
        RawJsonFrameStorageRecordV2 {
            schema_version: fields.schema_version,
            provider: fields.provider,
            feed: fields.feed,
            entitlement: fields.entitlement,
            source_numeric_encoding: fields.source_numeric_encoding,
            capture_instance_id: fields.capture_instance_id,
            source_generation: fields.source_generation,
            source_frame_sequence: fields.source_frame_sequence,
            canonical_generation: fields.canonical_generation,
            received_timestamp_utc: fields.received_timestamp_utc,
            frame_sha256: fields.frame_sha256,
            frame_bytes: fields.frame_bytes,
            event_count: fields.event_count,
            disposition: fields.disposition,
            symbols_json: fields.symbols_json,
        }
    }

    fn fixture_decimal(value: &serde_json::Value) -> Option<DecimalString> {
        value.as_str().map(|text| DecimalString::new(text).unwrap())
    }

    fn event_from_fixture(row: &serde_json::Value) -> MarketEventParquetRowV3 {
        let symbol = fixture_string(row, "symbol").to_owned();
        let event = match fixture_string(row, "event_kind") {
            "stock_trade" => MarketEventV1::StockTrade {
                symbol,
                price: fixture_decimal(&row["price"]).unwrap(),
                size: fixture_decimal(&row["size"]).unwrap(),
            },
            "option_trade" => MarketEventV1::OptionTrade {
                symbol,
                price: fixture_decimal(&row["price"]).unwrap(),
                size: fixture_decimal(&row["size"]).unwrap(),
            },
            "stock_quote" => MarketEventV1::StockQuote {
                symbol,
                bid: fixture_decimal(&row["bid"]),
                ask: fixture_decimal(&row["ask"]),
                bid_size: fixture_decimal(&row["bid_size"]),
                ask_size: fixture_decimal(&row["ask_size"]),
            },
            "option_quote" => MarketEventV1::OptionQuote {
                symbol,
                bid: fixture_decimal(&row["bid"]),
                ask: fixture_decimal(&row["ask"]),
                bid_size: fixture_decimal(&row["bid_size"]),
                ask_size: fixture_decimal(&row["ask_size"]),
            },
            other => panic!("unexpected synthetic fixture event kind: {other}"),
        };
        let capture_id = row["raw_frame_capture_instance_id"]
            .as_str()
            .map(RawFrameCaptureInstanceIdV2::parse)
            .transpose()
            .unwrap();
        let raw_frame_reference =
            capture_id.map(|raw_frame_capture_instance_id| RawFrameReferenceV3 {
                raw_frame_capture_instance_id,
                raw_frame_source_generation: fixture_u64(row, "raw_frame_source_generation"),
                raw_frame_generation: fixture_u64(row, "raw_frame_generation"),
                raw_frame_sequence: fixture_u64(row, "raw_frame_sequence"),
                raw_frame_event_ordinal: row["raw_frame_event_ordinal"].as_u64().unwrap() as u32,
                raw_frame_event_count: row["raw_frame_event_count"].as_u64().unwrap() as u32,
            });
        MarketEventParquetRowV3 {
            event: MarketEventEnvelopeV1 {
                metadata: EventMetadataV1 {
                    schema_version: row["schema_version"].as_u64().unwrap() as u32,
                    source: MarketDataSourceV1::new(
                        fixture_string(row, "provider"),
                        fixture_string(row, "feed"),
                        fixture_entitlement(fixture_string(row, "entitlement")),
                        fixture_encoding(fixture_string(row, "numeric_encoding")),
                        row["source_record_id"].as_str().map(str::to_owned),
                    )
                    .unwrap(),
                    generation: fixture_u64(row, "generation"),
                    sequence: fixture_u64(row, "sequence"),
                    raw_frame_sha256: row["raw_frame_sha256"].as_str().map(str::to_owned),
                    source_timestamp: row["source_timestamp"]
                        .as_str()
                        .map(UtcTimestamp::parse)
                        .transpose()
                        .unwrap(),
                    received_timestamp: UtcTimestamp::parse(fixture_string(
                        row,
                        "received_timestamp",
                    ))
                    .unwrap(),
                },
                event,
            },
            raw_frame_reference,
        }
    }

    #[test]
    fn capture_id_requires_lowercase_uuid_v4_and_rfc_variant() {
        assert_eq!(
            RawFrameCaptureInstanceIdV2::parse(CAPTURE_ID)
                .unwrap()
                .as_str(),
            CAPTURE_ID
        );
        for invalid in [
            "0123456789AB4def8123456789abcdef",
            "0123456789ab3def8123456789abcdef",
            "0123456789ab4def7123456789abcdef",
            "0123456789ab4def8123456789abcde",
            "01234567-89ab-4def-8123-456789abcdef",
        ] {
            assert_eq!(
                RawFrameCaptureInstanceIdV2::parse(invalid),
                Err(RawFrameContractError::InvalidCaptureInstanceId)
            );
        }
    }

    #[test]
    fn shared_cross_language_fixture_validates_messagepack_and_json_rows() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/fixtures/raw-frame-capture-v2.json"
        ))
        .unwrap();
        let messagepack_frames = fixture["messagepack_frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(raw_frame_from_fixture)
            .collect::<Vec<_>>();
        let messagepack_events = fixture["messagepack_events"]
            .as_array()
            .unwrap()
            .iter()
            .map(event_from_fixture)
            .collect::<Vec<_>>();
        assert_eq!(
            validate_messagepack_capture_chunk_v2(&messagepack_frames, &messagepack_events),
            Ok(())
        );

        let json_frame = json_frame_from_fixture(&fixture["json_frame"]);
        let json_event = event_from_fixture(&fixture["json_event"]);
        assert_eq!(json_frame.validate(), Ok(()));
        assert_eq!(json_event.validate_against_json_frame(&json_frame), Ok(()));
        assert_eq!(
            validate_json_capture_chunk_v2(&[json_frame], &[json_event]),
            Ok(())
        );
    }

    #[test]
    fn v2_messagepack_and_json_rows_bind_distinct_wire_encoding_and_full_identity() {
        let messagepack_bytes = vec![0x92, 0xa1, b't', 0x01];
        let messagepack = messagepack_frame(11, messagepack_bytes);
        let event = event_for_raw_frame(&messagepack);
        assert_eq!(messagepack.validate(), Ok(()));
        assert_eq!(event.validate_against_frame(&messagepack), Ok(()));

        let json_bytes = br#"{"T":"t","S":"QQQ","p":2.75}"#.to_vec();
        let json = json_frame(12, json_bytes);
        let event = event_for_json_frame(&json);
        assert_eq!(json.validate(), Ok(()));
        assert_eq!(event.validate_against_json_frame(&json), Ok(()));
        assert_eq!(
            validate_json_capture_chunk_v2(std::slice::from_ref(&json), &[event]),
            Ok(())
        );

        let mut invalid_messagepack = messagepack.clone();
        invalid_messagepack.schema_version = 1;
        assert_eq!(
            invalid_messagepack.validate(),
            Err(RawFrameContractError::UnsupportedVersion)
        );
        let mut invalid_json = json.clone();
        invalid_json.source_numeric_encoding = Some(NumericEncodingV1::RawJsonBytes);
        assert_eq!(
            invalid_json.validate(),
            Err(RawFrameContractError::InvalidProjectionEncoding)
        );
    }

    #[test]
    fn event_v3_checks_capture_source_and_canonical_generations_independently() {
        let frame = messagepack_frame(11, vec![0x92, 0xa1, b't', 0x01]);
        let event = event_for_raw_frame(&frame);
        assert_eq!(event.validate_against_frame(&frame), Ok(()));

        let mut wrong_capture = event.clone();
        wrong_capture
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_capture_instance_id =
            RawFrameCaptureInstanceIdV2::parse(OTHER_CAPTURE_ID).unwrap();
        assert_eq!(
            wrong_capture.validate_against_frame(&frame),
            Err(RawFrameContractError::ReferenceMismatch)
        );

        let mut wrong_source_generation = event.clone();
        wrong_source_generation
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_source_generation += 1;
        assert_eq!(
            wrong_source_generation.validate_against_frame(&frame),
            Err(RawFrameContractError::ReferenceMismatch)
        );

        let mut wrong_canonical_generation = event.clone();
        wrong_canonical_generation
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_generation += 1;
        assert_eq!(
            wrong_canonical_generation.validate(),
            Err(RawFrameContractError::InvalidReference)
        );

        let mut wrong_timestamp = event;
        wrong_timestamp.event.metadata.received_timestamp =
            UtcTimestamp::parse("2026-10-08T14:30:00.000000001Z").unwrap();
        assert_eq!(
            wrong_timestamp.validate_against_frame(&frame),
            Err(RawFrameContractError::ReferenceMismatch)
        );
    }

    #[test]
    fn chunk_requires_single_instance_generation_continuous_sequences_and_complete_events() {
        let first = messagepack_frame(11, vec![0x92, 0xa1, b't', 0x01]);
        let second = messagepack_frame(12, vec![0x92, 0xa1, b't', 0x02]);
        let events = vec![event_for_raw_frame(&first), event_for_raw_frame(&second)];
        assert_eq!(
            validate_messagepack_capture_chunk_v2(&[first.clone(), second.clone()], &events),
            Ok(())
        );
        assert_eq!(
            validate_messagepack_capture_chunk_v2(&[first.clone(), second.clone()], &events[..1]),
            Err(RawFrameContractError::IncompleteProjection)
        );

        let mut gap = second.clone();
        gap.source_frame_sequence = 13;
        let gap_event = event_for_raw_frame(&gap);
        assert_eq!(
            validate_messagepack_capture_chunk_v2(
                &[first.clone(), gap],
                &[events[0].clone(), gap_event]
            ),
            Err(RawFrameContractError::InvalidCaptureChunk)
        );

        let mut mixed_generation = second.clone();
        mixed_generation.source_generation += 1;
        let mixed_event = event_for_raw_frame(&mixed_generation);
        assert_eq!(
            validate_messagepack_capture_chunk_v2(
                &[first.clone(), mixed_generation],
                &[events[0].clone(), mixed_event],
            ),
            Err(RawFrameContractError::InvalidCaptureChunk)
        );

        let mut mixed_capture = second.clone();
        mixed_capture.capture_instance_id =
            RawFrameCaptureInstanceIdV2::parse(OTHER_CAPTURE_ID).unwrap();
        let mixed_event = event_for_raw_frame(&mixed_capture);
        assert_eq!(
            validate_messagepack_capture_chunk_v2(
                &[first.clone(), mixed_capture],
                &[events[0].clone(), mixed_event],
            ),
            Err(RawFrameContractError::InvalidCaptureChunk)
        );

        let mut two_event_frame = messagepack_frame(11, vec![0x92, 0xa1, b't', 0x01]);
        two_event_frame.event_count = 2;
        let mut first_event = event_for_raw_frame(&two_event_frame);
        first_event
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_event_count = 2;
        let mut second_event = first_event.clone();
        second_event.event.metadata.sequence += 1;
        second_event
            .raw_frame_reference
            .as_mut()
            .unwrap()
            .raw_frame_event_ordinal = 2;
        let valid_two_event_rows = vec![first_event.clone(), second_event.clone()];
        assert_eq!(
            validate_messagepack_capture_chunk_v2(
                &[two_event_frame.clone()],
                &valid_two_event_rows,
            ),
            Ok(())
        );
        let duplicate_ordinal = vec![first_event, {
            second_event
                .raw_frame_reference
                .as_mut()
                .unwrap()
                .raw_frame_event_ordinal = 1;
            second_event
        }];
        assert_eq!(
            validate_messagepack_capture_chunk_v2(&[two_event_frame], &duplicate_ordinal),
            Err(RawFrameContractError::InvalidReference)
        );
    }

    #[test]
    fn chunk_limits_are_checked_before_any_unbounded_grouping() {
        let too_many_frames = (1..=MAX_RAW_CAPTURE_CHUNK_FRAMES_V2 as u64 + 1)
            .map(|sequence| empty_diagnostic_frame(sequence, Vec::new()))
            .collect::<Vec<_>>();
        assert_eq!(
            validate_messagepack_capture_chunk_v2(&too_many_frames, &[]),
            Err(RawFrameContractError::CaptureChunkTooLarge)
        );

        let too_many_bytes = (1..=17)
            .map(|sequence| empty_diagnostic_frame(sequence, vec![sequence as u8; 1024 * 1024]))
            .collect::<Vec<_>>();
        assert!(
            too_many_bytes
                .iter()
                .map(|frame| frame.frame_bytes.len())
                .sum::<usize>()
                > MAX_RAW_CAPTURE_CHUNK_BYTES_V2
        );
        assert_eq!(
            validate_messagepack_capture_chunk_v2(&too_many_bytes, &[]),
            Err(RawFrameContractError::CaptureChunkTooLarge)
        );
    }

    #[test]
    fn raw_row_debug_hides_exact_bytes_and_symbol_values() {
        let payload = b"SYNTHETIC_PAYLOAD_NOT_A_PROVIDER_FRAME".to_vec();
        let mut frame = messagepack_frame(11, payload.clone());
        frame.symbols_json = r#"["SYNTHETIC_PRIVATE_SYMBOL"]"#.to_owned();
        let debug = format!("{frame:?}");
        assert!(!debug.contains("SYNTHETIC_PAYLOAD_NOT_A_PROVIDER_FRAME"));
        assert!(!debug.contains("SYNTHETIC_PRIVATE_SYMBOL"));
        assert!(!debug.contains(&format!("{payload:?}")));
        assert!(debug.contains("frame_bytes_len"));
        assert!(debug.contains("symbols_json_bytes_len"));
    }
}
