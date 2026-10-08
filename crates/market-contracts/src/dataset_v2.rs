//! Version 2 immutable dataset manifests and typed completion evidence.
//!
//! Validation here is structural. Hash fields identify bytes but do not prove that a provider
//! issued them, that a feed was entitled, or that a stream watermark is truthful. A trusted
//! composition root must separately verify source receipts before any research admission.

use crate::{
    DatasetManifestError, DatasetObjectV1, DatasetTimeRangeV1, DatasetTransportV1,
    EntitlementState, MarketDataSourceV1, NumericEncodingV1, UtcTimestamp,
    dataset::{stable_identity, valid_sha256, validate_dataset_manifest_fields},
};
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

/// Wire schema version accepted by [`DatasetManifestV2`].
pub const DATASET_MANIFEST_SCHEMA_VERSION_V2: u32 = 2;
/// Maximum JSON document size accepted by the bounded V2 parser.
pub const MAX_DATASET_MANIFEST_V2_JSON_BYTES: usize = 2 * 1024 * 1024;
/// Maximum sorted symbol identities accepted by one V2 manifest.
pub const MAX_DATASET_MANIFEST_V2_SYMBOLS: usize = 4096;
/// Maximum allowed lateness represented by a provider-watermark observation.
pub const MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS: u64 = 60_000_000_000;

/// Errors at the bounded JSON and versioned-manifest boundary.
#[derive(Debug, Error)]
pub enum DatasetManifestV2Error {
    /// JSON input exceeded the fixed limit before parsing began.
    #[error("dataset manifest v2 JSON exceeds the configured byte limit")]
    JsonTooLarge,
    /// JSON was malformed, contained duplicate fields, or used an unknown field.
    #[error("invalid dataset manifest v2 JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    /// A protobuf Timestamp was outside its supported range or malformed.
    #[error("invalid protobuf UTC timestamp")]
    InvalidTimestamp,
    /// A parsed manifest violated a dataset or completion invariant.
    #[error(transparent)]
    InvalidManifest(#[from] DatasetManifestError),
}

/// A protobuf Timestamp carried as a canonical UTC ProtoJSON string without losing nanoseconds.
#[derive(Clone, Debug)]
pub struct ProtoTimestampV2 {
    instant: DateTime<Utc>,
    canonical: String,
}

impl ProtoTimestampV2 {
    /// Parse an RFC3339 timestamp, normalize it to UTC, and retain nanosecond precision.
    pub fn parse(value: &str) -> Result<Self, DatasetManifestV2Error> {
        if !valid_proto_timestamp_v2_input(value) {
            return Err(DatasetManifestV2Error::InvalidTimestamp);
        }
        let parsed = DateTime::parse_from_rfc3339(value)
            .map_err(|_| DatasetManifestV2Error::InvalidTimestamp)?;
        let instant = parsed.with_timezone(&Utc);
        if !(1..=9999).contains(&instant.year()) {
            return Err(DatasetManifestV2Error::InvalidTimestamp);
        }
        let canonical = canonical_proto_timestamp(&instant);
        Ok(Self { instant, canonical })
    }

    /// Return the canonical UTC ProtoJSON string.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    fn to_v1_timestamp(&self) -> Result<UtcTimestamp, DatasetManifestV2Error> {
        UtcTimestamp::parse(&self.canonical).map_err(|_| DatasetManifestV2Error::InvalidTimestamp)
    }
}

fn valid_proto_timestamp_v2_input(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
        || !bytes[11..13].iter().all(u8::is_ascii_digit)
        || !bytes[14..16].iter().all(u8::is_ascii_digit)
        || !bytes[17..19].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let Some(hour) = two_ascii_digits(&bytes[11..13]) else {
        return false;
    };
    let Some(minute) = two_ascii_digits(&bytes[14..16]) else {
        return false;
    };
    let Some(second) = two_ascii_digits(&bytes[17..19]) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 59 {
        return false;
    }

    let mut zone_start = 19;
    if bytes.get(zone_start) == Some(&b'.') {
        zone_start += 1;
        let fraction_start = zone_start;
        while bytes.get(zone_start).is_some_and(u8::is_ascii_digit) {
            zone_start += 1;
        }
        let fraction_digits = zone_start - fraction_start;
        if !(1..=9).contains(&fraction_digits) {
            return false;
        }
    }

    let zone = &bytes[zone_start..];
    if zone == b"Z" {
        return true;
    }
    if zone.len() != 6 || (zone[0] != b'+' && zone[0] != b'-') || zone[3] != b':' {
        return false;
    }
    let Some(offset_hour) = two_ascii_digits(&zone[1..3]) else {
        return false;
    };
    let Some(offset_minute) = two_ascii_digits(&zone[4..6]) else {
        return false;
    };
    offset_hour <= 23 && offset_minute <= 59
}

fn two_ascii_digits(value: &[u8]) -> Option<u8> {
    let [tens, ones] = value else {
        return None;
    };
    if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
        return None;
    }
    Some((tens - b'0') * 10 + ones - b'0')
}

