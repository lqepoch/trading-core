//! Cross-language US equity one-minute trade bar V2 with a manifest completion reference.
//!
//! The typed completion oneof remains in DatasetManifestV2. Each row contains only its canonical
//! evidence-projection SHA-256, so immutable bar rows can be joined to the manifest without
//! repeating a potentially large receipt payload. The hash is a structural reference, not proof.

use crate::{
    DatasetManifestV2, DatasetManifestV2Error, DecimalString, EntitlementState, NumericEncodingV1,
    ProtoTimestampV2,
    dataset::{stable_identity, valid_sha256},
    v1::valid_identifier,
    validate_bar_v2_completion_evidence_reference,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use exact_decimal::ExactDecimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bar V2 uses the v2 storage row schema while retaining the shared descriptor format version.
pub const US_EQUITY_TRADE_BAR_SCHEMA_VERSION_V2: u32 = 2;
/// Maximum encoded ProtoJSON bytes accepted for one bar row.
pub const MAX_US_EQUITY_TRADE_BAR_V2_JSON_BYTES: usize = 64 * 1024;

/// Validation failures at the single-row BarV2 boundary.
#[derive(Debug, Error)]
pub enum TradeMinuteBarV2Error {
    /// Input was malformed, duplicated fields, used unknown properties, or exceeded its byte cap.
    #[error("invalid BarV2 JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    /// A row violated its typed time, source, price, or completion invariant.
    #[error("invalid BarV2 row")]
    InvalidRow,
    /// The referenced manifest or its completion evidence was invalid.
    #[error(transparent)]
    InvalidManifest(#[from] DatasetManifestV2Error),
}

/// Completion category carried by the referenced manifest oneof.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarCompletionModeV2 {
    /// One finite immutable input was fully consumed.
    FiniteBatch,
    /// A source watermark observation is present; trust requires a separate verifier.
    ProviderWatermark,
    /// A local stream stop or cutoff is recorded for diagnostics only.
    DiagnosticStream,
}

/// One-minute US equity trade bar; JSON uses camelCase ProtoJSON fields and uint64 strings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TradeMinuteBarV2 {
    /// Must be 2 for this row contract.
    #[serde(alias = "schema_version")]
    pub schema_version: u32,
    /// Provider identity shared with the manifest source.
    #[serde(alias = "source_provider")]
    pub source_provider: String,
    /// Feed identity shared with the manifest source.
    #[serde(alias = "source_feed")]
    pub source_feed: String,
    /// Entitlement observation shared with the manifest source.
    #[serde(alias = "source_entitlement")]
    pub source_entitlement: EntitlementState,
    /// Original normalized numeric source representation.
    #[serde(alias = "source_numeric_encoding")]
    pub source_numeric_encoding: NumericEncodingV1,
    /// Market symbol represented by the bar.
    pub symbol: String,
    /// UTC half-open start of the one-minute bar.
    #[serde(alias = "bar_start_utc")]
    pub bar_start_utc: ProtoTimestampV2,
    /// Exclusive UTC end of the one-minute bar.
    #[serde(alias = "bar_end_exclusive_utc")]
    pub bar_end_exclusive_utc: ProtoTimestampV2,
    /// Time when this output row became available to a consumer.
    #[serde(alias = "available_at_utc")]
    pub available_at_utc: ProtoTimestampV2,
    /// ISO-8601 trading date. The core does not infer a market calendar from this value.
    #[serde(alias = "trade_date")]
    pub trade_date: String,
    /// Opaque session identifier resolved by the referenced policy.
    #[serde(alias = "session_id")]
    pub session_id: String,
    /// IANA timezone identity; the consumer's policy registry resolves it.
    #[serde(alias = "session_timezone")]
    pub session_timezone: String,
    /// Immutable session policy identity.
    #[serde(alias = "session_policy_id")]
    pub session_policy_id: String,
    /// SHA-256 of the immutable session policy.
    #[serde(alias = "session_policy_sha256")]
    pub session_policy_sha256: String,
    /// Session start, inclusive.
    #[serde(alias = "session_start_utc")]
    pub session_start_utc: ProtoTimestampV2,
    /// Session end, exclusive.
    #[serde(alias = "session_end_exclusive_utc")]
    pub session_end_exclusive_utc: ProtoTimestampV2,
    /// Requested source window start, inclusive.
    #[serde(alias = "window_start_utc")]
    pub window_start_utc: ProtoTimestampV2,
    /// Requested source window end, exclusive.
    #[serde(alias = "window_end_exclusive_utc")]
    pub window_end_exclusive_utc: ProtoTimestampV2,
    /// Exact positive decimal open price.
    pub open: DecimalString,
    /// Exact positive decimal high price.
    pub high: DecimalString,
    /// Exact positive decimal low price.
    pub low: DecimalString,
    /// Exact positive decimal close price.
    pub close: DecimalString,
    /// Exact non-negative decimal volume.
    pub volume: DecimalString,
    /// Number of normalized trade observations.
    #[serde(with = "crate::wire_u64", alias = "trade_count")]
    pub trade_count: u64,
    /// Quote events deliberately excluded from trade-only aggregation.
    #[serde(with = "crate::wire_u64", alias = "quote_events_excluded")]
    pub quote_events_excluded: u64,
    /// Source rows missing their provider source timestamp.
    #[serde(with = "crate::wire_u64", alias = "source_timestamp_missing_rows")]
    pub source_timestamp_missing_rows: u64,
    /// Observed sequence gaps in the aggregation input.
    #[serde(with = "crate::wire_u64", alias = "sequence_gap_count")]
    pub sequence_gap_count: u64,
    /// Events observed beyond the configured allowed lateness.
    #[serde(with = "crate::wire_u64", alias = "late_event_count")]
    pub late_event_count: u64,
    /// Expected source-window minute count under the explicit session policy.
    #[serde(with = "crate::wire_u64", alias = "window_expected_minutes")]
    pub window_expected_minutes: u64,
    /// Expected minutes that had no trade event.
    #[serde(with = "crate::wire_u64", alias = "window_empty_trade_minutes")]
    pub window_empty_trade_minutes: u64,
    /// Earliest represented provider source timestamp.
    #[serde(alias = "source_start_utc")]
    pub source_start_utc: ProtoTimestampV2,
    /// Exclusive bound after the latest represented provider source timestamp.
    #[serde(alias = "source_end_exclusive_utc")]
    pub source_end_exclusive_utc: ProtoTimestampV2,
    /// Whether this finite input reader consumed its complete bounded input.
    #[serde(alias = "window_input_eof")]
    pub window_input_eof: bool,
    /// Present only for paged historical input; false is never a publishable completion.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "source_pages_exhausted"
    )]
    pub source_pages_exhausted: Option<bool>,
    /// Informational mode label, cross-checked against the manifest evidence oneof.
    #[serde(alias = "completion_mode")]
    pub completion_mode: BarCompletionModeV2,
    /// NBBO input status observation; not a substitute for quote/trade lineage.
    #[serde(alias = "nbbo_input_status")]
    pub nbbo_input_status: String,
    /// SHA-256 reference to the manifest's canonical completion oneof ProtoJSON bytes.
    #[serde(alias = "completion_evidence_sha256")]
    pub completion_evidence_sha256: String,
}

