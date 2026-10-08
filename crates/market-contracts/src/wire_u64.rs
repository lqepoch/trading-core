//! Lossless JSON projection for unsigned 64-bit protocol values.

use serde::{Deserialize, Deserializer, Serializer, de::Error};

/// Serialize an unsigned 64-bit integer as a canonical decimal string.
pub(crate) fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Deserialize only canonical decimal strings, rejecting JSON numbers that may lose JS precision.
pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
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
}
