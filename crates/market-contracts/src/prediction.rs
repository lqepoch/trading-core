//! Read-only Rust projection of the frozen `PredictionEnvelopeV1` ProtoJSON contract.
//!
//! This module validates bounded wire structure and cross-field identity bindings. Hash strings
//! are references only: parsing does not retrieve or hash artifacts, authenticate an issuer,
//! establish alpha, qualify a model, or grant research or execution authority.

use crate::{DecimalString, EntitlementState, NumericEncodingV1, ProtoTimestampV2};
use serde::{Deserialize, Deserializer};
use thiserror::Error;

/// Maximum UTF-8 size accepted by the bounded prediction ProtoJSON parser.
pub const MAX_PREDICTION_ENVELOPE_V1_JSON_BYTES: usize = 2 * 1024 * 1024;

/// A registered forecast horizon as represented by the frozen prediction protobuf.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForecastHorizonV1 {
    /// Identifies the horizon schema contract.
    #[serde(alias = "schema_name")]
    pub schema_name: String,
    /// Exact horizon registry identity.
    #[serde(alias = "horizon_id")]
    pub horizon_id: String,
    /// Exact registry revision.
    #[serde(alias = "registry_version")]
    pub registry_version: String,
    /// Optional horizon amount; absent for endpoints such as end of day.
    #[serde(default)]
    pub value: Option<u32>,
    /// Horizon unit, encoded by its protobuf enum name.
    pub unit: ForecastHorizonUnitV1,
    /// Exact label specification identity.
    #[serde(alias = "label_spec_id")]
    pub label_spec_id: String,
}

/// Known non-default units in the frozen horizon protobuf enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForecastHorizonUnitV1 {
    /// Protobuf's default sentinel; rejected by validation.
    Unspecified,
    /// A fixed duration in minutes.
    ElapsedMinutes,
    /// A session-close endpoint resolved by a consumer-owned calendar.
    SessionClose,
    /// A trading-day endpoint resolved by a consumer-owned calendar.
    TradingDays,
}

/// Source manifest identity embedded in a prediction envelope.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PredictionSourceV1 {
    /// Provider identity.
    pub provider: String,
    /// Provider feed identity.
    pub feed: String,
    /// Entitlement observation; `authorized` is still only a claim in this payload.
    pub entitlement: EntitlementState,
    /// Immutable dataset version identity.
    #[serde(alias = "dataset_id")]
    pub dataset_id: String,
    /// SHA-256 of the exact manifest bytes, supplied by the producer.
    #[serde(alias = "manifest_sha256")]
    pub manifest_sha256: String,
    /// SHA-256 of the immutable dataset object, supplied by the producer.
    #[serde(alias = "dataset_sha256")]
    pub dataset_sha256: String,
    /// Normalized source numeric encoding, using the protobuf enum name.
    #[serde(
        alias = "numeric_encoding",
        deserialize_with = "deserialize_prediction_numeric_encoding"
    )]
    pub numeric_encoding: NumericEncodingV1,
}

/// Quality state names in the frozen prediction protobuf enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PredictionQualityStatusV1 {
    /// Protobuf's default sentinel; rejected by validation.
    Unspecified,
    /// Producer-reported pass state; not independently verified here.
    Pass,
    /// Evidence is not fully verified.
    Unverified,
    /// Required source data is missing or incomplete.
    BlockedData,
    /// Producer-reported failure state.
    Failed,
}

/// Producer-reported prediction quality and its optional receipt reference.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PredictionQualityV1 {
    /// Protobuf status enum name.
    pub status: PredictionQualityStatusV1,
    /// Optional SHA-256 receipt reference.
    #[serde(default, alias = "receipt_sha256")]
    pub receipt_sha256: Option<String>,
    /// Unique reason codes for non-pass states.
    #[serde(default, alias = "reason_codes")]
    pub reason_codes: Vec<String>,
}

/// Hash references that bind a forecast to train, validation, OOS and selection evidence.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PredictionEvidenceV1 {
    /// Training evidence hash.
    #[serde(alias = "train_sha256")]
    pub train_sha256: String,
    /// Validation evidence hash.
    #[serde(alias = "validation_sha256")]
    pub validation_sha256: String,
    /// Frozen OOS partition evidence hash.
    #[serde(alias = "oos_freeze_sha256")]
    pub oos_freeze_sha256: String,
    /// Optional report-only OOS result hash.
    #[serde(default, alias = "oos_sha256")]
    pub oos_sha256: Option<String>,
    /// Selection evidence hash.
    #[serde(alias = "selection_sha256")]
    pub selection_sha256: String,
    /// Code identity hash.
    #[serde(alias = "code_sha256")]
    pub code_sha256: String,
    /// Source-manifest SHA-256; must equal the source field's manifest hash.
    #[serde(alias = "source_manifest_sha256")]
    pub source_manifest_sha256: String,
    /// Must remain false; OOS results cannot select a prediction.
    #[serde(default, alias = "oos_used_for_selection")]
    pub oos_used_for_selection: bool,
}