impl TradeMinuteBarV2 {
    /// Parse one strict, bounded JSON row and check row-local invariants.
    pub fn parse_json(bytes: &[u8]) -> Result<Self, TradeMinuteBarV2Error> {
        if bytes.len() > MAX_US_EQUITY_TRADE_BAR_V2_JSON_BYTES {
            return Err(TradeMinuteBarV2Error::InvalidRow);
        }
        let row: Self =
            serde_json::from_slice(bytes).map_err(TradeMinuteBarV2Error::InvalidJson)?;
        row.validate_shape()?;
        Ok(row)
    }

    /// Check the complete row against the single manifest that owns source and completion.
    pub fn validate_against_manifest(
        &self,
        manifest: &DatasetManifestV2,
    ) -> Result<(), TradeMinuteBarV2Error> {
        self.validate_shape()?;
        manifest
            .validate()
            .map_err(DatasetManifestV2Error::InvalidManifest)?;
        let source = &manifest.source;
        if self.source_provider != source.provider
            || self.source_feed != source.feed
            || self.source_entitlement != source.entitlement
            || self.source_numeric_encoding != source.numeric_encoding.into()
            || !manifest.symbols.contains(&self.symbol)
            || self.source_timestamp_missing_rows != manifest.source_timestamp_missing_rows
        {
            return Err(TradeMinuteBarV2Error::InvalidRow);
        }

        let (completion_mode, window_input_eof, source_pages_exhausted) = match (
            manifest.completion_evidence.finite_batch.as_ref(),
            manifest.completion_evidence.provider_watermark.as_ref(),
            manifest.completion_evidence.diagnostic_stream.as_ref(),
        ) {
            (Some(finite), None, None) => (
                BarCompletionModeV2::FiniteBatch,
                true,
                (finite.source_kind == crate::FiniteBatchSourceKindV2::HistoricalPaged)
                    .then_some(true),
            ),
            (None, Some(_), None) => (BarCompletionModeV2::ProviderWatermark, false, None),
            (None, None, Some(_)) => (BarCompletionModeV2::DiagnosticStream, false, None),
            _ => return Err(TradeMinuteBarV2Error::InvalidRow),
        };
        if self.completion_mode != completion_mode
            || self.window_input_eof != window_input_eof
            || self.source_pages_exhausted != source_pages_exhausted
        {
            return Err(TradeMinuteBarV2Error::InvalidRow);
        }
        validate_bar_v2_completion_evidence_reference(&self.completion_evidence_sha256, manifest)?;

        let time_range = manifest
            .time_range
            .as_ref()
            .ok_or(TradeMinuteBarV2Error::InvalidRow)?;
        if self.source_start_utc < time_range.start_inclusive
            || self.source_end_exclusive_utc > time_range.end_exclusive
        {
            return Err(TradeMinuteBarV2Error::InvalidRow);
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), TradeMinuteBarV2Error> {
        let numeric_encoding_valid = matches!(
            self.source_numeric_encoding,
            NumericEncodingV1::DecimalToken
                | NumericEncodingV1::IntegerToken
                | NumericEncodingV1::BinaryFloat64ShortestDecimal
                | NumericEncodingV1::BinaryFloat32ShortestDecimal
        );
        let times_valid = self.session_start_utc < self.session_end_exclusive_utc
            && self.window_start_utc < self.window_end_exclusive_utc
            && self.window_start_utc >= self.session_start_utc
            && self.window_end_exclusive_utc <= self.session_end_exclusive_utc
            && self.bar_end_exclusive_utc > self.bar_start_utc
            && timestamp_instant(&self.bar_end_exclusive_utc)
                - timestamp_instant(&self.bar_start_utc)
                == Duration::minutes(1)
            && self.bar_start_utc >= self.window_start_utc
            && self.bar_end_exclusive_utc <= self.window_end_exclusive_utc
            && self.bar_start_utc >= self.session_start_utc
            && self.bar_end_exclusive_utc <= self.session_end_exclusive_utc
            && self.available_at_utc >= self.bar_end_exclusive_utc
            && self.source_start_utc >= self.bar_start_utc
            && self.source_end_exclusive_utc <= self.bar_end_exclusive_utc
            && self.source_start_utc < self.source_end_exclusive_utc;
        let open = parse_positive_decimal(&self.open);
        let high = parse_positive_decimal(&self.high);
        let low = parse_positive_decimal(&self.low);
        let close = parse_positive_decimal(&self.close);
        let volume = ExactDecimal::parse_json_number(self.volume.as_str()).ok();
        let prices_valid = matches!((&open, &high, &low, &close), (Some(open), Some(high), Some(low), Some(close))
            if high >= open && high >= close && high >= low && low <= open && low <= close)
            && volume.is_some_and(|value| value >= ExactDecimal::ZERO);
        let date_valid = NaiveDate::parse_from_str(&self.trade_date, "%Y-%m-%d")
            .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == self.trade_date);
        let source = crate::MarketDataSourceV1 {
            provider: self.source_provider.clone(),
            feed: self.source_feed.clone(),
            entitlement: self.source_entitlement,
            numeric_encoding: self.source_numeric_encoding,
            source_record_id: None,
        };

        if self.schema_version != US_EQUITY_TRADE_BAR_SCHEMA_VERSION_V2
            || source.validate().is_err()
            || !numeric_encoding_valid
            || !self.arrow_timestamps_representable()
            || !valid_identifier(&self.symbol, 256)
            || !valid_sha256(&self.session_policy_sha256)
            || !valid_sha256(&self.completion_evidence_sha256)
            || !stable_identity(&self.session_id, 128)
            || !valid_identifier(&self.session_timezone, 128)
            || !stable_identity(&self.session_policy_id, 256)
            || !stable_identity(&self.completion_mode.to_string(), 32)
            || self.nbbo_input_status.is_empty()
            || self.nbbo_input_status.len() > 128
            || self.nbbo_input_status.trim() != self.nbbo_input_status
            || self.nbbo_input_status.chars().any(char::is_control)
            || self.trade_count == 0
            || self.window_expected_minutes == 0
            || self.window_empty_trade_minutes > self.window_expected_minutes
            || self.source_pages_exhausted == Some(false)
            || !times_valid
            || !prices_valid
            || !date_valid
        {
            return Err(TradeMinuteBarV2Error::InvalidRow);
        }
        Ok(())
    }

