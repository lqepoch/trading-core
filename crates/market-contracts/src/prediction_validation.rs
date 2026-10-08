//! Structural checks for the existing read-only research prediction contract.

use std::collections::HashSet;

use crate::{
    DecimalString, MarketDataSourceV1, NumericEncodingV1, PredictionEnvelopeV1Error,
    PredictionQualityStatusV1,
    prediction::{
        ForecastHorizonUnitV1, ForecastHorizonV1, ForecastSnapshotV2, PredictionEnvelopeV1,
        PredictionEvidenceV1, PredictionQualityV1, PredictionSourceV1,
    },
};
use exact_decimal::ExactDecimal;

const ENVELOPE_SCHEMA: &str = "lqepoch-prediction-envelope-v1";
const FORECAST_SCHEMA: &str = "quant-platform-forecast-snapshot-v2";
const HORIZON_SCHEMA: &str = "quant-platform-forecast-horizon-v1";
const MAX_HORIZON_VALUE: u32 = 10_080;

pub(crate) fn validate_prediction_envelope(
    value: &PredictionEnvelopeV1,
) -> Result<(), PredictionEnvelopeV1Error> {
    if value.schema_version != ENVELOPE_SCHEMA {
        return Err(invalid("schema version is not supported"));
    }
    require_identity(&value.prediction_id, "prediction_id")?;
    require_identity(&value.model_id, "model_id")?;
    require_source_version_token(&value.model_version, "model_version")?;
    require_hash(&value.model_sha256, "model_sha256")?;
    require_dataset_id(&value.train_dataset_version)?;
    require_hash(&value.train_dataset_sha256, "train_dataset_sha256")?;

    value.horizon.validate()?;
    validate_source(&value.source)?;
    validate_quality(&value.quality)?;
    validate_evidence(&value.evidence)?;
    validate_forecast(&value.forecast)?;

    if value.created_at < value.data_cutoff
        || value.data_cutoff > value.forecast.knowledge_at
        || value.data_cutoff > value.forecast.decision_at
    {
        return Err(invalid(
            "creation, cutoff, and forecast clocks are inconsistent",
        ));
    }
    if (
        value.model_id.as_str(),
        value.model_version.as_str(),
        value.model_sha256.as_str(),
    ) != (
        value.forecast.model_id.as_str(),
        value.forecast.model_version.as_str(),
        value.forecast.model_sha256.as_str(),
    ) {
        return Err(invalid("model identity differs from the nested forecast"));
    }
    if value.horizon != value.forecast.forecast_horizon {
        return Err(invalid("horizon differs from the nested forecast"));
    }
    if value.train_dataset_sha256 != value.forecast.dataset_sha256
        || value.source.dataset_id != value.train_dataset_version
        || value.source.dataset_sha256 != value.train_dataset_sha256
    {
        return Err(invalid(
            "dataset identity or hash differs across the envelope",
        ));
    }
    if value.evidence.source_manifest_sha256 != value.source.manifest_sha256 {
        return Err(invalid("source manifest hash differs from the evidence"));
    }
    if value.evidence.code_sha256 != value.forecast.code_sha256 {
        return Err(invalid("code hash differs from the forecast evidence"));
    }
    Ok(())
}

impl ForecastHorizonV1 {
    fn validate(&self) -> Result<(), PredictionEnvelopeV1Error> {
        if self.schema_name != HORIZON_SCHEMA
            || !valid_identity(&self.horizon_id)
            || !valid_source_version_token(&self.registry_version)
            || !valid_identity(&self.label_spec_id)
            || self
                .value
                .is_some_and(|value| !(1..=MAX_HORIZON_VALUE).contains(&value))
            || matches!(self.unit, ForecastHorizonUnitV1::Unspecified)
            || (matches!(self.unit, ForecastHorizonUnitV1::ElapsedMinutes) && self.value.is_none())
        {
            return Err(invalid("horizon fields are incomplete or malformed"));
        }
        Ok(())
    }
}

fn validate_source(value: &PredictionSourceV1) -> Result<(), PredictionEnvelopeV1Error> {
    if !valid_sha256(&value.manifest_sha256)
        || !valid_sha256(&value.dataset_sha256)
        || !matches!(
            value.numeric_encoding,
            NumericEncodingV1::DecimalToken
                | NumericEncodingV1::IntegerToken
                | NumericEncodingV1::BinaryFloat64ShortestDecimal
                | NumericEncodingV1::BinaryFloat32ShortestDecimal
        )
    {
        return Err(invalid(
            "source hash or normalized numeric encoding is invalid",
        ));
    }
    if !crate::dataset::stable_identity(&value.dataset_id, 256) {
        return Err(invalid("source dataset identity is not immutable"));
    }
    MarketDataSourceV1::new(
        value.provider.clone(),
        value.feed.clone(),
        value.entitlement,
        value.numeric_encoding,
        None,
    )
    .map_err(|_| invalid("source identity is invalid"))?;
    Ok(())
}

