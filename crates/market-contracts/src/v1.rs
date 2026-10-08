//! Version 1 market event and control envelopes.
//!
//! Prices and sizes use validated decimal strings to preserve exact wire values. Provider/feed
//! identity, source time, local receive time, generation, and sequence remain explicit.

use chrono::{DateTime, SecondsFormat, Utc};
use domain::OptionSymbol;
use exact_decimal::ExactDecimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{cmp::Ordering, collections::HashSet, fmt, hash::Hash};
use thiserror::Error;

/// Current version of the serialized market event contract.
pub const MARKET_EVENT_SCHEMA_VERSION: u32 = 1;
/// Maximum UTF-8 bytes in one provider, feed, or source-record identifier.
pub const MAX_SOURCE_ID_BYTES: usize = 128;
/// Maximum UTF-8 bytes in one instrument or subscription identifier.
pub const MAX_INSTRUMENT_ID_BYTES: usize = 256;
/// Maximum raw market/control frame size adapters must enforce before decoding.
pub const MAX_MARKET_FRAME_BYTES: usize = 16 * 1024;
/// Maximum accepted plus rejected instruments in one provider acknowledgement.
pub const MAX_CONTROL_INSTRUMENTS: usize = 32;

/// Stable validation failures for a market contract value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum MarketWireError {
    #[error("invalid decimal value")]
    InvalidDecimal,
    #[error("invalid UTC timestamp")]
    InvalidTimestamp,
    #[error("invalid source identity")]
    InvalidSource,
    #[error("invalid event version or sequence")]
    InvalidSequence,
    #[error("invalid instrument identity")]
    InvalidInstrument,
    #[error("invalid trade size")]
    InvalidTradeSize,
    #[error("invalid price")]
    InvalidPrice,
    #[error("invalid quote")]
    InvalidQuote,
    #[error("invalid quote size")]
    InvalidQuoteSize,
    #[error("invalid subscription acknowledgement")]
    InvalidAcknowledgement,
    #[error("market frame exceeds the configured byte bound")]
    FrameTooLarge,
}

/// Reject an untrusted provider frame before parsing it into a DTO.
pub fn validate_market_frame_len(length: usize) -> Result<(), MarketWireError> {
    if length <= MAX_MARKET_FRAME_BYTES {
        Ok(())
    } else {
        Err(MarketWireError::FrameTooLarge)
    }
}

/// Exact base-10 JSON-number text carried as a JSON string.
///
/// The original token is retained after strict bounded decimal validation.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct DecimalString(String);

impl DecimalString {
    /// Validate and retain one exact decimal token.
    pub fn new(value: impl Into<String>) -> Result<Self, MarketWireError> {
        let value = value.into();
        ExactDecimal::parse_json_number(&value).map_err(|_| MarketWireError::InvalidDecimal)?;
        Ok(Self(value))
    }

    /// Borrow the exact source token.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parsed(&self) -> ExactDecimal {
        ExactDecimal::parse_json_number(&self.0)
            .expect("DecimalString is validated at construction")
    }
}

impl fmt::Debug for DecimalString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DecimalString")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for DecimalString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for DecimalString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DecimalString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// RFC3339 timestamp normalized to UTC with minimal fractional precision.
#[derive(Clone, Debug)]
pub struct UtcTimestamp {
    instant: DateTime<Utc>,
    canonical: String,
}

impl UtcTimestamp {
    /// Parse an RFC3339 timestamp and require a zero UTC offset.
    pub fn parse(value: &str) -> Result<Self, MarketWireError> {
        let parsed =
            DateTime::parse_from_rfc3339(value).map_err(|_| MarketWireError::InvalidTimestamp)?;
        if parsed.offset().local_minus_utc() != 0 {
            return Err(MarketWireError::InvalidTimestamp);
        }
        let instant = parsed.with_timezone(&Utc);
        Ok(Self {
            canonical: canonical_utc_timestamp(&instant),
            instant,
        })
    }

    /// Return the normalized RFC3339 value.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }
}