    fn arrow_timestamps_representable(&self) -> bool {
        [
            &self.bar_start_utc,
            &self.bar_end_exclusive_utc,
            &self.available_at_utc,
            &self.session_start_utc,
            &self.session_end_exclusive_utc,
            &self.window_start_utc,
            &self.window_end_exclusive_utc,
            &self.source_start_utc,
            &self.source_end_exclusive_utc,
        ]
        .into_iter()
        .all(ProtoTimestampV2::is_arrow_ns_compatible)
    }
}

impl std::fmt::Display for BarCompletionModeV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FiniteBatch => "finite_batch",
            Self::ProviderWatermark => "provider_watermark",
            Self::DiagnosticStream => "diagnostic_stream",
        })
    }
}

fn parse_positive_decimal(value: &DecimalString) -> Option<ExactDecimal> {
    ExactDecimal::parse_json_number(value.as_str())
        .ok()
        .filter(|decimal| !decimal.is_negative() && *decimal > ExactDecimal::ZERO)
}

fn timestamp_instant(value: &ProtoTimestampV2) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value.as_str())
        .expect("ProtoTimestampV2 stores a validated RFC3339 value")
        .with_timezone(&Utc)
}

#[cfg(test)]
mod tests {
    use super::{TradeMinuteBarV2, TradeMinuteBarV2Error};
    use crate::{
        DatasetManifestV2Error, ProtoTimestampV2, parse_dataset_manifest_v2_json,
        validate_bar_v2_completion_evidence_reference,
    };
    use serde_json::{Value, json};