/// The existing complete forecast snapshot nested without field remapping.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForecastSnapshotV2 {
    /// Forecast schema identity.
    #[serde(alias = "schema_version")]
    pub schema_version: String,
    /// Owning system identity.
    pub owner: String,
    /// Forecast producer identity.
    pub producer: String,
    /// Forecast consumer identity.
    pub consumer: String,
    /// Forecast row identity.
    #[serde(alias = "forecast_id")]
    pub forecast_id: String,
    /// Strategy identity recorded by the producer.
    #[serde(alias = "strategy_id")]
    pub strategy_id: String,
    /// Uppercase stock symbol.
    pub symbol: String,
    /// Source event clock.
    #[serde(alias = "event_time")]
    pub event_time: ProtoTimestampV2,
    /// Latest information admitted to the forecast.
    #[serde(alias = "knowledge_at")]
    pub knowledge_at: ProtoTimestampV2,
    /// Time the forecast became available.
    #[serde(alias = "available_at")]
    pub available_at: ProtoTimestampV2,
    /// Decision clock.
    #[serde(alias = "decision_at")]
    pub decision_at: ProtoTimestampV2,
    /// Input bar or event resolution.
    #[serde(alias = "input_resolution")]
    pub input_resolution: String,
    /// Model inference frequency.
    #[serde(alias = "inference_frequency")]
    pub inference_frequency: String,
    /// Consumer decision frequency.
    #[serde(alias = "decision_frequency")]
    pub decision_frequency: String,
    /// Registered forecast horizon.
    #[serde(alias = "forecast_horizon")]
    pub forecast_horizon: ForecastHorizonV1,
    /// Beginning of the forecast validity interval.
    #[serde(alias = "valid_from")]
    pub valid_from: ProtoTimestampV2,
    /// Exclusive end of the forecast validity interval.
    #[serde(alias = "valid_until")]
    pub valid_until: ProtoTimestampV2,
    /// Label specification identity.
    #[serde(alias = "label_spec_id")]
    pub label_spec_id: String,
    /// Label specification hash.
    #[serde(alias = "label_spec_sha256")]
    pub label_spec_sha256: String,
    /// Return basis identity.
    #[serde(alias = "return_basis")]
    pub return_basis: String,
    /// Price basis recorded by the producer.
    #[serde(alias = "price_basis")]
    pub price_basis: String,
    /// Exact unscaled producer score.
    #[serde(alias = "raw_score")]
    pub raw_score: DecimalString,
    /// Optional expected return in basis points.
    #[serde(default, alias = "expected_return_bps")]
    pub expected_return_bps: Option<DecimalString>,
    /// Optional probability-like confidence value.
    #[serde(default)]
    pub confidence: Option<DecimalString>,
    /// Optional uncertainty in basis points.
    #[serde(default, alias = "uncertainty_bps")]
    pub uncertainty_bps: Option<DecimalString>,
    /// Optional prediction interval evidence hash.
    #[serde(default, alias = "prediction_interval_sha256")]
    pub prediction_interval_sha256: Option<String>,
    /// Model identity; must match the envelope.
    #[serde(alias = "model_id")]
    pub model_id: String,
    /// Model version; must match the envelope.
    #[serde(alias = "model_version")]
    pub model_version: String,
    /// Model hash; must match the envelope.
    #[serde(alias = "model_sha256")]
    pub model_sha256: String,
    /// Feature-set identity.
    #[serde(alias = "feature_set_id")]
    pub feature_set_id: String,
    /// Feature-set version.
    #[serde(alias = "feature_version")]
    pub feature_version: String,
    /// Feature-set hash.
    #[serde(alias = "feature_sha256")]
    pub feature_sha256: String,
    /// Training dataset hash; must match the envelope and source.
    #[serde(alias = "dataset_sha256")]
    pub dataset_sha256: String,
    /// Universe identity.
    #[serde(alias = "universe_id")]
    pub universe_id: String,
    /// Exchange session identity.
    #[serde(alias = "session_id")]
    pub session_id: String,
    /// Session policy identity, possibly bound to a calendar by the producer.
    #[serde(alias = "session_policy")]
    pub session_policy: String,
    /// Code hash; must match evidence.
    #[serde(alias = "code_sha256")]
    pub code_sha256: String,
    /// Monotone forecast sequence represented as a canonical decimal string.
    #[serde(with = "crate::wire_u64")]
    pub sequence: u64,
    /// Forecast evidence scope.
    #[serde(alias = "evidence_scope")]
    pub evidence_scope: String,
    /// Producer-reported origin category.
    #[serde(alias = "evidence_origin")]
    pub evidence_origin: String,
}