fn validate_quality(value: &PredictionQualityV1) -> Result<(), PredictionEnvelopeV1Error> {
    if value.status == PredictionQualityStatusV1::Unspecified
        || value
            .receipt_sha256
            .as_deref()
            .is_some_and(|hash| !valid_sha256(hash))
    {
        return Err(invalid("quality status or receipt hash is invalid"));
    }
    if value.status == PredictionQualityStatusV1::Pass && value.receipt_sha256.is_none() {
        return Err(invalid("PASS quality requires a receipt hash reference"));
    }
    if value.status != PredictionQualityStatusV1::Pass && value.reason_codes.is_empty() {
        return Err(invalid(
            "non-PASS quality requires at least one reason code",
        ));
    }
    let mut unique_reasons = HashSet::new();
    for reason in &value.reason_codes {
        if !valid_identity(reason) || !unique_reasons.insert(reason.as_str()) {
            return Err(invalid("quality reason codes must be valid and unique"));
        }
    }
    Ok(())
}

fn validate_evidence(value: &PredictionEvidenceV1) -> Result<(), PredictionEnvelopeV1Error> {
    for hash in [
        value.train_sha256.as_str(),
        value.validation_sha256.as_str(),
        value.oos_freeze_sha256.as_str(),
        value.selection_sha256.as_str(),
        value.code_sha256.as_str(),
        value.source_manifest_sha256.as_str(),
    ] {
        if !valid_sha256(hash) {
            return Err(invalid("prediction evidence contains an invalid SHA-256"));
        }
    }
    if value
        .oos_sha256
        .as_deref()
        .is_some_and(|hash| !valid_sha256(hash))
    {
        return Err(invalid("OOS evidence hash is invalid"));
    }
    if value.oos_used_for_selection {
        return Err(invalid("OOS evidence must remain report-only"));
    }
    Ok(())
}

fn validate_forecast(value: &ForecastSnapshotV2) -> Result<(), PredictionEnvelopeV1Error> {
    if value.schema_version != FORECAST_SCHEMA
        || value.owner != "QLIB_RESEARCH"
        || value.consumer != "LEAN"
        || !matches!(value.producer.as_str(), "QLIB" | "RD_AGENT")
        || value.evidence_scope != "RESEARCH"
        || !matches!(
            value.evidence_origin.as_str(),
            "LOCAL"
                | "INJECTED"
                | "SIMULATED_ONLY"
                | "UNVERIFIED"
                | "OPERATOR_SUPPLIED"
                | "COMPONENT_RUNTIME"
                | "PROVIDER_HISTORICAL"
                | "WEBHOOK_SHADOW"
        )
    {
        return Err(invalid("forecast schema or research identity is invalid"));
    }

    for (identity, field) in [
        (&value.forecast_id, "forecast_id"),
        (&value.strategy_id, "strategy_id"),
        (&value.model_id, "forecast.model_id"),
        (&value.feature_set_id, "feature_set_id"),
        (&value.universe_id, "universe_id"),
        (&value.session_id, "session_id"),
        (&value.session_policy, "session_policy"),
        (&value.label_spec_id, "label_spec_id"),
        (&value.return_basis, "return_basis"),
    ] {
        require_identity(identity, field)?;
    }
    require_source_version_token(&value.model_version, "forecast.model_version")?;
    require_source_version_token(&value.feature_version, "feature_version")?;
    if !valid_stock_symbol(&value.symbol)
        || !valid_frequency(&value.input_resolution)
        || !valid_frequency(&value.inference_frequency)
        || !valid_frequency(&value.decision_frequency)
        || !matches!(
            value.price_basis.as_str(),
            "RAW" | "SPLIT_ADJUSTED" | "TOTAL_RETURN" | "TOTAL_RETURN_CASH"
        )
    {
        return Err(invalid(
            "forecast symbol, frequency, or price basis is invalid",
        ));
    }
    for hash in [
        value.label_spec_sha256.as_str(),
        value.model_sha256.as_str(),
        value.feature_sha256.as_str(),
        value.dataset_sha256.as_str(),
        value.code_sha256.as_str(),
    ] {
        if !valid_sha256(hash) {
            return Err(invalid("forecast contains an invalid SHA-256"));
        }
    }
    if value
        .prediction_interval_sha256
        .as_deref()
        .is_some_and(|hash| !valid_sha256(hash))
        || value.sequence == 0
    {
        return Err(invalid("forecast interval hash or sequence is invalid"));
    }
    value.forecast_horizon.validate()?;
    if value.forecast_horizon.label_spec_id != value.label_spec_id {
        return Err(invalid(
            "forecast horizon label differs from the forecast label",
        ));
    }
    validate_decimal(&value.raw_score)?;
    if let Some(expected_return) = &value.expected_return_bps {
        validate_decimal(expected_return)?;
    }
    if let Some(confidence) = &value.confidence {
        let parsed = validate_decimal(confidence)?;
        if parsed < ExactDecimal::ZERO || parsed > ExactDecimal::from_integer(1) {
            return Err(invalid("forecast confidence must be between zero and one"));
        }
    }
    if let Some(uncertainty) = &value.uncertainty_bps
        && validate_decimal(uncertainty)? < ExactDecimal::ZERO
    {
        return Err(invalid("forecast uncertainty must be non-negative"));
    }
    if !(value.event_time <= value.knowledge_at
        && value.knowledge_at <= value.available_at
        && value.available_at <= value.decision_at
        && value.decision_at <= value.valid_from
        && value.valid_from < value.valid_until)
    {
        return Err(invalid(
            "forecast clocks violate causal ordering or validity interval",
        ));
    }
    Ok(())
}