impl PartialEq for UtcTimestamp {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant
    }
}

impl Eq for UtcTimestamp {}

impl PartialOrd for UtcTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UtcTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.instant.cmp(&other.instant)
    }
}

impl Hash for UtcTimestamp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.instant.hash(state);
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Whether the provider/feed entitlement is independently known.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitlementState {
    /// No trusted entitlement evidence is attached.
    #[default]
    Unknown,
    /// A trusted operator or provider record confirms entitlement.
    Authorized,
    /// A trusted operator or provider record denies entitlement.
    Unauthorized,
}

/// On-wire numeric encoding evidence used before building `DecimalString` values.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericEncodingV1 {
    /// Missing encoding evidence is rejected by source validation.
    #[default]
    Unspecified,
    /// JSON or another wire format supplied an exact decimal token.
    DecimalToken,
    /// The source supplied a non-negative integer token.
    IntegerToken,
    /// A MessagePack float64 was projected to its shortest round-tripping decimal string.
    BinaryFloat64ShortestDecimal,
    /// A MessagePack float32 was projected to its shortest round-tripping decimal string.
    BinaryFloat32ShortestDecimal,
}

impl NumericEncodingV1 {
    /// Return the canonical lower-snake-case JSON spelling for this encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::DecimalToken => "decimal_token",
            Self::IntegerToken => "integer_token",
            Self::BinaryFloat64ShortestDecimal => "binary_float64_shortest_decimal",
            Self::BinaryFloat32ShortestDecimal => "binary_float32_shortest_decimal",
        }
    }

    fn is_binary_float_projection(self) -> bool {
        matches!(
            self,
            Self::BinaryFloat64ShortestDecimal | Self::BinaryFloat32ShortestDecimal
        )
    }
}

/// Exact source identity for one market event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MarketDataSourceV1 {
    /// Provider code such as alpaca or schwab.
    pub provider: String,
    /// Exact feed code such as sip, opra, indicative, or an explicitly named provider feed.
    pub feed: String,
    /// Entitlement status; defaults to unknown when old or incomplete evidence is read.
    #[serde(default)]
    pub entitlement: EntitlementState,
    /// Source numeric representation. `Unspecified` is invalid in a validated envelope.
    #[serde(default)]
    pub numeric_encoding: NumericEncodingV1,
    /// Optional provider-native record identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_record_id: Option<String>,
}

impl MarketDataSourceV1 {
    /// Create a source identity with bounded non-empty codes.
    pub fn new(
        provider: impl Into<String>,
        feed: impl Into<String>,
        entitlement: EntitlementState,
        numeric_encoding: NumericEncodingV1,
        source_record_id: Option<String>,
    ) -> Result<Self, MarketWireError> {
        let source = Self {
            provider: provider.into(),
            feed: feed.into(),
            entitlement,
            numeric_encoding,
            source_record_id,
        };
        source.validate()?;
        Ok(source)
    }

    /// Check source identifiers before an event enters a store or consumer.
    pub fn validate(&self) -> Result<(), MarketWireError> {
        let mentions_synthetic = self.provider.eq_ignore_ascii_case("synthetic")
            || self.feed.eq_ignore_ascii_case("synthetic");
        let is_canonical_synthetic = self.provider == "synthetic" && self.feed == "synthetic";
        if !valid_identifier(&self.provider, MAX_SOURCE_ID_BYTES)
            || !valid_identifier(&self.feed, MAX_SOURCE_ID_BYTES)
            || self
                .source_record_id
                .as_deref()
                .is_some_and(|value| !valid_identifier(value, MAX_SOURCE_ID_BYTES))
            || self.numeric_encoding == NumericEncodingV1::Unspecified
            || mentions_synthetic
                && (!is_canonical_synthetic
                    || self.numeric_encoding != NumericEncodingV1::DecimalToken)
        {
            return Err(MarketWireError::InvalidSource);
        }
        Ok(())
    }
}

