//! Version 1 read-only market-bars query and response DTOs.
//!
//! These are HTTP JSON contracts only. They describe request intent and returned evidence;
//! they do not fetch data, establish provider entitlement, or grant execution authority.

use crate::{DecimalString, NumericEncodingV1, ProtoTimestampV2};
use exact_decimal::ExactDecimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use thiserror::Error;

/// Current wire schema version for the bars query response.
pub const BARS_SCHEMA_VERSION_V1: u32 = 1;
/// Maximum JSON request size accepted by the Core projection.
pub const MAX_BARS_REQUEST_JSON_BYTES_V1: usize = 16 * 1024;
/// Maximum JSON response size accepted by the Core projection.
pub const MAX_BARS_RESPONSE_JSON_BYTES_V1: usize = 8 * 1024 * 1024;
/// Maximum UTF-8 cursor size accepted by the shared DTO.
pub const MAX_BARS_CURSOR_BYTES_V1: usize = 2048;

/// Stable errors at the market-bars DTO boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum BarsContractErrorV1 {
    /// JSON was malformed, contained an unknown field, or exceeded its byte bound.
    #[error("invalid market bars v1 JSON")]
    InvalidJson,
    /// A request field was outside the supported query contract.
    #[error("invalid market bars v1 request")]
    InvalidRequest,
    /// A response failed its structural, provenance, or row checks.
    #[error("invalid market bars v1 response")]
    InvalidResponse,
    /// An OHLCV tuple violates exact decimal or high/low ordering constraints.
    #[error("invalid market bars v1 OHLCV values")]
    InvalidOhlcv,
}

/// Stock or exchange-traded fund query scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClassV1 {
    /// Common and preferred equity symbols.
    Stock,
    /// Exchange-traded funds.
    Etf,
}

/// Requested or returned bar interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BarResolutionV1 {
    /// One-minute bars.
    #[serde(rename = "1m")]
    OneMinute,
    /// Five-minute bars.
    #[serde(rename = "5m")]
    FiveMinute,
    /// Fifteen-minute bars.
    #[serde(rename = "15m")]
    FifteenMinute,
    /// One-hour bars.
    #[serde(rename = "1h")]
    OneHour,
    /// One-day bars.
    #[serde(rename = "1d")]
    OneDay,
}

impl BarResolutionV1 {
    /// Canonical HTTP wire label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::FiveMinute => "5m",
            Self::FifteenMinute => "15m",
            Self::OneHour => "1h",
            Self::OneDay => "1d",
        }
    }
}

/// Market session selection for a historical bars request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionScopeV1 {
    /// Include all supported sessions.
    All,
    /// Regular trading hours only.
    Regular,
    /// Premarket and postmarket sessions.
    Extended,
    /// Overnight session where the provider and calendar support it.
    Overnight,
}

/// Provider adjustment policy for returned prices and volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjustmentV1 {
    /// No corporate-action adjustment.
    Unadjusted,
    /// Split-adjusted values.
    Split,
    /// Dividend-adjusted values.
    Dividend,
    /// Split- and dividend-adjusted values.
    All,
}

/// Provider selection policy requested by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarsProviderModeV1 {
    /// Select an eligible provider under the caller's routing policy.
    Auto,
    /// Restrict the request to Alpaca.
    #[serde(rename = "alpaca_only")]
    AlpacaOnly,
    /// Restrict the request to Futu.
    #[serde(rename = "futu_only")]
    FutuOnly,
    /// Select a provider for research-only use.
    Research,
}