impl PartialEq for ProtoTimestampV2 {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant
    }
}

impl Eq for ProtoTimestampV2 {}

impl PartialOrd for ProtoTimestampV2 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProtoTimestampV2 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.instant.cmp(&other.instant)
    }
}

impl Serialize for ProtoTimestampV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical)
    }
}

impl<'de> Deserialize<'de> for ProtoTimestampV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

fn canonical_proto_timestamp(instant: &DateTime<Utc>) -> String {
    if instant.timestamp_subsec_nanos() == 0 {
        return instant.to_rfc3339_opts(SecondsFormat::Secs, true);
    }
    let nanos = instant.timestamp_subsec_nanos();
    let width = if nanos.is_multiple_of(1_000_000) {
        3
    } else if nanos.is_multiple_of(1_000) {
        6
    } else {
        9
    };
    let formatted = instant.to_rfc3339_opts(SecondsFormat::Nanos, true);
    let decimal = formatted.find('.').expect("non-zero nanos use a fraction");
    let suffix = decimal + 1 + 9;
    format!(
        "{}{}{}",
        &formatted[..decimal],
        &formatted[decimal..decimal + 1 + width],
        &formatted[suffix..]
    )
}

/// Exact provider identity projection used by the V2 JSON contract.
///
/// Protobuf enum names are retained on the wire; conversion to the existing V1 domain value is
/// explicit so there is one source validator and no second market-domain authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetSourceV2 {
    /// Provider identifier.
    pub provider: String,
    /// Exact feed identifier.
    pub feed: String,
    /// Entitlement observation; it does not itself prove authorization.
    pub entitlement: EntitlementState,
    /// Protobuf enum name describing the source numeric representation.
    #[serde(alias = "numeric_encoding")]
    pub numeric_encoding: NumericEncodingProtoJsonV2,
    /// Optional provider-native source record identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "source_record_id"
    )]
    pub source_record_id: Option<String>,
}

impl DatasetSourceV2 {
    fn to_v1(&self) -> MarketDataSourceV1 {
        MarketDataSourceV1 {
            provider: self.provider.clone(),
            feed: self.feed.clone(),
            entitlement: self.entitlement,
            numeric_encoding: self.numeric_encoding.into(),
            source_record_id: self.source_record_id.clone(),
        }
    }
}

/// Numeric source encodings in the V1 Protobuf enum, represented by their ProtoJSON names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NumericEncodingProtoJsonV2 {
    /// Unspecified is present in the protobuf enum but rejected during validation.
    #[serde(rename = "NUMERIC_ENCODING_UNSPECIFIED")]
    Unspecified,
    /// Source supplied an exact decimal token.
    #[serde(rename = "NUMERIC_ENCODING_DECIMAL_TOKEN")]
    DecimalToken,
    /// Source supplied a non-negative integer token.
    #[serde(rename = "NUMERIC_ENCODING_INTEGER_TOKEN")]
    IntegerToken,
    /// Binary float64 projected to shortest round-tripping decimal.
    #[serde(rename = "NUMERIC_ENCODING_BINARY_FLOAT64_SHORTEST_DECIMAL")]
    BinaryFloat64ShortestDecimal,
    /// Binary float32 projected to shortest round-tripping decimal.
    #[serde(rename = "NUMERIC_ENCODING_BINARY_FLOAT32_SHORTEST_DECIMAL")]
    BinaryFloat32ShortestDecimal,
    /// Raw MessagePack byte object, never a normalized numeric event.
    #[serde(rename = "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES")]
    RawMessagePackBytes,
    /// Raw JSON byte object, never a normalized numeric event.
    #[serde(rename = "NUMERIC_ENCODING_RAW_JSON_BYTES")]
    RawJsonBytes,
}

impl From<NumericEncodingProtoJsonV2> for NumericEncodingV1 {
    fn from(value: NumericEncodingProtoJsonV2) -> Self {
        match value {
            NumericEncodingProtoJsonV2::Unspecified => Self::Unspecified,
            NumericEncodingProtoJsonV2::DecimalToken => Self::DecimalToken,
            NumericEncodingProtoJsonV2::IntegerToken => Self::IntegerToken,
            NumericEncodingProtoJsonV2::BinaryFloat64ShortestDecimal => {
                Self::BinaryFloat64ShortestDecimal
            }
            NumericEncodingProtoJsonV2::BinaryFloat32ShortestDecimal => {
                Self::BinaryFloat32ShortestDecimal
            }
            NumericEncodingProtoJsonV2::RawMessagePackBytes => Self::RawMessagePackBytes,
            NumericEncodingProtoJsonV2::RawJsonBytes => Self::RawJsonBytes,
        }
    }
}

/// Source-time coverage for a V2 manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetTimeRangeV2 {
    /// Earliest represented source timestamp, inclusive.
    #[serde(alias = "start_inclusive")]
    pub start_inclusive: ProtoTimestampV2,
    /// Exclusive upper bound after the latest represented source timestamp.
    #[serde(alias = "end_exclusive")]
    pub end_exclusive: ProtoTimestampV2,
}