/// Shared provenance and ordering metadata for data and control envelopes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventMetadataV1 {
    /// Contract version. Readers must reject unsupported non-zero versions.
    pub schema_version: u32,
    /// Provider and exact feed identity.
    pub source: MarketDataSourceV1,
    /// Positive local session generation.
    #[serde(with = "crate::wire_u64")]
    pub generation: u64,
    /// Positive sequence within the generation.
    #[serde(with = "crate::wire_u64")]
    pub sequence: u64,
    /// SHA-256 of the original raw provider frame, required for binary-float projections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_frame_sha256: Option<String>,
    /// Provider event time, absent when the source did not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_timestamp: Option<UtcTimestamp>,
    /// Local UTC time at which the event was received.
    pub received_timestamp: UtcTimestamp,
}

impl EventMetadataV1 {
    /// Validate version, source identity, and positive ordering identifiers.
    pub fn validate(&self) -> Result<(), MarketWireError> {
        if self.schema_version != MARKET_EVENT_SCHEMA_VERSION
            || self.generation == 0
            || self.sequence == 0
        {
            return Err(MarketWireError::InvalidSequence);
        }
        self.source.validate()?;
        if self
            .raw_frame_sha256
            .as_deref()
            .is_some_and(|value| !valid_sha256(value))
            || self.source.numeric_encoding.is_binary_float_projection()
                && self.raw_frame_sha256.is_none()
        {
            return Err(MarketWireError::InvalidSource);
        }
        Ok(())
    }
}

/// Market data event payload. Timestamps and source identity live in the envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketEventV1 {
    /// Sparse underlying quote update.
    StockQuote {
        symbol: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bid: Option<DecimalString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask: Option<DecimalString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bid_size: Option<DecimalString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask_size: Option<DecimalString>,
    },
    /// Underlying trade observation.
    StockTrade {
        symbol: String,
        price: DecimalString,
        size: DecimalString,
    },
    /// Sparse option quote update.
    OptionQuote {
        symbol: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bid: Option<DecimalString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask: Option<DecimalString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bid_size: Option<DecimalString>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask_size: Option<DecimalString>,
    },
    /// Option trade observation.
    OptionTrade {
        symbol: String,
        price: DecimalString,
        size: DecimalString,
    },
}

impl MarketEventV1 {
    /// Validate the event identity and numeric field constraints.
    pub fn validate(&self) -> Result<(), MarketWireError> {
        match self {
            Self::StockQuote {
                symbol,
                bid,
                ask,
                bid_size,
                ask_size,
            } => {
                validate_instrument(symbol)?;
                validate_quote(bid, ask, bid_size, ask_size)?;
            }
            Self::StockTrade {
                symbol,
                price,
                size,
            } => {
                validate_instrument(symbol)?;
                validate_trade_price(price)?;
                if size.parsed() <= ExactDecimal::ZERO {
                    return Err(MarketWireError::InvalidTradeSize);
                }
            }
            Self::OptionQuote {
                symbol,
                bid,
                ask,
                bid_size,
                ask_size,
            } => {
                parse_occ_symbol_candidate(symbol)?;
                validate_quote(bid, ask, bid_size, ask_size)?;
            }
            Self::OptionTrade {
                symbol,
                price,
                size,
            } => {
                parse_occ_symbol_candidate(symbol)?;
                validate_trade_price(price)?;
                if size.parsed() <= ExactDecimal::ZERO {
                    return Err(MarketWireError::InvalidTradeSize);
                }
            }
        }
        Ok(())
    }
}

/// Versioned market-data message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MarketEventEnvelopeV1 {
    /// Shared source, time, generation, and sequence evidence.
    #[serde(flatten)]
    pub metadata: EventMetadataV1,
    /// Sparse quote or trade payload.
    pub event: MarketEventV1,
}

impl MarketEventEnvelopeV1 {
    /// Validate envelope metadata and payload before forwarding or persistence.
    pub fn validate(&self) -> Result<(), MarketWireError> {
        self.metadata.validate()?;
        self.event.validate()
    }
}