/// Read-only GET query DTO. `start` is inclusive and `end` is exclusive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarsRequestV1 {
    /// Requested market symbol.
    pub symbol: String,
    /// Stock or ETF scope.
    pub asset_class: AssetClassV1,
    /// Inclusive UTC lower bound.
    pub start: ProtoTimestampV2,
    /// Exclusive UTC upper bound.
    pub end: ProtoTimestampV2,
    /// Requested bar interval.
    pub resolution: BarResolutionV1,
    /// Session selection.
    pub session_scope: SessionScopeV1,
    /// Corporate-action adjustment.
    pub adjustment: AdjustmentV1,
    /// Provider routing policy.
    pub provider_mode: BarsProviderModeV1,
    /// Whether an unsupported intraday request may fall back to daily bars.
    pub allow_daily_downgrade: bool,
    /// Maximum returned rows, inclusive range 1 through 1000.
    pub limit: u16,
    /// Opaque provider-independent continuation cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl BarsRequestV1 {
    /// Parse bounded snake_case request JSON and validate its half-open range and limits.
    pub fn parse_json(bytes: &[u8]) -> Result<Self, BarsContractErrorV1> {
        if bytes.len() > MAX_BARS_REQUEST_JSON_BYTES_V1 {
            return Err(BarsContractErrorV1::InvalidJson);
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| BarsContractErrorV1::InvalidJson)?;
        if !timestamp_fields_are_utc(&value) {
            return Err(BarsContractErrorV1::InvalidRequest);
        }
        let request: Self =
            serde_json::from_value(value).map_err(|_| BarsContractErrorV1::InvalidJson)?;
        request.validate()?;
        Ok(request)
    }

    /// Validate the fields shared by HTTP adapters.
    pub fn validate(&self) -> Result<(), BarsContractErrorV1> {
        if !valid_label(&self.symbol, 256)
            || self.start >= self.end
            || !(1..=1000).contains(&self.limit)
            || self
                .cursor
                .as_deref()
                .is_some_and(|cursor| !valid_label(cursor, MAX_BARS_CURSOR_BYTES_V1))
        {
            return Err(BarsContractErrorV1::InvalidRequest);
        }
        Ok(())
    }

    /// Require a response to reflect this exact symbol, asset class, adjustment and resolution
    /// request. Daily downgrade is permitted only when it was explicitly enabled.
    pub fn validate_response(&self, response: &BarsResponseV1) -> Result<(), BarsContractErrorV1> {
        self.validate()?;
        response.validate()?;
        if response.requested_resolution != self.resolution
            || response.adjustment != self.adjustment
            || response.rows.len() > usize::from(self.limit)
            || response
                .rows
                .iter()
                .any(|row| row.symbol != self.symbol || row.asset_class != self.asset_class)
            || response.effective_resolution != self.resolution
                && !(self.allow_daily_downgrade
                    && self.resolution != BarResolutionV1::OneDay
                    && response.effective_resolution == BarResolutionV1::OneDay)
            || response.segments.iter().any(|segment| {
                segment.start_utc < self.start || segment.end_exclusive_utc > self.end
            })
            || response
                .gaps
                .iter()
                .any(|gap| gap.start_utc < self.start || gap.end_exclusive_utc > self.end)
            || response
                .rows
                .iter()
                .any(|row| row.bar_start_utc < self.start || row.bar_end_exclusive_utc > self.end)
        {
            return Err(BarsContractErrorV1::InvalidResponse);
        }
        Ok(())
    }
}

/// Data coverage status for a response or a bounded interval segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarsCoverageStatusV1 {
    /// The requested interval is covered and the result is not truncated.
    Complete,
    /// Some requested interval is missing or cannot be established.
    Partial,
    /// The source confirmed the interval but returned no bars.
    Empty,
    /// Coverage cannot be established from the available evidence.
    Unknown,
    /// The requested data/resolution is unsupported by the selected source.
    Unsupported,
}

/// Cache observation for a bars response.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarsCacheStatusV1 {
    /// All returned data came from a fresh cache entry.
    Hit,
    /// The response required a provider fetch.
    Miss,
    /// A stale cache entry contributed to the response.
    Stale,
    /// Cache lookup was explicitly bypassed.
    Bypass,
    /// Cache state could not be read or was unavailable.
    Unavailable,
    /// Cache state was not observed.
    Unknown,
}