impl DatasetTimeRangeV2 {
    fn to_v1(&self) -> Result<DatasetTimeRangeV1, DatasetManifestV2Error> {
        Ok(DatasetTimeRangeV1 {
            start_inclusive: self.start_inclusive.to_v1_timestamp()?,
            end_exclusive: self.end_exclusive.to_v1_timestamp()?,
        })
    }
}

/// A single immutable Parquet object referenced by a V2 manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetObjectV2 {
    /// Stable object basename, not a directory path.
    #[serde(alias = "object_name")]
    pub object_name: String,
    /// Opaque provider identity or explicit `local-test:` identity.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "object_id")]
    pub object_id: Option<String>,
    /// Exact stored object size in bytes.
    #[serde(with = "crate::wire_u64", alias = "size_bytes")]
    pub size_bytes: u64,
    /// SHA-256 of the complete stored object.
    #[serde(alias = "content_sha256")]
    pub content_sha256: String,
    /// SHA-256 fingerprint of the Parquet schema.
    #[serde(alias = "parquet_schema_sha256")]
    pub parquet_schema_sha256: String,
    /// Row count read from the Parquet footer.
    #[serde(with = "crate::wire_u64", alias = "parquet_footer_rows")]
    pub parquet_footer_rows: u64,
    /// Storage transport identity.
    pub transport: DatasetTransportV1,
}

impl DatasetObjectV2 {
    fn to_v1(&self) -> DatasetObjectV1 {
        DatasetObjectV1 {
            object_name: self.object_name.clone(),
            object_id: self.object_id.clone(),
            size_bytes: self.size_bytes,
            content_sha256: self.content_sha256.clone(),
            parquet_schema_sha256: self.parquet_schema_sha256.clone(),
            parquet_footer_rows: self.parquet_footer_rows,
            transport: self.transport,
        }
    }
}

/// Storage readback evidence, deliberately separate from source completion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetStorageVerificationV2 {
    /// SHA-256 measured by reading the complete stored object back.
    #[serde(alias = "readback_sha256")]
    pub readback_sha256: String,
    /// True only when the complete object was read back and verified before publication.
    #[serde(alias = "verified_before_publish")]
    pub verified_before_publish: bool,
}

/// Finite input kinds whose exhaustion can be recorded without claiming provider truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FiniteBatchSourceKindV2 {
    /// Protobuf default; invalid for published manifests.
    #[serde(rename = "FINITE_BATCH_SOURCE_KIND_UNSPECIFIED")]
    Unspecified,
    /// Finite replay of explicitly synthetic source data.
    #[serde(rename = "FINITE_BATCH_SOURCE_KIND_SYNTHETIC_REPLAY")]
    SyntheticReplay,
    /// A finite paged historical query whose page set was fully exhausted.
    #[serde(rename = "FINITE_BATCH_SOURCE_KIND_HISTORICAL_PAGED")]
    HistoricalPaged,
    /// A finite non-paginated historical response.
    #[serde(rename = "FINITE_BATCH_SOURCE_KIND_HISTORICAL_NON_PAGED")]
    HistoricalNonPaged,
    /// An immutable local archive was completely reread.
    #[serde(rename = "FINITE_BATCH_SOURCE_KIND_LOCAL_ARCHIVE")]
    LocalArchive,
}

/// Completion evidence for one finite, byte-identified input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FiniteBatchCompletionV2 {
    /// Finite input provenance category; this never certifies provider history completeness.
    #[serde(alias = "source_kind")]
    pub source_kind: FiniteBatchSourceKindV2,
    /// Immutable logical identity for the exact input; never a filesystem path.
    #[serde(alias = "input_identity")]
    pub input_identity: String,
    /// SHA-256 of the exact consumed input bytes.
    #[serde(alias = "input_sha256")]
    pub input_sha256: String,
    /// Exact input byte count.
    #[serde(with = "crate::wire_u64", alias = "input_size_bytes")]
    pub input_size_bytes: u64,
    /// Record count asserted by the immutable input receipt.
    #[serde(with = "crate::wire_u64", alias = "input_record_count")]
    pub input_record_count: u64,
    /// Records actually consumed by the finite processor.
    #[serde(with = "crate::wire_u64", alias = "consumed_record_count")]
    pub consumed_record_count: u64,
    /// SHA-256 of the reviewed finite-input policy.
    #[serde(alias = "reviewed_policy_sha256")]
    pub reviewed_policy_sha256: String,
    /// SHA-256 of the receipt binding the exact finite input and completed processing.
    #[serde(alias = "seal_receipt_sha256")]
    pub seal_receipt_sha256: String,
    /// Exclusive requested data cutoff, not a provider watermark.
    #[serde(alias = "data_cutoff_exclusive")]
    pub data_cutoff_exclusive: ProtoTimestampV2,
    /// Time when the finite input was sealed after consumption.
    #[serde(alias = "sealed_at")]
    pub sealed_at: ProtoTimestampV2,
    /// Time when output generation completed.
    #[serde(alias = "completed_at")]
    pub completed_at: ProtoTimestampV2,
    /// Number of pages in a paginated query; present only for historical paged input.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_wire_u64",
        alias = "page_count"
    )]
    pub page_count: Option<u64>,
    /// Actual provider/page-reader exhaustion result; must be present and true for paginated input.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "pages_exhausted"
    )]
    pub pages_exhausted: Option<bool>,
    /// SHA-256 receipt of the exact ordered page set; present only for paginated input.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "page_set_sha256"
    )]
    pub page_set_sha256: Option<String>,
}