/// Read-only research prediction envelope from the frozen v1 protobuf schema.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PredictionEnvelopeV1 {
    /// Envelope schema discriminator.
    #[serde(alias = "schema_version")]
    pub schema_version: String,
    /// Exact prediction identity.
    #[serde(alias = "prediction_id")]
    pub prediction_id: String,
    /// Producer creation timestamp.
    #[serde(alias = "created_at")]
    pub created_at: ProtoTimestampV2,
    /// Exclusive cutoff applied by the producer.
    #[serde(alias = "data_cutoff")]
    pub data_cutoff: ProtoTimestampV2,
    /// Model identity; must match the nested forecast.
    #[serde(alias = "model_id")]
    pub model_id: String,
    /// Model version; must match the nested forecast.
    #[serde(alias = "model_version")]
    pub model_version: String,
    /// Model hash; must match the nested forecast.
    #[serde(alias = "model_sha256")]
    pub model_sha256: String,
    /// Immutable training dataset identity.
    #[serde(alias = "train_dataset_version")]
    pub train_dataset_version: String,
    /// Training dataset hash.
    #[serde(alias = "train_dataset_sha256")]
    pub train_dataset_sha256: String,
    /// Horizon identity; must equal the nested forecast horizon.
    pub horizon: ForecastHorizonV1,
    /// Source-manifest and feed identity.
    pub source: PredictionSourceV1,
    /// Producer-reported quality state.
    pub quality: PredictionQualityV1,
    /// Evidence hashes and OOS firewall declaration.
    pub evidence: PredictionEvidenceV1,
    /// Existing full forecast payload.
    pub forecast: ForecastSnapshotV2,
}

impl PredictionEnvelopeV1 {
    /// Parse bounded ProtoJSON bytes and enforce structure and cross-field bindings.
    pub fn parse_json(bytes: &[u8]) -> Result<Self, PredictionEnvelopeV1Error> {
        parse_prediction_envelope_v1_protojson(bytes)
    }

    /// Check wire-field semantics and identity bindings without granting authority.
    pub fn validate(&self) -> Result<(), PredictionEnvelopeV1Error> {
        crate::prediction_validation::validate_prediction_envelope(self)
    }
}

/// Parse bounded ProtoJSON bytes for a prediction envelope.
pub fn parse_prediction_envelope_v1_protojson(
    bytes: &[u8],
) -> Result<PredictionEnvelopeV1, PredictionEnvelopeV1Error> {
    if bytes.len() > MAX_PREDICTION_ENVELOPE_V1_JSON_BYTES {
        return Err(PredictionEnvelopeV1Error::JsonTooLarge);
    }
    let envelope = serde_json::from_slice::<PredictionEnvelopeV1>(bytes)
        .map_err(PredictionEnvelopeV1Error::InvalidJson)?;
    envelope.validate()?;
    Ok(envelope)
}

/// Errors from the bounded prediction ProtoJSON projection.
#[derive(Debug, Error)]
pub enum PredictionEnvelopeV1Error {
    /// Input exceeded the fixed byte limit before parsing.
    #[error("prediction ProtoJSON exceeds the configured byte limit")]
    JsonTooLarge,
    /// JSON was malformed, duplicated a field/alias, or contained an unknown field or enum.
    #[error("invalid prediction ProtoJSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    /// The typed value failed a declared structural or binding invariant.
    #[error("invalid prediction envelope: {0}")]
    InvalidEnvelope(&'static str),
}

fn deserialize_prediction_numeric_encoding<'de, D>(
    deserializer: D,
) -> Result<NumericEncodingV1, D::Error>
where
    D: Deserializer<'de>,
{
    let name = String::deserialize(deserializer)?;
    match name.as_str() {
        "NUMERIC_ENCODING_DECIMAL_TOKEN" => Ok(NumericEncodingV1::DecimalToken),
        "NUMERIC_ENCODING_INTEGER_TOKEN" => Ok(NumericEncodingV1::IntegerToken),
        "NUMERIC_ENCODING_BINARY_FLOAT64_SHORTEST_DECIMAL" => {
            Ok(NumericEncodingV1::BinaryFloat64ShortestDecimal)
        }
        "NUMERIC_ENCODING_BINARY_FLOAT32_SHORTEST_DECIMAL" => {
            Ok(NumericEncodingV1::BinaryFloat32ShortestDecimal)
        }
        _ => Err(serde::de::Error::custom(
            "prediction numeric encoding must be a supported normalized protobuf enum name",
        )),
    }
}
