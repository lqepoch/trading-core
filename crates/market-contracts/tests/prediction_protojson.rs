use market_contracts::{
    MAX_PREDICTION_ENVELOPE_V1_JSON_BYTES, PredictionEnvelopeV1,
    parse_prediction_envelope_v1_protojson,
};
use serde::Deserialize;
use serde_json::Value;

const EXPORTED_FIXTURE: &[u8] =
    include_bytes!("../../../schemas/fixtures/prediction-envelope-v1-quant-export.json");
const NANOSECOND_FIXTURE: &[u8] =
    include_bytes!("../../../schemas/fixtures/prediction-envelope-v1-nanosecond.json");
const SNAKE_FIXTURE: &[u8] =
    include_bytes!("../../../schemas/fixtures/prediction-envelope-v1-snake.json");
const CASES_FIXTURE: &[u8] =
    include_bytes!("../../../schemas/fixtures/prediction-envelope-v1-protojson-cases.json");

#[derive(Deserialize)]
struct ProtoJsonCases {
    invalid_mutations: Vec<InvalidMutation>,
    rust_invalid_removed_fields: Vec<RemovedField>,
    invalid_text_replacements: Vec<TextReplacement>,
}

#[derive(Deserialize)]
struct InvalidMutation {
    name: String,
    path: Vec<String>,
    value: Value,
}

#[derive(Deserialize)]
struct TextReplacement {
    name: String,
    needle: String,
    replacement: String,
}

#[derive(Deserialize)]
struct RemovedField {
    name: String,
    path: Vec<String>,
}

fn cases() -> ProtoJsonCases {
    serde_json::from_slice(CASES_FIXTURE).expect("shared prediction cases are valid JSON")
}

fn set_path(document: &mut Value, path: &[String], value: Value) {
    assert!(!path.is_empty());
    let mut current = document;
    for segment in &path[..path.len() - 1] {
        current = current
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
            .unwrap_or_else(|| panic!("fixture path segment is missing: {segment}"));
    }
    current
        .as_object_mut()
        .expect("path parent is an object")
        .insert(path.last().unwrap().clone(), value);
}

fn remove_path(document: &mut Value, path: &[String]) {
    assert!(!path.is_empty());
    let mut current = document;
    for segment in &path[..path.len() - 1] {
        current = current
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
            .unwrap_or_else(|| panic!("fixture path segment is missing: {segment}"));
    }
    let removed = current
        .as_object_mut()
        .expect("path parent is an object")
        .remove(path.last().unwrap());
    assert!(removed.is_some(), "fixture field to remove is missing");
}

#[test]
fn quant_export_fixture_preserves_maximum_uint64_and_typed_bindings() {
    let prediction = parse_prediction_envelope_v1_protojson(EXPORTED_FIXTURE).unwrap();
    assert_eq!(prediction.forecast.sequence, u64::MAX);
    assert_eq!(
        prediction.created_at.as_str(),
        "2026-01-05T14:30:05.123456Z"
    );
    assert_eq!(prediction.forecast.raw_score.as_str(), "0.2");
    assert_eq!(
        prediction.quality.status,
        market_contracts::PredictionQualityStatusV1::Pass
    );
    assert!(!prediction.evidence.oos_used_for_selection);
}

#[test]
fn quant_export_fixture_derivative_preserves_nine_timestamp_fraction_digits() {
    let prediction = parse_prediction_envelope_v1_protojson(NANOSECOND_FIXTURE).unwrap();
    assert_eq!(
        prediction.created_at.as_str(),
        "2026-01-05T14:30:05.123456789Z"
    );
}

#[test]
fn snake_case_protojson_aliases_are_accepted_without_changing_the_typed_value() {
    let camel: PredictionEnvelopeV1 =
        parse_prediction_envelope_v1_protojson(EXPORTED_FIXTURE).unwrap();
    let snake: PredictionEnvelopeV1 =
        parse_prediction_envelope_v1_protojson(SNAKE_FIXTURE).unwrap();
    assert_eq!(camel, snake);
}

#[test]
fn shared_protojson_mutations_fail_closed() {
    let fixture: Value = serde_json::from_slice(EXPORTED_FIXTURE).unwrap();
    for mutation in cases().invalid_mutations {
        let mut document = fixture.clone();
        set_path(&mut document, &mutation.path, mutation.value);
        let bytes = serde_json::to_vec(&document).unwrap();
        assert!(
            parse_prediction_envelope_v1_protojson(&bytes).is_err(),
            "invalid shared case was accepted: {}",
            mutation.name
        );
    }
}

#[test]
fn required_source_quality_and_horizon_enums_cannot_fall_back_to_proto_defaults() {
    let fixture: Value = serde_json::from_slice(EXPORTED_FIXTURE).unwrap();
    for missing in cases().rust_invalid_removed_fields {
        let mut document = fixture.clone();
        remove_path(&mut document, &missing.path);
        assert!(
            parse_prediction_envelope_v1_protojson(&serde_json::to_vec(&document).unwrap())
                .is_err(),
            "missing shared field was accepted: {}",
            missing.name
        );
    }
}

#[test]
fn duplicate_json_keys_and_mixed_protojson_aliases_are_rejected_before_projection() {
    let source = std::str::from_utf8(EXPORTED_FIXTURE).unwrap();
    for replacement in cases().invalid_text_replacements {
        let invalid = source.replacen(&replacement.needle, &replacement.replacement, 1);
        assert_ne!(
            invalid, source,
            "fixture text needle missing: {}",
            replacement.name
        );
        assert!(
            parse_prediction_envelope_v1_protojson(invalid.as_bytes()).is_err(),
            "invalid shared text case was accepted: {}",
            replacement.name
        );
    }
}

#[test]
fn malformed_bindings_and_unbounded_documents_are_rejected() {
    let mut mismatch: Value = serde_json::from_slice(EXPORTED_FIXTURE).unwrap();
    set_path(
        &mut mismatch,
        &["source".into(), "datasetSha256".into()],
        Value::String("b".repeat(64)),
    );
    assert!(
        parse_prediction_envelope_v1_protojson(&serde_json::to_vec(&mismatch).unwrap()).is_err()
    );

    let oversized = vec![b' '; MAX_PREDICTION_ENVELOPE_V1_JSON_BYTES + 1];
    assert!(parse_prediction_envelope_v1_protojson(&oversized).is_err());
}

#[test]
fn large_reason_code_sets_are_bounded_and_reject_a_trailing_duplicate() {
    let mut document: Value = serde_json::from_slice(EXPORTED_FIXTURE).unwrap();
    let reasons = (0..100_000)
        .map(|index| Value::String(format!("reason-{index:06}")))
        .collect::<Vec<_>>();
    document["quality"]["reasonCodes"] = Value::Array(reasons);
    let bytes = serde_json::to_vec(&document).unwrap();
    assert!(bytes.len() < MAX_PREDICTION_ENVELOPE_V1_JSON_BYTES);

    let mut duplicate: Value = serde_json::from_slice(&bytes).unwrap();
    let values = duplicate["quality"]["reasonCodes"]
        .as_array_mut()
        .expect("reason codes are an array");
    let repeated = values[0].clone();
    values.push(repeated);
    let duplicate_bytes = serde_json::to_vec(&duplicate).unwrap();
    assert!(duplicate_bytes.len() < MAX_PREDICTION_ENVELOPE_V1_JSON_BYTES);
    assert!(parse_prediction_envelope_v1_protojson(&duplicate_bytes).is_err());
}