/// Provider stream watermark observation. Parsing this value never mints a trusted watermark.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderWatermarkCompletionV2 {
    /// Provider identifier; must match manifest source identity.
    pub provider: String,
    /// Feed identifier; must match manifest source identity.
    pub feed: String,
    /// Immutable subscription instance identity that scopes generation across restarts.
    #[serde(alias = "subscription_instance_id")]
    pub subscription_instance_id: String,
    /// Monotonic generation within the subscription instance, not across process restarts.
    #[serde(with = "crate::wire_u64")]
    pub generation: u64,
    /// First sequence covered by the continuity receipt, inclusive.
    #[serde(alias = "first_sequence", with = "crate::wire_u64")]
    pub first_sequence: u64,
    /// Last sequence covered by the continuity receipt, inclusive.
    #[serde(alias = "last_sequence", with = "crate::wire_u64")]
    pub last_sequence: u64,
    /// Number of contiguous sequence values covered by the receipt.
    #[serde(alias = "sequence_count", with = "crate::wire_u64")]
    pub sequence_count: u64,
    /// SHA-256 receipt for the contiguous sequence interval.
    #[serde(alias = "continuity_receipt_sha256")]
    pub continuity_receipt_sha256: String,
    /// Exclusive timestamp through which the trusted source claims completeness.
    #[serde(alias = "complete_up_to_exclusive")]
    pub complete_up_to_exclusive: ProtoTimestampV2,
    /// Explicit allowed lateness; values above the V2 60-second limit are rejected, never clamped.
    #[serde(with = "crate::wire_u64", alias = "allowed_lateness_ns")]
    pub allowed_lateness_ns: u64,
    /// SHA-256 of the reviewed watermark policy.
    #[serde(alias = "reviewed_policy_sha256")]
    pub reviewed_policy_sha256: String,
    /// SHA-256 receipt issued by the provider/source adapter.
    #[serde(alias = "source_receipt_sha256")]
    pub source_receipt_sha256: String,
}

/// Diagnostic stream observation. It has no provider-completeness cutoff.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticStreamCompletionV2 {
    /// Immutable local source/session identity.
    #[serde(alias = "source_instance_id")]
    pub source_instance_id: String,
    /// Generation scoped to the source instance.
    #[serde(with = "crate::wire_u64")]
    pub generation: u64,
    /// Highest observed sequence; this is not a complete cursor.
    #[serde(with = "crate::wire_u64", alias = "observed_last_sequence")]
    pub observed_last_sequence: u64,
    /// Local policy cutoff for diagnostics only.
    #[serde(alias = "local_policy_cutoff")]
    pub local_policy_cutoff: ProtoTimestampV2,
    /// Maximum observed source timestamp, absent if the source supplied none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "observed_max_source_timestamp"
    )]
    pub observed_max_source_timestamp: Option<ProtoTimestampV2>,
    /// SHA-256 of the local diagnostic policy.
    #[serde(alias = "diagnostic_policy_sha256")]
    pub diagnostic_policy_sha256: String,
    /// SHA-256 of the diagnostic receipt.
    #[serde(alias = "diagnostic_receipt_sha256")]
    pub diagnostic_receipt_sha256: String,
}

/// Protobuf oneof projection. Exactly one case must be present.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetCompletionEvidenceV2 {
    /// Sealed finite-input evidence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "finite_batch"
    )]
    pub finite_batch: Option<FiniteBatchCompletionV2>,
    /// Provider watermark observation, always unqualified until a fixed trusted verifier checks it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "provider_watermark"
    )]
    pub provider_watermark: Option<ProviderWatermarkCompletionV2>,
    /// Local diagnostic stream evidence, never complete-provider evidence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "diagnostic_stream"
    )]
    pub diagnostic_stream: Option<DiagnosticStreamCompletionV2>,
}

impl DatasetCompletionEvidenceV2 {
    fn validate(&self, source: &MarketDataSourceV1, manifest: &DatasetManifestV2) -> bool {
        match (
            self.finite_batch.as_ref(),
            self.provider_watermark.as_ref(),
            self.diagnostic_stream.as_ref(),
        ) {
            (Some(value), None, None) => value.validate(source, manifest),
            (None, Some(value), None) => value.validate(source, manifest),
            (None, None, Some(value)) => value.validate(),
            _ => false,
        }
    }
}