/// Provider connection state, separate from subscription acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

/// One rejected instrument in a provider subscription response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RejectedInstrumentV1 {
    /// Instrument identity exactly as submitted to the provider.
    pub symbol: String,
    /// Stable provider or adapter rejection code.
    pub reason_code: String,
}

/// Backwards-compatible alias for the rejected-instrument DTO.
pub type InstrumentRejection = RejectedInstrumentV1;

/// Control event reported independently from quote/trade events.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketControlEventV1 {
    /// WebSocket or provider session state; this does not confirm a subscription.
    ConnectionStatus { state: ConnectionState },
    /// Provider-confirmed subscription result.
    SubscriptionAck {
        request_id: String,
        subscription_id: String,
        acknowledged: Vec<String>,
        rejected: Vec<RejectedInstrumentV1>,
    },
    /// Provider rejected a subscription request.
    SubscriptionRejected {
        request_id: String,
        reason_code: String,
    },
}

impl MarketControlEventV1 {
    /// Validate identifiers and require an actual provider response for acknowledgements.
    pub fn validate(&self) -> Result<(), MarketWireError> {
        match self {
            Self::ConnectionStatus { .. } => Ok(()),
            Self::SubscriptionAck {
                request_id,
                subscription_id,
                acknowledged,
                rejected,
            } => {
                if !valid_identifier(request_id, MAX_INSTRUMENT_ID_BYTES)
                    || !valid_identifier(subscription_id, MAX_INSTRUMENT_ID_BYTES)
                    || acknowledged.is_empty() && rejected.is_empty()
                    || acknowledged.len().saturating_add(rejected.len()) > MAX_CONTROL_INSTRUMENTS
                {
                    return Err(MarketWireError::InvalidAcknowledgement);
                }
                let mut identities = HashSet::with_capacity(acknowledged.len() + rejected.len());
                if acknowledged.iter().any(|symbol| {
                    !valid_identifier(symbol, MAX_INSTRUMENT_ID_BYTES)
                        || !identities.insert(symbol.as_str())
                }) || rejected.iter().any(|item| {
                    !valid_identifier(&item.symbol, MAX_INSTRUMENT_ID_BYTES)
                        || !valid_identifier(&item.reason_code, MAX_SOURCE_ID_BYTES)
                        || !identities.insert(item.symbol.as_str())
                }) {
                    return Err(MarketWireError::InvalidAcknowledgement);
                }
                Ok(())
            }
            Self::SubscriptionRejected {
                request_id,
                reason_code,
            } => {
                if !valid_identifier(request_id, MAX_INSTRUMENT_ID_BYTES)
                    || !valid_identifier(reason_code, MAX_SOURCE_ID_BYTES)
                {
                    return Err(MarketWireError::InvalidAcknowledgement);
                }
                Ok(())
            }
        }
    }
}

/// Versioned provider connection and subscription control message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ControlEventEnvelopeV1 {
    /// Shared source, time, generation, and sequence evidence.
    #[serde(flatten)]
    pub metadata: EventMetadataV1,
    /// Connection or provider-confirmed subscription response.
    pub control: MarketControlEventV1,
}

impl ControlEventEnvelopeV1 {
    /// Validate envelope metadata and control payload before forwarding.
    pub fn validate(&self) -> Result<(), MarketWireError> {
        self.metadata.validate()?;
        self.control.validate()
    }
}

/// Backwards-compatible name for [`ControlEventEnvelopeV1`].
pub type MarketControlEnvelopeV1 = ControlEventEnvelopeV1;