    fn bar_bytes() -> &'static [u8] {
        include_bytes!("../../../schemas/fixtures/us-equity-trade-bar-v2.json")
    }

    fn manifest_bytes() -> &'static [u8] {
        include_bytes!("../../../schemas/fixtures/dataset-manifest-v2.json")
    }

    fn bar_value() -> Value {
        serde_json::from_slice(bar_bytes()).unwrap()
    }

    #[test]
    fn synthetic_v2_bar_binds_to_the_manifest_completion_projection() {
        let manifest = parse_dataset_manifest_v2_json(manifest_bytes()).unwrap();
        let row = TradeMinuteBarV2::parse_json(bar_bytes()).unwrap();
        row.validate_against_manifest(&manifest).unwrap();
        validate_bar_v2_completion_evidence_reference(&row.completion_evidence_sha256, &manifest)
            .unwrap();
        assert_eq!(row.trade_count, 4);
        assert_eq!(row.bar_start_utc.as_str(), "2026-10-08T14:30:00Z");
    }

    #[test]
    fn v2_bar_timestamps_fit_signed_arrow_nanoseconds() {
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/fixtures/raw-frame-timestamp-ns-v2.json"
        ))
        .unwrap();
        let row = TradeMinuteBarV2::parse_json(bar_bytes()).unwrap();
        assert!(row.arrow_timestamps_representable());

        for case in fixture["valid"].as_array().unwrap() {
            let timestamp =
                ProtoTimestampV2::parse(case["timestamp_utc"].as_str().unwrap()).unwrap();
            assert!(timestamp.is_arrow_ns_compatible(), "{case}");
        }

        let year_one = ProtoTimestampV2::parse("0001-01-01T00:00:00Z").unwrap();
        assert!(!year_one.is_arrow_ns_compatible());

        for case in fixture["invalid"].as_array().unwrap() {
            let value = case["timestamp_utc"].as_str().unwrap();
            if let Ok(timestamp) = ProtoTimestampV2::parse(value) {
                assert!(!timestamp.is_arrow_ns_compatible(), "{case}");
            }
            let mut document = bar_value();
            document["availableAtUtc"] = json!(value);
            assert!(
                TradeMinuteBarV2::parse_json(&serde_json::to_vec(&document).unwrap()).is_err(),
                "{}",
                case["name"]
            );
        }

        for case in fixture["bar_v2_proto_valid_but_arrow_ns_invalid"]
            .as_array()
            .unwrap()
        {
            let timestamps = case["timestamps_utc"].as_object().unwrap();
            for value in timestamps.values() {
                assert!(ProtoTimestampV2::parse(value.as_str().unwrap()).is_ok());
            }
            let mut document = bar_value();
            for (field, value) in timestamps {
                document[field.as_str()] = value.clone();
            }
            assert!(
                TradeMinuteBarV2::parse_json(&serde_json::to_vec(&document).unwrap()).is_err(),
                "{}",
                case["name"]
            );
        }

        let out_of_range = ProtoTimestampV2::parse("2262-04-11T23:47:16.854775808Z").unwrap();
        macro_rules! assert_bar_timestamp_rejected {
            ($field:ident) => {{
                let mut candidate = row.clone();
                candidate.$field = out_of_range.clone();
                assert!(
                    !candidate.arrow_timestamps_representable(),
                    stringify!($field)
                );
            }};
        }
        assert_bar_timestamp_rejected!(bar_start_utc);
        assert_bar_timestamp_rejected!(bar_end_exclusive_utc);
        assert_bar_timestamp_rejected!(available_at_utc);
        assert_bar_timestamp_rejected!(session_start_utc);
        assert_bar_timestamp_rejected!(session_end_exclusive_utc);
        assert_bar_timestamp_rejected!(window_start_utc);
        assert_bar_timestamp_rejected!(window_end_exclusive_utc);
        assert_bar_timestamp_rejected!(source_start_utc);
        assert_bar_timestamp_rejected!(source_end_exclusive_utc);

        let mut maximum_endpoint = bar_value();
        maximum_endpoint["availableAtUtc"] = json!("2262-04-11T23:47:16.854775807Z");
        assert!(
            TradeMinuteBarV2::parse_json(&serde_json::to_vec(&maximum_endpoint).unwrap()).is_ok()
        );
    }

    #[test]
    fn generated_row_validation_rejects_an_invalid_trade_date() {
        let manifest = parse_dataset_manifest_v2_json(manifest_bytes()).unwrap();
        let mut row = TradeMinuteBarV2::parse_json(bar_bytes()).unwrap();
        row.trade_date = "2026-02-30".to_owned();
        assert!(matches!(
            row.validate_against_manifest(&manifest),
            Err(TradeMinuteBarV2Error::InvalidRow)
        ));
    }

    #[test]
    fn v2_bar_accepts_snake_case_and_rejects_duplicate_aliases() {
        let camel = TradeMinuteBarV2::parse_json(bar_bytes()).unwrap();
        let snake = TradeMinuteBarV2::parse_json(include_bytes!(
            "../../../schemas/fixtures/us-equity-trade-bar-v2-snake.json"
        ))
        .unwrap();
        assert_eq!(camel, snake);

        let mut duplicate = bar_value();
        duplicate["schema_version"] = json!(2);
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn v2_bar_projection_matches_each_manifest_completion_case() {
        for (row_bytes, manifest_bytes) in [
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-historical-non-paged.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-historical-non-paged.json"
                )
                .as_slice(),
            ),
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-synthetic-replay.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-synthetic-replay.json"
                )
                .as_slice(),
            ),
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-provider-watermark-zero-lateness.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-provider-watermark-zero-lateness.json"
                )
                .as_slice(),
            ),
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-provider-watermark.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-provider-watermark.json"
                )
                .as_slice(),
            ),
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-diagnostic-stream.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-diagnostic-stream.json"
                )
                .as_slice(),
            ),
        ] {
            let row = TradeMinuteBarV2::parse_json(row_bytes).unwrap();
            let manifest = parse_dataset_manifest_v2_json(manifest_bytes).unwrap();
            row.validate_against_manifest(&manifest).unwrap();
        }
    }

    #[test]
    fn finite_bar_page_presence_is_absent_for_nonpaged_and_synthetic_inputs() {
        for (row_bytes, manifest_bytes) in [
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-historical-non-paged.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-historical-non-paged.json"
                )
                .as_slice(),
            ),
            (
                include_bytes!(
                    "../../../schemas/fixtures/us-equity-trade-bar-v2-synthetic-replay.json"
                )
                .as_slice(),
                include_bytes!(
                    "../../../schemas/fixtures/dataset-manifest-v2-synthetic-replay.json"
                )
                .as_slice(),
            ),
        ] {
            let manifest = crate::parse_dataset_manifest_v2_json(manifest_bytes).unwrap();
            let row = TradeMinuteBarV2::parse_json(row_bytes).unwrap();
            assert_eq!(row.source_pages_exhausted, None);
            row.validate_against_manifest(&manifest).unwrap();

            for page_value in [true, false] {
                let mut changed = row.clone();
                changed.source_pages_exhausted = Some(page_value);
                assert!(changed.validate_against_manifest(&manifest).is_err());
            }
        }
    }

    #[test]
    fn v2_bar_rejects_mismatched_completion_or_source_references() {
        let manifest = parse_dataset_manifest_v2_json(manifest_bytes()).unwrap();
        let mut changed = bar_value();
        changed["completionEvidenceSha256"] = json!("e".repeat(64));
        let row = TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(matches!(
            row.validate_against_manifest(&manifest),
            Err(TradeMinuteBarV2Error::InvalidManifest(
                DatasetManifestV2Error::InvalidCompletionEvidenceReference
            ))
        ));

        let mut changed = bar_value();
        changed["sourceProvider"] = json!("another-provider");
        let row = TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(matches!(
            row.validate_against_manifest(&manifest),
            Err(TradeMinuteBarV2Error::InvalidRow)
        ));

        let mut changed = bar_value();
        changed["sourceStartUtc"] = json!("2026-10-08T14:29:59Z");
        assert!(matches!(
            TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()),
            Err(TradeMinuteBarV2Error::InvalidRow)
        ));
    }

    #[test]
    fn v2_bar_rejects_source_bounds_outside_its_minute_even_within_manifest_range() {
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/fixtures/us-equity-trade-bar-v2-source-bounds-invalid.json"
        ))
        .unwrap();
        let mut manifest_json: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/fixtures/dataset-manifest-v2-provider-watermark.json"
        ))
        .unwrap();
        manifest_json["timeRange"]["startInclusive"] =
            fixture["manifestRange"]["startInclusive"].clone();
        manifest_json["timeRange"]["endExclusive"] =
            fixture["manifestRange"]["endExclusive"].clone();
        manifest_json["completionEvidence"]["providerWatermark"]["completeUpToExclusive"] =
            fixture["manifestRange"]["endExclusive"].clone();
        let manifest =
            crate::parse_dataset_manifest_v2_json(&serde_json::to_vec(&manifest_json).unwrap())
                .unwrap();
        let range = manifest.time_range.as_ref().unwrap();
        let mut base_row: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/fixtures/us-equity-trade-bar-v2-provider-watermark.json"
        ))
        .unwrap();
        base_row["completionEvidenceSha256"] =
            json!(crate::dataset_completion_evidence_v2_sha256(&manifest).unwrap());

        for case in fixture["cases"].as_array().unwrap() {
            let field = case["field"].as_str().unwrap();
            let value = case["value"].as_str().unwrap();
            let timestamp = crate::ProtoTimestampV2::parse(value).unwrap();
            assert!(timestamp >= range.start_inclusive && timestamp < range.end_exclusive);
            let mut row = base_row.clone();
            row[field] = json!(value);
            assert!(
                TradeMinuteBarV2::parse_json(&serde_json::to_vec(&row).unwrap()).is_err(),
                "accepted source bound outside the bar: {}",
                case["name"]
            );
        }
    }

    #[test]
    fn v2_bar_rejects_bad_ohlc_time_and_noncanonical_uint64_values() {
        let mut changed = bar_value();
        changed["high"] = json!("9.00");
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err());

        let mut changed = bar_value();
        changed["barEndExclusiveUtc"] = json!("2026-10-08T14:31:00.000000001Z");
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err());

        let mut changed = bar_value();
        changed["tradeCount"] = json!(4);
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err());

        let mut changed = bar_value();
        changed["tradeCount"] = json!("18446744073709551616");
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err());

        for (field, invalid) in [("open", "0"), ("volume", "-1"), ("low", "1e39")] {
            let mut changed = bar_value();
            changed[field] = json!(invalid);
            assert!(
                TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err(),
                "accepted invalid {field}"
            );
        }

        let mut changed = bar_value();
        changed["unrecognized"] = json!(true);
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err());

        let oversized = vec![b' '; super::MAX_US_EQUITY_TRADE_BAR_V2_JSON_BYTES + 1];
        assert!(TradeMinuteBarV2::parse_json(&oversized).is_err());
    }

    #[test]
    fn v2_bar_requires_manifest_oneof_specific_eof_and_page_presence() {
        let manifest = parse_dataset_manifest_v2_json(manifest_bytes()).unwrap();
        let mut changed = bar_value();
        changed["windowInputEof"] = json!(false);
        let row = TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(row.validate_against_manifest(&manifest).is_err());

        let mut changed = bar_value();
        changed["sourcePagesExhausted"] = json!(false);
        assert!(TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).is_err());

        let mut changed = bar_value();
        changed
            .as_object_mut()
            .unwrap()
            .remove("sourcePagesExhausted");
        let row = TradeMinuteBarV2::parse_json(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(row.validate_against_manifest(&manifest).is_err());
    }
}