impl FiniteBatchCompletionV2 {
    fn validate(&self, source: &MarketDataSourceV1, manifest: &DatasetManifestV2) -> bool {
        if self.source_kind == FiniteBatchSourceKindV2::Unspecified
            || !stable_identity(&self.input_identity, 256)
            || !valid_sha256(&self.input_sha256)
            || self.input_size_bytes == 0
            || self.input_record_count == 0
            || self.input_record_count != self.consumed_record_count
            || !valid_sha256(&self.reviewed_policy_sha256)
            || !valid_sha256(&self.seal_receipt_sha256)
            || self.data_cutoff_exclusive > self.sealed_at
            || self.sealed_at > self.completed_at
            || manifest
                .time_range
                .as_ref()
                .is_some_and(|range| range.end_exclusive > self.data_cutoff_exclusive)
        {
            return false;
        }

        let paged = self.source_kind == FiniteBatchSourceKindV2::HistoricalPaged;
        let page_evidence_valid = if paged {
            matches!(self.page_count, Some(value) if value > 0)
                && self.pages_exhausted == Some(true)
                && self.page_set_sha256.as_deref().is_some_and(valid_sha256)
        } else {
            self.page_count.is_none()
                && self.pages_exhausted.is_none()
                && self.page_set_sha256.is_none()
        };
        if !page_evidence_valid {
            return false;
        }

        let is_synthetic_source = source.provider == "synthetic" && source.feed == "synthetic";
        match self.source_kind {
            FiniteBatchSourceKindV2::SyntheticReplay => is_synthetic_source,
            FiniteBatchSourceKindV2::HistoricalPaged
            | FiniteBatchSourceKindV2::HistoricalNonPaged => !is_synthetic_source,
            FiniteBatchSourceKindV2::LocalArchive => true,
            FiniteBatchSourceKindV2::Unspecified => false,
        }
    }
}

impl ProviderWatermarkCompletionV2 {
    fn validate(&self, source: &MarketDataSourceV1, manifest: &DatasetManifestV2) -> bool {
        let sequence_span = self
            .last_sequence
            .checked_sub(self.first_sequence)
            .and_then(|span| span.checked_add(1));
        let covers_manifest_time = manifest
            .time_range
            .as_ref()
            .is_some_and(|range| range.end_exclusive <= self.complete_up_to_exclusive);
        self.provider == source.provider
            && self.feed == source.feed
            && stable_identity(&self.subscription_instance_id, 256)
            && self.generation > 0
            && self.first_sequence > 0
            && sequence_span == Some(self.sequence_count)
            && self.sequence_count > 0
            && valid_sha256(&self.continuity_receipt_sha256)
            && self.allowed_lateness_ns <= MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS
            && valid_sha256(&self.reviewed_policy_sha256)
            && valid_sha256(&self.source_receipt_sha256)
            && self.provider != "synthetic"
            && self.feed != "synthetic"
            && manifest.source_timestamp_missing_rows == 0
            && covers_manifest_time
    }
}

impl DiagnosticStreamCompletionV2 {
    fn validate(&self) -> bool {
        stable_identity(&self.source_instance_id, 256)
            && self.generation > 0
            && self.observed_last_sequence > 0
            && valid_sha256(&self.diagnostic_policy_sha256)
            && valid_sha256(&self.diagnostic_receipt_sha256)
    }
}

/// Immutable dataset manifest V2 with storage and input/stream evidence separated.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetManifestV2 {
    /// This contract accepts only version 2.
    #[serde(alias = "schema_version")]
    pub schema_version: u32,
    /// Immutable logical dataset version identity, never a path.
    #[serde(alias = "dataset_id")]
    pub dataset_id: String,
    /// Exact source identity; `entitlement` remains evidence, not authorization.
    pub source: DatasetSourceV2,
    /// Exact symbol set, sorted lexically by UTF-8 bytes and without duplicates.
    pub symbols: Vec<String>,
    /// Optional half-open source-time interval.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "time_range")]
    pub time_range: Option<DatasetTimeRangeV2>,
    /// Number of rows whose source timestamp was absent.
    #[serde(with = "crate::wire_u64", alias = "source_timestamp_missing_rows")]
    pub source_timestamp_missing_rows: u64,
    /// Number of rows in the immutable object.
    #[serde(with = "crate::wire_u64", alias = "row_count")]
    pub row_count: u64,
    /// One immutable Parquet object.
    pub object: DatasetObjectV2,
    /// Object readback receipt, independent of source completion evidence.
    #[serde(alias = "storage_verification")]
    pub storage_verification: DatasetStorageVerificationV2,
    /// Exactly one completion-evidence oneof case.
    #[serde(alias = "completion_evidence")]
    pub completion_evidence: DatasetCompletionEvidenceV2,
}