/// Entitlement observation for a source or summarized response.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarsEntitlementStatusV1 {
    /// A trusted source record confirms entitlement.
    Authorized,
    /// A trusted source record denies entitlement.
    Unauthorized,
    /// No trusted entitlement observation is attached.
    Unknown,
    /// The response combines sources with different entitlement observations.
    Mixed,
}

/// Validation quality for a returned row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarsValidationStateV1 {
    /// The row passed the shared structural and OHLCV checks.
    Validated,
    /// The row is structurally usable but its source evidence is provisional.
    Provisional,
    /// The row conflicts with another observation or its declared provenance.
    Conflict,
}

/// Machine-readable reason why an interval is represented as a gap.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageGapReasonV1 {
    /// The source reported or exposed a discontinuity.
    SourceGap,
    /// The selected provider was unavailable for this interval.
    ProviderUnavailable,
    /// Entitlement prevented the interval from being read.
    EntitlementLimited,
    /// The requested resolution is unsupported for this interval.
    UnsupportedResolution,
    /// The caller excluded the session containing this interval.
    SessionExcluded,
    /// A material validation conflict prevents a reliable row projection.
    ValidationConflict,
    /// The reason could not be determined.
    Unknown,
}

/// Exact provider/feed identity and numeric-source evidence for one response segment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarSourceProvenanceV1 {
    /// Stable identity referenced by rows and coverage segments.
    pub provenance_id: String,
    /// Actual provider identity; never the requested routing mode.
    pub provider: String,
    /// Actual feed identity.
    pub feed: String,
    /// Actual data origin such as `Yahoo Finance`, `Futu OpenD`, or `Alpaca SIP`.
    pub source: String,
    /// Source entitlement observation, not an authorization grant.
    pub entitlement_status: BarsEntitlementStatusV1,
    /// Numeric representation exposed by the source.
    pub numeric_encoding: NumericEncodingV1,
}

/// Coverage interval backed by one session and optional actual source identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarCoverageSegmentV1 {
    /// Inclusive UTC interval start.
    pub start_utc: ProtoTimestampV2,
    /// Exclusive UTC interval end.
    pub end_exclusive_utc: ProtoTimestampV2,
    /// Coverage observation for this interval.
    pub coverage_status: BarsCoverageStatusV1,
    /// Provider/feed record that supports this segment, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_id: Option<String>,
    /// Concrete market-calendar session identity for this interval.
    pub session_instance_id: String,
}

/// Missing interval with an explicit reason code.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarCoverageGapV1 {
    /// Inclusive UTC interval start.
    pub start_utc: ProtoTimestampV2,
    /// Exclusive UTC interval end.
    pub end_exclusive_utc: ProtoTimestampV2,
    /// Machine-readable gap classification.
    pub reason_code: CoverageGapReasonV1,
}

/// One exact OHLCV bar with event, receipt, and availability times.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarRowV1 {
    /// Instrument identity.
    pub symbol: String,
    /// Instrument class.
    pub asset_class: AssetClassV1,
    /// Effective interval represented by this row.
    pub resolution: BarResolutionV1,
    /// Corporate-action adjustment already applied to this row.
    pub adjustment: AdjustmentV1,
    /// Inclusive event interval start.
    pub bar_start_utc: ProtoTimestampV2,
    /// Exclusive event interval end.
    pub bar_end_exclusive_utc: ProtoTimestampV2,
    /// Collector receive time for the source payload.
    pub received_at_utc: ProtoTimestampV2,
    /// Time when the normalized row became readable.
    pub available_at_utc: ProtoTimestampV2,
    /// Exact decimal open price string.
    pub open: DecimalString,
    /// Exact decimal high price string.
    pub high: DecimalString,
    /// Exact decimal low price string.
    pub low: DecimalString,
    /// Exact decimal close price string.
    pub close: DecimalString,
    /// Exact non-negative decimal volume string.
    pub volume: DecimalString,
    /// Reference into `BarsResponseV1.source_provenance`.
    pub provenance_id: String,
    /// Concrete market-calendar session identity.
    pub session_instance_id: String,
    /// Structural/source validation result; independent verification is recorded separately.
    pub validation_state: BarsValidationStateV1,
}