/// Parse an OCC option symbol as an unqualified contract candidate.
///
/// Provider feeds commonly omit the six-column root padding. This adapter restores that padding
/// before delegating all syntax, date, right, and exact-strike checks to the strong domain parser.
/// The returned value is only a candidate identity; it does not qualify multiplier, currency,
/// deliverable, trading class, exercise style, settlement, or provider evidence.
pub fn parse_occ_symbol_candidate(value: &str) -> Result<OptionSymbol, MarketWireError> {
    if !value.is_ascii() || value.len() <= 15 || value.len() > 21 {
        return Err(MarketWireError::InvalidInstrument);
    }
    if value.len() == 21 {
        return OptionSymbol::parse(value).map_err(|_| MarketWireError::InvalidInstrument);
    }
    let (root, suffix) = value.split_at(value.len() - 15);
    if root.is_empty() || root.len() > 6 || root.trim() != root {
        return Err(MarketWireError::InvalidInstrument);
    }
    let canonical = format!("{root:<6}{suffix}");
    OptionSymbol::parse(&canonical).map_err(|_| MarketWireError::InvalidInstrument)
}

fn canonical_utc_timestamp(value: &DateTime<Utc>) -> String {
    let mut formatted = value.to_rfc3339_opts(SecondsFormat::AutoSi, true);
    if let Some(decimal) = formatted.find('.') {
        let zulu = formatted.len() - 1;
        let mut end = zulu;
        while end > decimal + 1 && formatted.as_bytes()[end - 1] == b'0' {
            end -= 1;
        }
        if end == decimal + 1 {
            formatted.replace_range(decimal..zulu, "");
        } else if end < zulu {
            formatted.replace_range(end..zulu, "");
        }
    }
    formatted
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_quote(
    bid: &Option<DecimalString>,
    ask: &Option<DecimalString>,
    bid_size: &Option<DecimalString>,
    ask_size: &Option<DecimalString>,
) -> Result<(), MarketWireError> {
    if [bid, ask, bid_size, ask_size]
        .into_iter()
        .all(Option::is_none)
    {
        return Err(MarketWireError::InvalidQuote);
    }
    if [bid, ask]
        .into_iter()
        .flatten()
        .any(|price| price.parsed() < ExactDecimal::ZERO)
    {
        return Err(MarketWireError::InvalidPrice);
    }
    if [bid_size, ask_size]
        .into_iter()
        .flatten()
        .any(|size| size.parsed() < ExactDecimal::ZERO)
    {
        return Err(MarketWireError::InvalidQuoteSize);
    }
    Ok(())
}

fn validate_trade_price(price: &DecimalString) -> Result<(), MarketWireError> {
    if price.parsed() <= ExactDecimal::ZERO {
        Err(MarketWireError::InvalidPrice)
    } else {
        Ok(())
    }
}

fn validate_instrument(symbol: &str) -> Result<(), MarketWireError> {
    if valid_identifier(symbol, MAX_INSTRUMENT_ID_BYTES) {
        Ok(())
    } else {
        Err(MarketWireError::InvalidInstrument)
    }
}

pub(crate) fn valid_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= max_bytes
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionState, DecimalString, EntitlementState, EventMetadataV1, MAX_CONTROL_INSTRUMENTS,
        MAX_MARKET_FRAME_BYTES, MarketControlEnvelopeV1, MarketControlEventV1, MarketDataSourceV1,
        MarketEventEnvelopeV1, MarketEventV1, MarketWireError, NumericEncodingV1, UtcTimestamp,
        parse_occ_symbol_candidate, validate_market_frame_len,
    };

    #[test]
    fn numeric_encoding_exposes_canonical_wire_names() {
        assert_eq!(NumericEncodingV1::Unspecified.as_str(), "unspecified");
        assert_eq!(NumericEncodingV1::DecimalToken.as_str(), "decimal_token");
        assert_eq!(NumericEncodingV1::IntegerToken.as_str(), "integer_token");
        assert_eq!(
            NumericEncodingV1::BinaryFloat64ShortestDecimal.as_str(),
            "binary_float64_shortest_decimal"
        );
        assert_eq!(
            NumericEncodingV1::BinaryFloat32ShortestDecimal.as_str(),
            "binary_float32_shortest_decimal"
        );
    }

    fn metadata() -> EventMetadataV1 {
        EventMetadataV1 {
            schema_version: 1,
            source: MarketDataSourceV1::new(
                "synthetic",
                "synthetic",
                EntitlementState::Unknown,
                NumericEncodingV1::DecimalToken,
                Some("synthetic-record-1".to_owned()),
            )
            .unwrap(),
            generation: 3,
            sequence: 19,
            raw_frame_sha256: None,
            source_timestamp: Some(UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap()),
            received_timestamp: UtcTimestamp::parse("2026-10-08T14:30:00.000000123Z").unwrap(),
        }
    }

    #[test]
    fn metadata_json_uint64_projection_is_lossless_and_string_only() {
        let mut value = metadata();
        value.generation = u64::MAX;
        value.sequence = u64::MAX;

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["generation"], u64::MAX.to_string());
        assert_eq!(json["sequence"], u64::MAX.to_string());
        let decoded: EventMetadataV1 = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.generation, u64::MAX);
        assert_eq!(decoded.sequence, u64::MAX);

        let numeric = r#"{"schema_version":1,"source":{"provider":"synthetic","feed":"synthetic","entitlement":"unknown","numeric_encoding":"decimal_token","source_record_id":"synthetic-record-1"},"generation":9007199254740993,"sequence":"1","source_timestamp":"2026-10-08T14:30:00Z","received_timestamp":"2026-10-08T14:30:00Z"}"#;
        assert!(serde_json::from_str::<EventMetadataV1>(numeric).is_err());
        let leading_zero = numeric.replace("9007199254740993", "\"01\"");
        assert!(serde_json::from_str::<EventMetadataV1>(&leading_zero).is_err());
        let overflow = numeric.replace("9007199254740993", "\"18446744073709551616\"");
        assert!(serde_json::from_str::<EventMetadataV1>(&overflow).is_err());
    }

    #[test]
    fn decimal_strings_validate_exact_values_and_preserve_source_text() {
        let amount = DecimalString::new("12.3400").unwrap();
        assert_eq!(amount.as_str(), "12.3400");
        assert_eq!(serde_json::to_string(&amount).unwrap(), "\"12.3400\"");
        assert!(serde_json::from_str::<DecimalString>("\"1e3\"").is_ok());
        assert!(serde_json::from_str::<DecimalString>("\"NaN\"").is_err());
        assert!(serde_json::from_str::<DecimalString>("\"01.0\"").is_err());
        assert!(DecimalString::new("170141183460469231731687303715884105728").is_err());
    }

    #[test]
    fn timestamps_require_utc_and_normalize_fractional_seconds() {
        let timestamp = UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap();
        assert_eq!(timestamp.as_str(), "2026-10-08T14:30:00Z");
        assert_eq!(
            UtcTimestamp::parse("2026-10-08T14:30:00.123400000Z")
                .unwrap()
                .as_str(),
            "2026-10-08T14:30:00.1234Z"
        );
        assert!(UtcTimestamp::parse("2026-10-08T22:30:00+08:00").is_err());
    }

    #[test]
    fn timestamp_order_uses_instants_not_canonical_text() {
        let whole = UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap();
        let one_nanosecond = UtcTimestamp::parse("2026-10-08T14:30:00.000000001Z").unwrap();
        let one_tenth = UtcTimestamp::parse("2026-10-08T14:30:00.1Z").unwrap();
        let eleven_hundredths = UtcTimestamp::parse("2026-10-08T14:30:00.11Z").unwrap();
        let same_instant = UtcTimestamp::parse("2026-10-08T14:30:00.100000000Z").unwrap();

        assert!(whole < one_nanosecond);
        assert!(one_tenth < eleven_hundredths);
        assert_eq!(one_tenth, same_instant);
        assert_eq!(one_tenth.as_str(), "2026-10-08T14:30:00.1Z");
    }

    #[test]
    fn event_envelope_keeps_feed_time_and_ordering_evidence() {
        let envelope = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::OptionQuote {
                symbol: "QQQ261016C00600000".to_owned(),
                bid: Some(DecimalString::new("1.25").unwrap()),
                ask: None,
                bid_size: Some(DecimalString::new("4").unwrap()),
                ask_size: None,
            },
        };
        envelope.validate().unwrap();
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["source"]["provider"], "synthetic");
        assert_eq!(json["source"]["feed"], "synthetic");
        assert_eq!(json["source"]["entitlement"], "unknown");
        assert_eq!(json["source"]["numeric_encoding"], "decimal_token");
        assert_eq!(json["generation"], "3");
        assert_eq!(json["sequence"], "19");
        assert_eq!(json["source_timestamp"], "2026-10-08T14:30:00Z");
        assert_eq!(json["received_timestamp"], "2026-10-08T14:30:00.000000123Z");
        assert!(json["event"].get("ask").is_none());
    }

    #[test]
    fn event_validation_rejects_zero_sequence_and_nonpositive_trade_size() {
        let mut no_sequence = metadata();
        no_sequence.sequence = 0;
        let envelope = MarketEventEnvelopeV1 {
            metadata: no_sequence,
            event: MarketEventV1::StockTrade {
                symbol: "QQQ".to_owned(),
                price: DecimalString::new("600").unwrap(),
                size: DecimalString::new("1").unwrap(),
            },
        };
        assert_eq!(envelope.validate(), Err(MarketWireError::InvalidSequence));

        let envelope = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::StockTrade {
                symbol: "QQQ".to_owned(),
                price: DecimalString::new("600").unwrap(),
                size: DecimalString::new("0").unwrap(),
            },
        };
        assert_eq!(envelope.validate(), Err(MarketWireError::InvalidTradeSize));
    }

    #[test]
    fn binary_float_projection_requires_original_frame_hash_evidence() {
        let mut value = metadata();
        value.source.provider = "alpaca".to_owned();
        value.source.feed = "opra".to_owned();
        value.source.numeric_encoding = NumericEncodingV1::BinaryFloat64ShortestDecimal;
        assert_eq!(value.validate(), Err(MarketWireError::InvalidSource));

        value.raw_frame_sha256 = Some("a".repeat(64));
        value.validate().unwrap();

        value.raw_frame_sha256 = Some("A".repeat(64));
        assert_eq!(value.validate(), Err(MarketWireError::InvalidSource));
    }

    #[test]
    fn synthetic_source_identity_is_explicit_and_canonical() {
        assert!(
            MarketDataSourceV1::new(
                "synthetic",
                "sip",
                EntitlementState::Unknown,
                NumericEncodingV1::DecimalToken,
                None,
            )
            .is_err()
        );
        assert!(
            MarketDataSourceV1::new(
                "alpaca",
                "synthetic",
                EntitlementState::Unknown,
                NumericEncodingV1::DecimalToken,
                None,
            )
            .is_err()
        );
        assert!(
            MarketDataSourceV1::new(
                "Synthetic",
                "synthetic",
                EntitlementState::Unknown,
                NumericEncodingV1::DecimalToken,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn sparse_quotes_allow_zero_but_reject_empty_negative_and_unrepresentable_values() {
        let sparse = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::StockQuote {
                symbol: "QQQ".to_owned(),
                bid: Some(DecimalString::new("0").unwrap()),
                ask: None,
                bid_size: Some(DecimalString::new("0").unwrap()),
                ask_size: None,
            },
        };
        sparse.validate().unwrap();

        let empty = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::StockQuote {
                symbol: "QQQ".to_owned(),
                bid: None,
                ask: None,
                bid_size: None,
                ask_size: None,
            },
        };
        assert_eq!(empty.validate(), Err(MarketWireError::InvalidQuote));

        let negative = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::OptionQuote {
                symbol: "QQQ261016C00600000".to_owned(),
                bid: Some(DecimalString::new("-0.01").unwrap()),
                ask: None,
                bid_size: None,
                ask_size: None,
            },
        };
        assert_eq!(negative.validate(), Err(MarketWireError::InvalidPrice));

        let negative_size = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::StockQuote {
                symbol: "QQQ".to_owned(),
                bid: None,
                ask: None,
                bid_size: Some(DecimalString::new("-1").unwrap()),
                ask_size: None,
            },
        };
        assert_eq!(
            negative_size.validate(),
            Err(MarketWireError::InvalidQuoteSize)
        );
    }

    #[test]
    fn trades_require_positive_prices_and_exact_occ_candidate_syntax() {
        let zero_price = MarketEventEnvelopeV1 {
            metadata: metadata(),
            event: MarketEventV1::StockTrade {
                symbol: "QQQ".to_owned(),
                price: DecimalString::new("0").unwrap(),
                size: DecimalString::new("1").unwrap(),
            },
        };
        assert_eq!(zero_price.validate(), Err(MarketWireError::InvalidPrice));

        assert_eq!(
            parse_occ_symbol_candidate("QQQ261016C00600000")
                .unwrap()
                .format(),
            "QQQ   261016C00600000"
        );
        assert_eq!(
            parse_occ_symbol_candidate("QQQ   261016C00600000")
                .unwrap()
                .format(),
            "QQQ   261016C00600000"
        );
        assert_eq!(
            parse_occ_symbol_candidate("QQQ261032C00600000"),
            Err(MarketWireError::InvalidInstrument)
        );
    }

    #[test]
    fn connection_state_does_not_imply_subscription_ack() {
        let control = MarketControlEnvelopeV1 {
            metadata: metadata(),
            control: MarketControlEventV1::ConnectionStatus {
                state: ConnectionState::Connected,
            },
        };
        control.validate().unwrap();

        let ack = MarketControlEnvelopeV1 {
            metadata: metadata(),
            control: MarketControlEventV1::SubscriptionAck {
                request_id: "request-1".to_owned(),
                subscription_id: "subscription-1".to_owned(),
                acknowledged: vec!["QQQ261016C00600000".to_owned()],
                rejected: Vec::new(),
            },
        };
        ack.validate().unwrap();
        let json = serde_json::to_value(ack).unwrap();
        assert_eq!(json["control"]["kind"], "subscription_ack");
        assert_eq!(json["control"]["acknowledged"][0], "QQQ261016C00600000");
    }

    #[test]
    fn acknowledgements_reject_duplicates_overlap_and_excess_capacity() {
        let duplicate = MarketControlEventV1::SubscriptionAck {
            request_id: "request-1".to_owned(),
            subscription_id: "subscription-1".to_owned(),
            acknowledged: vec!["QQQ".to_owned(), "QQQ".to_owned()],
            rejected: Vec::new(),
        };
        assert_eq!(
            duplicate.validate(),
            Err(MarketWireError::InvalidAcknowledgement)
        );

        let overlap = MarketControlEventV1::SubscriptionAck {
            request_id: "request-1".to_owned(),
            subscription_id: "subscription-1".to_owned(),
            acknowledged: vec!["QQQ".to_owned()],
            rejected: vec![super::RejectedInstrumentV1 {
                symbol: "QQQ".to_owned(),
                reason_code: "not_authorized".to_owned(),
            }],
        };
        assert_eq!(
            overlap.validate(),
            Err(MarketWireError::InvalidAcknowledgement)
        );

        let over_capacity = MarketControlEventV1::SubscriptionAck {
            request_id: "request-1".to_owned(),
            subscription_id: "subscription-1".to_owned(),
            acknowledged: (0..=MAX_CONTROL_INSTRUMENTS)
                .map(|index| format!("QQQ{index}"))
                .collect(),
            rejected: Vec::new(),
        };
        assert_eq!(
            over_capacity.validate(),
            Err(MarketWireError::InvalidAcknowledgement)
        );
    }

    #[test]
    fn adapters_can_reject_oversized_raw_frames_before_decode() {
        assert!(validate_market_frame_len(MAX_MARKET_FRAME_BYTES).is_ok());
        assert_eq!(
            validate_market_frame_len(MAX_MARKET_FRAME_BYTES + 1),
            Err(MarketWireError::FrameTooLarge)
        );
    }
}