impl DatasetManifestV2 {
    /// Validate common identity/object fields, object readback, and evidence structure.
    ///
    /// This validates structure and internal bindings only. It never creates a trusted provider
    /// watermark token or establishes source authorization/completeness.
    pub fn validate(&self) -> Result<(), DatasetManifestError> {
        if self.schema_version != DATASET_MANIFEST_SCHEMA_VERSION_V2 {
            return Err(DatasetManifestError::UnsupportedVersion);
        }
        if self.symbols.len() > MAX_DATASET_MANIFEST_V2_SYMBOLS {
            return Err(DatasetManifestError::InvalidSymbols);
        }
        let source = self.source.to_v1();
        let object = self.object.to_v1();
        let time_range = self
            .time_range
            .as_ref()
            .map(DatasetTimeRangeV2::to_v1)
            .transpose()
            .map_err(|_| DatasetManifestError::InvalidTimeRange)?;
        validate_dataset_manifest_fields(
            &self.dataset_id,
            &source,
            &self.symbols,
            time_range.as_ref(),
            self.source_timestamp_missing_rows,
            self.row_count,
            &object,
        )?;

        if !valid_sha256(&self.storage_verification.readback_sha256)
            || !self.storage_verification.verified_before_publish
            || self.storage_verification.readback_sha256 != self.object.content_sha256
        {
            return Err(DatasetManifestError::InvalidStorageVerification);
        }
        if !self.completion_evidence.validate(&source, self) {
            return Err(DatasetManifestError::InvalidCompletion);
        }
        Ok(())
    }
}

/// Parse bounded canonical-shape JSON and validate the V2 manifest before returning it.
///
/// Serde rejects duplicate known fields and all V2 structs reject unknown fields. Both camelCase
/// ProtoJSON and snake_case protobuf field names are accepted individually; using both spellings
/// for a single field is a duplicate-field error.
pub fn parse_dataset_manifest_v2_json(
    bytes: &[u8],
) -> Result<DatasetManifestV2, DatasetManifestV2Error> {
    if bytes.len() > MAX_DATASET_MANIFEST_V2_JSON_BYTES {
        return Err(DatasetManifestV2Error::JsonTooLarge);
    }
    let manifest: DatasetManifestV2 =
        serde_json::from_slice(bytes).map_err(DatasetManifestV2Error::InvalidJson)?;
    manifest.validate()?;
    Ok(manifest)
}