impl BarRowV1 {
    fn validate(&self, effective_resolution: BarResolutionV1) -> Result<(), BarsContractErrorV1> {
        let zero = ExactDecimal::ZERO;
        let open = decimal(&self.open)?;
        let high = decimal(&self.high)?;
        let low = decimal(&self.low)?;
        let close = decimal(&self.close)?;
        let volume = decimal(&self.volume)?;
        if !valid_label(&self.symbol, 256)
            || !valid_label(&self.provenance_id, 128)
            || !valid_label(&self.session_instance_id, 128)
            || self.resolution != effective_resolution
            || self.bar_start_utc >= self.bar_end_exclusive_utc
            || self.received_at_utc > self.available_at_utc
            || open <= zero
            || high <= zero
            || low <= zero
            || close <= zero
            || volume < zero
            || high < open
            || high < close
            || high < low
            || low > open
            || low > close
        {
            return Err(BarsContractErrorV1::InvalidOhlcv);
        }
        Ok(())
    }
}

/// Read-only bars query result with coverage and source provenance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarsResponseV1 {
    /// Must be 1 for this response contract.
    pub schema_version: u32,
    /// Provider summary (`mixed` or `unknown` when no single identity applies).
    pub provider: String,
    /// Feed summary (`mixed` or `unknown` when no single identity applies).
    pub feed: String,
    /// Entitlement summary across all actual source records.
    pub entitlement_status: BarsEntitlementStatusV1,
    /// Requested interval.
    pub requested_resolution: BarResolutionV1,
    /// Interval represented by each returned row.
    pub effective_resolution: BarResolutionV1,
    /// Adjustment applied to every row and bound to storage provenance.
    pub adjustment: AdjustmentV1,
    /// Returned exact OHLCV rows.
    pub rows: Vec<BarRowV1>,
    /// Aggregate coverage status. A truncated result can never be complete.
    pub coverage_status: BarsCoverageStatusV1,
    /// Source-backed coverage intervals.
    pub segments: Vec<BarCoverageSegmentV1>,
    /// Explicitly classified missing intervals.
    pub gaps: Vec<BarCoverageGapV1>,
    /// Actual source identities referenced by rows and segments.
    pub source_provenance: Vec<BarSourceProvenanceV1>,
    /// Source data's observation cutoff, when the source provides one.
    pub as_of: Option<ProtoTimestampV2>,
    /// Local fetch completion time.
    pub fetched_at: ProtoTimestampV2,
    /// Independent verification time, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<ProtoTimestampV2>,
    /// Cache observation for this response.
    pub cache_status: BarsCacheStatusV1,
    /// Immutable calendar version used to interpret session segments, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_calendar_version: Option<String>,
    /// Opaque continuation token for a truncated page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_cursor: Option<String>,
    /// Whether more response rows are available through the continuation cursor.
    pub truncated: bool,
}

impl BarsResponseV1 {
    /// Parse bounded snake_case response JSON and validate rows against source and session evidence.
    pub fn parse_json(bytes: &[u8]) -> Result<Self, BarsContractErrorV1> {
        if bytes.len() > MAX_BARS_RESPONSE_JSON_BYTES_V1 {
            return Err(BarsContractErrorV1::InvalidJson);
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| BarsContractErrorV1::InvalidJson)?;
        if !timestamp_fields_are_utc(&value) {
            return Err(BarsContractErrorV1::InvalidResponse);
        }
        let response: Self =
            serde_json::from_value(value).map_err(|_| BarsContractErrorV1::InvalidJson)?;
        response.validate()?;
        Ok(response)
    }