fn validate_decimal(value: &DecimalString) -> Result<ExactDecimal, PredictionEnvelopeV1Error> {
    ExactDecimal::parse_json_number(value.as_str())
        .map_err(|_| invalid("forecast decimal exceeds the exact-decimal contract"))
}

fn require_hash(value: &str, field: &'static str) -> Result<(), PredictionEnvelopeV1Error> {
    if valid_sha256(value) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn require_identity(value: &str, field: &'static str) -> Result<(), PredictionEnvelopeV1Error> {
    if valid_identity(value) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn require_source_version_token(
    value: &str,
    field: &'static str,
) -> Result<(), PredictionEnvelopeV1Error> {
    if valid_source_version_token(value) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn require_dataset_id(value: &str) -> Result<(), PredictionEnvelopeV1Error> {
    if crate::dataset::stable_identity(value, 256) {
        Ok(())
    } else {
        Err(invalid(
            "train_dataset_version is not an immutable dataset identity",
        ))
    }
}

fn valid_identity(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 255
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'@' | b'+' | b'-')
        })
        || value.contains('*')
        || value.contains('?')
        || value.contains('[')
        || value.contains(']')
    {
        return false;
    }
    let lowercase = value.to_ascii_lowercase();
    !["latest", "fallback"].iter().any(|selector| {
        lowercase
            .split(['-', '_', '.', ':', '/', ' '])
            .any(|component| component == *selector)
    })
}

// Mirrors quant-research's current `_SEMVER_RE` plus stable-selector rules. This is its
// compatibility syntax, not a SemVer 2.0.0 validator; see prediction-envelope-v1.md.
fn valid_source_version_token(value: &str) -> bool {
    let suffix_start = value.find(['-', '+']).unwrap_or(value.len());
    let core = &value[..suffix_start];
    let mut parts = core.split('.');
    let numeric_core = parts.clone().count() == 3
        && parts
            .by_ref()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !numeric_core || !valid_identity(value) {
        return false;
    }
    let suffix = &value[suffix_start..];
    suffix.is_empty()
        || (suffix.len() > 1
            && matches!(suffix.as_bytes()[0], b'-' | b'+')
            && suffix[1..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')))
}

fn valid_stock_symbol(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 16
        && bytes[0].is_ascii_uppercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_frequency(value: &str) -> bool {
    matches!(
        value,
        "tick" | "1s" | "1m" | "5m" | "15m" | "30m" | "1h" | "1d" | "EOD"
    )
}

fn valid_sha256(value: &str) -> bool {
    crate::dataset::valid_sha256(value)
}

fn invalid(message: &'static str) -> PredictionEnvelopeV1Error {
    PredictionEnvelopeV1Error::InvalidEnvelope(message)
}

#[cfg(test)]
mod tests {
    use super::{valid_identity, valid_source_version_token};

    #[test]
    fn identity_rules_reject_selectors_and_preserve_supported_punctuation() {
        assert!(valid_identity("quant-platform/model+v1"));
        assert!(!valid_identity("dataset-latest-v1"));
        assert!(!valid_identity("dataset/fallback/v1"));
        assert!(!valid_identity("*"));
        assert!(valid_identity("latest@immutable-version"));
    }

    #[test]
    fn version_rules_match_the_source_compatibility_pattern() {
        for value in ["1.0.0", "1.0.0-rc.1", "1.0.0+build.5", "01.2.3", "1.2.3-.."] {
            assert!(
                valid_source_version_token(value),
                "expected source-compatible version token: {value}"
            );
        }
        for value in [
            "1.0.0-",
            "1.0.0+",
            "1.0",
            "1.0.0/latest",
            "1.2.3-alpha+build.2",
        ] {
            assert!(
                !valid_source_version_token(value),
                "expected rejected source version token: {value}"
            );
        }
    }
}