mod optional_wire_u64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?;
        value
            .map(|text| {
                let value = text.parse::<u64>().map_err(D::Error::custom)?;
                if value.to_string() != text {
                    return Err(D::Error::custom(
                        "uint64 string must use canonical decimal form",
                    ));
                }
                Ok(value)
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DATASET_MANIFEST_SCHEMA_VERSION_V2, DatasetManifestV2, DatasetManifestV2Error,
        MAX_DATASET_MANIFEST_V2_JSON_BYTES, MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS,
        ProtoTimestampV2, parse_dataset_manifest_v2_json,
    };
    use serde_json::{Value, json};

    fn fixture_bytes() -> &'static [u8] {
        include_bytes!("../../../schemas/fixtures/dataset-manifest-v2.json")
    }

    fn fixture_value() -> Value {
        serde_json::from_slice(fixture_bytes()).unwrap()
    }

    fn parse_value(value: &Value) -> Result<DatasetManifestV2, DatasetManifestV2Error> {
        parse_dataset_manifest_v2_json(&serde_json::to_vec(value).unwrap())
    }

    fn fixture() -> DatasetManifestV2 {
        parse_dataset_manifest_v2_json(fixture_bytes()).unwrap()
    }

    #[test]
    fn shared_protojson_fixture_preserves_nanoseconds_and_validates_finite_input() {
        let value = fixture();
        value.validate().unwrap();
        assert_eq!(value.schema_version, DATASET_MANIFEST_SCHEMA_VERSION_V2);
        assert_eq!(value.row_count, 1);
        assert_eq!(value.object.size_bytes, 4096);
        assert_eq!(
            value.time_range.as_ref().unwrap().start_inclusive.as_str(),
            "2026-10-08T14:30:00.100Z"
        );

        let encoded = serde_json::to_value(value).unwrap();
        assert_eq!(encoded["rowCount"], "1");
        assert_eq!(encoded["object"]["sizeBytes"], "4096");
        assert_eq!(
            encoded["completionEvidence"]["finiteBatch"]["completedAt"],
            "2026-10-08T14:31:02.123456789Z"
        );
        assert!(encoded.get("completion").is_none());
    }

    #[test]
    fn complete_snake_case_fixtures_parse_and_dual_spellings_are_rejected() {
        for bytes in [
            include_bytes!("../../../schemas/fixtures/dataset-manifest-v2-snake.json").as_slice(),
            include_bytes!(
                "../../../schemas/fixtures/dataset-manifest-v2-provider-watermark-snake.json"
            )
            .as_slice(),
        ] {
            parse_dataset_manifest_v2_json(bytes).unwrap();
        }

        let mut schema_version = fixture_value();
        schema_version["schema_version"] = schema_version["schemaVersion"].clone();
        assert!(parse_value(&schema_version).is_err());

        let mut object_name = fixture_value();
        object_name["object"]["object_name"] = object_name["object"]["objectName"].clone();
        assert!(parse_value(&object_name).is_err());

        let provider_fixture =
            include_bytes!("../../../schemas/fixtures/dataset-manifest-v2-provider-watermark.json");
        for (camel, snake) in [
            ("firstSequence", "first_sequence"),
            ("lastSequence", "last_sequence"),
            ("sequenceCount", "sequence_count"),
        ] {
            let mut value: Value = serde_json::from_slice(provider_fixture).unwrap();
            value["completionEvidence"]["providerWatermark"][snake] =
                value["completionEvidence"]["providerWatermark"][camel].clone();
            assert!(
                parse_value(&value).is_err(),
                "accepted both {camel} spellings"
            );
        }
    }

    #[test]
    fn protobuf_timestamp_canonicalization_keeps_nanos_and_orders_by_instant() {
        let zero = ProtoTimestampV2::parse("2026-10-08T14:30:00Z").unwrap();
        let one_ns = ProtoTimestampV2::parse("2026-10-08T14:30:00.000000001Z").unwrap();
        let tenth = ProtoTimestampV2::parse("2026-10-08T14:30:00.1Z").unwrap();
        let same_tenth = ProtoTimestampV2::parse("2026-10-08T14:30:00.100000000+00:00").unwrap();
        let next_tenth = ProtoTimestampV2::parse("2026-10-08T14:30:00.11Z").unwrap();

        assert!(zero < one_ns);
        assert!(tenth < next_tenth);
        assert_eq!(tenth, same_tenth);
        assert_eq!(tenth.as_str(), "2026-10-08T14:30:00.100Z");
        assert_eq!(
            serde_json::to_string(&ProtoTimestampV2::parse("2026-10-08T14:30:00.000001Z").unwrap())
                .unwrap(),
            "\"2026-10-08T14:30:00.000001Z\""
        );
    }

    #[test]
    fn protobuf_timestamp_rejects_precision_loss_and_leap_seconds() {
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/fixtures/proto-timestamp-v2.json"
        ))
        .unwrap();
        for case in fixture["valid"].as_array().unwrap() {
            let value = case["value"].as_str().unwrap();
            assert!(ProtoTimestampV2::parse(value).is_ok(), "{}", case["name"]);
        }
        for case in fixture["invalid"].as_array().unwrap() {
            let value = case["value"].as_str().unwrap();
            assert!(ProtoTimestampV2::parse(value).is_err(), "{}", case["name"]);
        }
    }

    #[test]
    fn finite_batch_rejects_empty_partial_or_misclassified_page_receipts() {
        let mut value = fixture_value();
        value["completionEvidence"]["finiteBatch"]["consumedRecordCount"] = json!("0");
        assert!(parse_value(&value).is_err());

        let mut value = fixture_value();
        value["completionEvidence"]["finiteBatch"]["pagesExhausted"] = json!(false);
        assert!(parse_value(&value).is_err());

        let mut value = fixture_value();
        value["completionEvidence"]["finiteBatch"]["pageCount"] = Value::Null;
        assert!(parse_value(&value).is_err());

        let mut value = fixture_value();
        value["completionEvidence"]["finiteBatch"]["completedAt"] =
            json!("2026-10-08T14:31:00.999999999Z");
        assert!(parse_value(&value).is_err());

        let mut value = fixture_value();
        value["completionEvidence"]["finiteBatch"]["dataCutoffExclusive"] =
            json!("2026-10-08T14:30:59.999999999Z");
        assert!(parse_value(&value).is_err());
    }

    #[test]
    fn provider_watermark_is_structural_only_and_enforces_scope_continuity_and_bounds() {
        let provider_fixture =
            include_bytes!("../../../schemas/fixtures/dataset-manifest-v2-provider-watermark.json");
        let decoded = parse_dataset_manifest_v2_json(provider_fixture).unwrap();
        let watermark = decoded.completion_evidence.provider_watermark.unwrap();
        assert_eq!(
            watermark.allowed_lateness_ns,
            MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS
        );
        assert_eq!(watermark.sequence_count, 5);

        let mut value: Value = serde_json::from_slice(provider_fixture).unwrap();
        value["completionEvidence"]["providerWatermark"]["sequenceCount"] = json!("4");
        assert!(parse_value(&value).is_err());

        let mut synthetic: Value = serde_json::from_slice(provider_fixture).unwrap();
        synthetic["source"]["provider"] = json!("synthetic");
        synthetic["source"]["feed"] = json!("synthetic");
        synthetic["source"]["numericEncoding"] = json!("NUMERIC_ENCODING_DECIMAL_TOKEN");
        synthetic["completionEvidence"]["providerWatermark"]["provider"] = json!("synthetic");
        synthetic["completionEvidence"]["providerWatermark"]["feed"] = json!("synthetic");
        assert!(parse_value(&synthetic).is_err());

        let mut value: Value = serde_json::from_slice(provider_fixture).unwrap();
        value["completionEvidence"]["providerWatermark"]["allowedLatenessNs"] =
            json!((MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS + 1).to_string());
        assert!(parse_value(&value).is_err());
    }

    #[test]
    fn provider_watermark_sequence_boundaries_use_checked_u64_arithmetic() {
        const MAX: &str = "18446744073709551615";
        let boundary = include_bytes!(
            "../../../schemas/fixtures/dataset-manifest-v2-provider-watermark-u64-boundary.json"
        );
        let decoded = parse_dataset_manifest_v2_json(boundary).unwrap();
        let watermark = decoded.completion_evidence.provider_watermark.unwrap();
        assert_eq!(watermark.generation, u64::MAX);
        assert_eq!(watermark.first_sequence, 1);
        assert_eq!(watermark.last_sequence, u64::MAX);
        assert_eq!(watermark.sequence_count, u64::MAX);
        assert_eq!(watermark.allowed_lateness_ns, 0);

        let mut zero_generation: Value = serde_json::from_slice(boundary).unwrap();
        zero_generation["completionEvidence"]["providerWatermark"]["generation"] = json!("0");
        assert!(parse_value(&zero_generation).is_err());

        let mut zero_first: Value = serde_json::from_slice(boundary).unwrap();
        zero_first["completionEvidence"]["providerWatermark"]["firstSequence"] = json!("0");
        assert!(parse_value(&zero_first).is_err());

        let mut zero_last: Value = serde_json::from_slice(boundary).unwrap();
        zero_last["completionEvidence"]["providerWatermark"]["lastSequence"] = json!("0");
        assert!(parse_value(&zero_last).is_err());

        let mut overflowing_span: Value = serde_json::from_slice(boundary).unwrap();
        overflowing_span["completionEvidence"]["providerWatermark"]["firstSequence"] = json!("0");
        overflowing_span["completionEvidence"]["providerWatermark"]["lastSequence"] = json!(MAX);
        assert!(parse_value(&overflowing_span).is_err());
    }

    #[test]
    fn diagnostic_stream_can_record_late_observation_without_claiming_completeness() {
        let decoded = parse_dataset_manifest_v2_json(include_bytes!(
            "../../../schemas/fixtures/dataset-manifest-v2-diagnostic-stream.json"
        ))
        .unwrap();
        assert!(decoded.completion_evidence.diagnostic_stream.is_some());
        let encoded = serde_json::to_value(decoded).unwrap();
        assert!(
            encoded["completionEvidence"]["diagnosticStream"]
                .get("completeUpToExclusive")
                .is_none()
        );
    }

    #[test]
    fn parser_rejects_missing_or_multiple_oneof_cases_and_unknown_fields() {
        let mut no_evidence = fixture_value();
        no_evidence["completionEvidence"] = json!({});
        assert!(parse_value(&no_evidence).is_err());

        let mut multiple = fixture_value();
        let finite = multiple["completionEvidence"]["finiteBatch"].clone();
        multiple["completionEvidence"]["diagnosticStream"] = json!({
            "sourceInstanceId": "local-test-session",
            "generation": "1",
            "observedLastSequence": "1",
            "localPolicyCutoff": "2026-10-08T14:31:00Z",
            "diagnosticPolicySha256": "d".repeat(64),
            "diagnosticReceiptSha256": "e".repeat(64)
        });
        multiple["completionEvidence"]["finiteBatch"] = finite;
        assert!(parse_value(&multiple).is_err());

        let mut unknown = fixture_value();
        unknown["completionEvidence"]["finiteBatch"]["callerQualified"] = json!(true);
        assert!(parse_value(&unknown).is_err());

        let mut unknown_source = fixture_value();
        unknown_source["source"]["accountId"] = json!("must-not-be-accepted");
        assert!(parse_value(&unknown_source).is_err());
    }

    #[test]
    fn parser_rejects_duplicate_field_spellings_numeric_u64_and_oversized_json() {
        let duplicate = String::from_utf8(fixture_bytes().to_vec())
            .unwrap()
            .replace(
                "\"rowCount\": \"1\"",
                "\"rowCount\": \"1\", \"row_count\": \"1\"",
            );
        assert!(parse_dataset_manifest_v2_json(duplicate.as_bytes()).is_err());

        let mut numeric_u64 = fixture_value();
        numeric_u64["completionEvidence"]["finiteBatch"]["inputSizeBytes"] = json!(8192);
        assert!(parse_value(&numeric_u64).is_err());

        assert!(matches!(
            parse_dataset_manifest_v2_json(&vec![b' '; MAX_DATASET_MANIFEST_V2_JSON_BYTES + 1]),
            Err(DatasetManifestV2Error::JsonTooLarge)
        ));
    }

    #[test]
    fn protojson_enums_require_known_string_names_instead_of_numeric_values() {
        let cases: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/fixtures/dataset-manifest-v2-enum-invalid.json"
        ))
        .unwrap();
        for case in cases["source_numeric_encoding"].as_array().unwrap() {
            let mut value = fixture_value();
            value["source"]["numericEncoding"] = case["value"].clone();
            assert!(parse_value(&value).is_err(), "accepted {}", case["name"]);
        }
        for case in cases["finite_source_kind"].as_array().unwrap() {
            let mut value = fixture_value();
            value["completionEvidence"]["finiteBatch"]["sourceKind"] = case["value"].clone();
            assert!(parse_value(&value).is_err(), "accepted {}", case["name"]);
        }
    }
}