    /// Check source summaries, row intervals, pagination, and exact OHLCV invariants.
    pub fn validate(&self) -> Result<(), BarsContractErrorV1> {
        if self.schema_version != BARS_SCHEMA_VERSION_V1
            || self.rows.len() > 1000
            || self
                .market_calendar_version
                .as_deref()
                .is_some_and(|value| !valid_label(value, 128))
            || self
                .continuation_cursor
                .as_deref()
                .is_some_and(|value| !valid_label(value, MAX_BARS_CURSOR_BYTES_V1))
            || self.truncated != self.continuation_cursor.is_some()
            || self.truncated && self.coverage_status == BarsCoverageStatusV1::Complete
            || self.coverage_status == BarsCoverageStatusV1::Complete
                && (!self.gaps.is_empty()
                    || self
                        .segments
                        .iter()
                        .any(|segment| segment.coverage_status != BarsCoverageStatusV1::Complete))
            || self.coverage_status == BarsCoverageStatusV1::Empty && !self.rows.is_empty()
        {
            return Err(BarsContractErrorV1::InvalidResponse);
        }

        let mut provenance_by_id = HashMap::with_capacity(self.source_provenance.len());
        let mut providers = BTreeSet::new();
        let mut feeds = BTreeSet::new();
        let mut entitlements = BTreeSet::new();
        for provenance in &self.source_provenance {
            if !valid_label(&provenance.provenance_id, 128)
                || !valid_label(&provenance.provider, 128)
                || !valid_label(&provenance.feed, 128)
                || !valid_label(&provenance.source, 128)
                || is_mixed_marker(&provenance.provider)
                || is_mixed_marker(&provenance.feed)
                || provenance.entitlement_status == BarsEntitlementStatusV1::Mixed
                || provenance.numeric_encoding == NumericEncodingV1::Unspecified
                || provenance.numeric_encoding.is_raw_bytes()
                || provenance_by_id
                    .insert(provenance.provenance_id.as_str(), provenance)
                    .is_some()
            {
                return Err(BarsContractErrorV1::InvalidResponse);
            }
            providers.insert(provenance.provider.as_str());
            feeds.insert(provenance.feed.as_str());
            entitlements.insert(provenance.entitlement_status);
        }
        if summary(&providers) != self.provider
            || summary(&feeds) != self.feed
            || summarize_entitlement(&entitlements) != self.entitlement_status
        {
            return Err(BarsContractErrorV1::InvalidResponse);
        }

        validate_ordered_ranges(
            self.segments
                .iter()
                .map(|segment| (&segment.start_utc, &segment.end_exclusive_utc)),
        )?;
        validate_ordered_ranges(
            self.gaps
                .iter()
                .map(|gap| (&gap.start_utc, &gap.end_exclusive_utc)),
        )?;
        for segment in &self.segments {
            if segment.start_utc >= segment.end_exclusive_utc
                || !valid_label(&segment.session_instance_id, 128)
                || matches!(
                    segment.coverage_status,
                    BarsCoverageStatusV1::Empty | BarsCoverageStatusV1::Unsupported
                )
                || segment
                    .provenance_id
                    .as_deref()
                    .is_some_and(|id| !provenance_by_id.contains_key(id))
            {
                return Err(BarsContractErrorV1::InvalidResponse);
            }
        }
        for gap in &self.gaps {
            if gap.start_utc >= gap.end_exclusive_utc {
                return Err(BarsContractErrorV1::InvalidResponse);
            }
            if self.segments.iter().any(|segment| {
                gap.start_utc < segment.end_exclusive_utc
                    && segment.start_utc < gap.end_exclusive_utc
                    && segment.coverage_status == BarsCoverageStatusV1::Complete
            }) {
                return Err(BarsContractErrorV1::InvalidResponse);
            }
        }
        for row in &self.rows {
            row.validate(self.effective_resolution)?;
            if row.adjustment != self.adjustment
                || !provenance_by_id.contains_key(row.provenance_id.as_str())
                || self.gaps.iter().any(|gap| {
                    gap.start_utc < row.bar_end_exclusive_utc
                        && row.bar_start_utc < gap.end_exclusive_utc
                })
                || !self.segments.iter().any(|segment| {
                    segment.provenance_id.as_deref() == Some(row.provenance_id.as_str())
                        && segment.session_instance_id == row.session_instance_id
                        && segment.start_utc <= row.bar_start_utc
                        && segment.end_exclusive_utc >= row.bar_end_exclusive_utc
                })
            {
                return Err(BarsContractErrorV1::InvalidResponse);
            }
        }
        Ok(())
    }
}

