//! Lossless JSON projection for unsigned 64-bit protocol values.

use serde::{Deserialize, Deserializer, Serializer, de::Error};

/// Serialize an unsigned 64-bit integer as a canonical decimal string.
pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Deserialize only canonical decimal strings, rejecting JSON numbers that may lose JS precision.
pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    let value = text.parse::<u64>().map_err(D::Error::custom)?;
    if value.to_string() != text {
        return Err(D::Error::custom(
            "uint64 string must use canonical decimal form",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::Value as JsonValue;

    #[derive(Deserialize)]
    struct U64Fixture {
        valid: Vec<NamedValue>,
        invalid: Vec<NamedValue>,
    }

    #[derive(Deserialize)]
    struct NamedValue {
        value: JsonValue,
    }

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Value {
        #[serde(with = "super")]
        value: u64,
    }

    #[test]
    fn uint64_json_is_canonical_decimal_string_and_rejects_lossy_inputs() {
        let value = Value { value: u64::MAX };
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"value":"18446744073709551615"}"#
        );
        assert_eq!(
            serde_json::from_str::<Value>(r#"{"value":"18446744073709551615"}"#).unwrap(),
            value
        );
        assert!(serde_json::from_str::<Value>(r#"{"value":1}"#).is_err());
        assert!(serde_json::from_str::<Value>(r#"{"value":"01"}"#).is_err());
        assert!(serde_json::from_str::<Value>(r#"{"value":"18446744073709551616"}"#).is_err());
    }

    #[test]
    fn shared_uint64_fixture_roundtrips_and_rejects_ambiguous_json() {
        let fixture: U64Fixture = serde_json::from_str(include_str!(
            "../../../schemas/fixtures/uint64-json-v1.json"
        ))
        .unwrap();

        for case in fixture.valid {
            let input = serde_json::json!({"value": case.value});
            let parsed: Value = serde_json::from_value(input).unwrap();
            let reencoded = serde_json::to_value(parsed).unwrap();
            assert_eq!(reencoded["value"], case.value);
        }
        for case in fixture.invalid {
            let input = serde_json::json!({"value": case.value});
            assert!(serde_json::from_value::<Value>(input).is_err());
        }
    }
}