fn decimal(value: &DecimalString) -> Result<ExactDecimal, BarsContractErrorV1> {
    value
        .as_str()
        .parse()
        .map_err(|_| BarsContractErrorV1::InvalidOhlcv)
}

fn valid_label(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn is_mixed_marker(value: &str) -> bool {
    value == "mixed"
}

fn timestamp_fields_are_utc(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().all(|(key, child)| {
            let is_timestamp = matches!(
                key.as_str(),
                "start"
                    | "end"
                    | "start_utc"
                    | "end_exclusive_utc"
                    | "bar_start_utc"
                    | "bar_end_exclusive_utc"
                    | "received_at_utc"
                    | "available_at_utc"
                    | "as_of"
                    | "fetched_at"
                    | "verified_at"
            );
            if is_timestamp {
                child.is_null()
                    || child
                        .as_str()
                        .is_some_and(|timestamp| timestamp.ends_with('Z'))
            } else {
                timestamp_fields_are_utc(child)
            }
        }),
        serde_json::Value::Array(values) => values.iter().all(timestamp_fields_are_utc),
        _ => true,
    }
}

fn summary(values: &BTreeSet<&str>) -> String {
    match values.len() {
        0 => "unknown".to_owned(),
        1 => values
            .iter()
            .next()
            .expect("one source identity")
            .to_string(),
        _ => "mixed".to_owned(),
    }
}

fn summarize_entitlement(values: &BTreeSet<BarsEntitlementStatusV1>) -> BarsEntitlementStatusV1 {
    match values.len() {
        0 => BarsEntitlementStatusV1::Unknown,
        1 => *values.iter().next().expect("one entitlement state"),
        _ => BarsEntitlementStatusV1::Mixed,
    }
}

fn validate_ordered_ranges<'a>(
    ranges: impl Iterator<Item = (&'a ProtoTimestampV2, &'a ProtoTimestampV2)>,
) -> Result<(), BarsContractErrorV1> {
    let mut previous_end: Option<&ProtoTimestampV2> = None;
    for (start, end) in ranges {
        if start >= end || previous_end.is_some_and(|previous| previous > start) {
            return Err(BarsContractErrorV1::InvalidResponse);
        }
        previous_end = Some(end);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BarsCoverageStatusV1, BarsRequestV1, BarsResponseV1, BarsValidationStateV1};
    use serde_json::Value;

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../schemas/fixtures/market-bars-v1.json"
        ))
        .expect("shared bars fixture is valid JSON")
    }

    #[test]
    fn shared_snake_case_fixture_roundtrips_in_rust() {
        let fixture = fixture();
        let request_json = serde_json::to_vec(&fixture["request"]).unwrap();
        let request = BarsRequestV1::parse_json(&request_json).unwrap();
        assert_eq!(request.limit, 1000);
        assert_eq!(request.resolution.as_str(), "1m");

        let response_json = serde_json::to_vec(&fixture["response"]).unwrap();
        let response = BarsResponseV1::parse_json(&response_json).unwrap();
        request.validate_response(&response).unwrap();
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].open.as_str(), "10.00");
        assert_eq!(serde_json::to_value(response).unwrap(), fixture["response"]);
    }

    #[test]
    fn request_rejects_empty_ranges_unknown_fields_and_out_of_range_limits() {
        let mut value = fixture()["request"].clone();
        value["end"] = value["start"].clone();
        assert!(BarsRequestV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["request"].clone();
        value["extra"] = Value::Bool(true);
        assert!(BarsRequestV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["request"].clone();
        value["limit"] = Value::from(0);
        assert!(BarsRequestV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["request"].clone();
        value["start"] = Value::String("2026-10-08T10:30:00-04:00".to_owned());
        assert!(BarsRequestV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn response_rejects_decimal_and_source_session_mismatches() {
        let mut value = fixture()["response"].clone();
        value["rows"][0]["high"] = Value::String("9.00".to_owned());
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["response"].clone();
        value["rows"][0]["provenance_id"] = Value::String("other".to_owned());
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["response"].clone();
        value["rows"][0]["session_instance_id"] = Value::String("other".to_owned());
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["response"].clone();
        value["as_of"] = Value::String("2026-10-08T10:31:00-04:00".to_owned());
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut value = fixture()["response"].clone();
        value["coverage_status"] = Value::String("complete".to_owned());
        value["truncated"] = Value::Bool(true);
        value["continuation_cursor"] = Value::String("next".to_owned());
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn provisional_and_conflict_rows_remain_explicit_states() {
        let mut value = fixture()["response"].clone();
        value["rows"][0]["validation_state"] = Value::String("provisional".to_owned());
        let parsed = BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            parsed.rows[0].validation_state,
            BarsValidationStateV1::Provisional
        );
        assert_eq!(parsed.coverage_status, BarsCoverageStatusV1::Complete);
    }

    #[test]
    fn daily_rows_accept_calendar_dependent_dst_session_lengths() {
        let mut request_value = fixture()["request"].clone();
        request_value["resolution"] = Value::String("1d".to_owned());
        request_value["start"] = Value::String("2026-03-08T05:00:00Z".to_owned());
        request_value["end"] = Value::String("2026-03-09T04:00:00Z".to_owned());
        let request =
            BarsRequestV1::parse_json(&serde_json::to_vec(&request_value).unwrap()).unwrap();

        let mut response_value = fixture()["response"].clone();
        response_value["requested_resolution"] = Value::String("1d".to_owned());
        response_value["effective_resolution"] = Value::String("1d".to_owned());
        response_value["rows"][0]["resolution"] = Value::String("1d".to_owned());
        response_value["rows"][0]["bar_start_utc"] =
            Value::String("2026-03-08T05:00:00Z".to_owned());
        response_value["rows"][0]["bar_end_exclusive_utc"] =
            Value::String("2026-03-09T04:00:00Z".to_owned());
        response_value["segments"][0]["start_utc"] =
            Value::String("2026-03-08T05:00:00Z".to_owned());
        response_value["segments"][0]["end_exclusive_utc"] =
            Value::String("2026-03-09T04:00:00Z".to_owned());
        let response =
            BarsResponseV1::parse_json(&serde_json::to_vec(&response_value).unwrap()).unwrap();
        request.validate_response(&response).unwrap();
    }

    #[test]
    fn response_rejects_segments_and_rows_outside_the_requested_window() {
        let request =
            BarsRequestV1::parse_json(&serde_json::to_vec(&fixture()["request"]).unwrap()).unwrap();
        let mut response =
            BarsResponseV1::parse_json(&serde_json::to_vec(&fixture()["response"]).unwrap())
                .unwrap();
        response.rows[0].bar_start_utc =
            crate::ProtoTimestampV2::parse("2026-10-08T14:29:00Z").unwrap();
        response.segments[0].start_utc = response.rows[0].bar_start_utc.clone();
        assert!(request.validate_response(&response).is_err());
    }

    #[test]
    fn request_limit_bounds_returned_rows() {
        let mut request =
            BarsRequestV1::parse_json(&serde_json::to_vec(&fixture()["request"]).unwrap()).unwrap();
        request.limit = 1;
        let mut response =
            BarsResponseV1::parse_json(&serde_json::to_vec(&fixture()["response"]).unwrap())
                .unwrap();
        response.rows.push(response.rows[0].clone());
        assert!(request.validate_response(&response).is_err());
    }

    #[test]
    fn gaps_may_overlap_partial_or_unknown_segments_without_rows() {
        for status in ["partial", "unknown"] {
            let mut value = fixture()["response"].clone();
            value["coverage_status"] = Value::String("partial".to_owned());
            value["rows"] = Value::Array(Vec::new());
            value["segments"][0]["coverage_status"] = Value::String(status.to_owned());
            value["segments"][0]["end_exclusive_utc"] =
                Value::String("2026-10-08T14:32:00Z".to_owned());
            value["gaps"] = serde_json::json!([{
                "start_utc": "2026-10-08T14:31:00Z",
                "end_exclusive_utc": "2026-10-08T14:31:30Z",
                "reason_code": "source_gap"
            }]);
            BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        }
    }

    #[test]
    fn gaps_may_be_returned_without_invented_session_segments() {
        let mut value = fixture()["response"].clone();
        value["provider"] = Value::String("unknown".to_owned());
        value["feed"] = Value::String("unknown".to_owned());
        value["entitlement_status"] = Value::String("unknown".to_owned());
        value["coverage_status"] = Value::String("unsupported".to_owned());
        value["rows"] = Value::Array(Vec::new());
        value["segments"] = Value::Array(Vec::new());
        value["source_provenance"] = Value::Array(Vec::new());
        value["gaps"] = serde_json::json!([{
            "start_utc": "2026-10-08T14:30:00Z",
            "end_exclusive_utc": "2026-10-08T14:31:00Z",
            "reason_code": "unsupported_resolution"
        }]);
        value["as_of"] = Value::Null;
        let response = BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(response.as_of, None);
        assert_eq!(response.fetched_at.as_str(), "2026-10-08T14:31:04Z");

        value.as_object_mut().unwrap().remove("as_of");
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_ok());

        value.as_object_mut().unwrap().remove("fetched_at");
        assert!(BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn gaps_cannot_overlap_complete_segments_or_any_row_window() {
        let mut complete_segment = fixture()["response"].clone();
        complete_segment["coverage_status"] = Value::String("partial".to_owned());
        complete_segment["segments"][0]["end_exclusive_utc"] =
            Value::String("2026-10-08T14:32:00Z".to_owned());
        complete_segment["gaps"] = serde_json::json!([{
            "start_utc": "2026-10-08T14:31:00Z",
            "end_exclusive_utc": "2026-10-08T14:31:30Z",
            "reason_code": "source_gap"
        }]);
        assert!(
            BarsResponseV1::parse_json(&serde_json::to_vec(&complete_segment).unwrap()).is_err()
        );

        let mut row_intersection = fixture()["response"].clone();
        row_intersection["coverage_status"] = Value::String("partial".to_owned());
        row_intersection["segments"][0]["coverage_status"] = Value::String("partial".to_owned());
        row_intersection["gaps"] = serde_json::json!([{
            "start_utc": "2026-10-08T14:30:30Z",
            "end_exclusive_utc": "2026-10-08T14:30:40Z",
            "reason_code": "source_gap"
        }]);
        assert!(
            BarsResponseV1::parse_json(&serde_json::to_vec(&row_intersection).unwrap()).is_err()
        );
    }

    #[test]
    fn actual_unknown_feed_remains_unverified_provenance() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../schemas/fixtures/market-bars-v1-unknown-feed.json"
        ))
        .unwrap();
        let value = fixture["response"].clone();
        let response = BarsResponseV1::parse_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(response.provider, "alpaca");
        assert_eq!(response.feed, "unknown");
        assert_eq!(
            response.source_provenance[0].feed, "unknown",
            "the source's missing feed identity must stay explicit"
        );
        assert_eq!(
            response.rows[0].validation_state,
            BarsValidationStateV1::Provisional
        );
        assert_eq!(
            response.source_provenance[0].source,
            "Alpaca synthetic compatibility fixture"
        );
    }
}
