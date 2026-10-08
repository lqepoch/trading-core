//! Canonical fingerprints for the Parquet logical schemas owned by this workspace.
//!
//! Fingerprints are over a small versioned descriptor, not a Rust/Python Arrow display string or
//! Parquet file metadata. Producers and consumers must match the descriptor to a trusted schema
//! ID before treating a manifest fingerprint as evidence of a particular dataset shape.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

/// Prefix/domain separator prepended to canonical descriptor JSON before hashing.
pub const PARQUET_SCHEMA_FINGERPRINT_PREFIX: &str = "LQEpoch-Parquet-Schema-v1\n";
/// Trusted schema ID for one normalized market event per Parquet row.
pub const MARKET_EVENT_PARQUET_SCHEMA_ID: &str = "lqepoch.market_event.v1";
/// Trusted schema ID for one-minute US equity trade bars.
pub const US_EQUITY_TRADE_BAR_1M_SCHEMA_ID: &str = "lqepoch.us_equity_trade_bar_1m.v1";

/// Validate a row-level SHA-256 logical value as lowercase hexadecimal.
pub fn validate_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A canonical schema descriptor. JSON object keys serialize in lexical order; field array order
/// is significant and preserves the Parquet column order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParquetSchemaDescriptorV1 {
    pub fields: Vec<ParquetSchemaFieldV1>,
    pub schema_id: String,
    pub schema_version: u32,
}

/// One ordered Parquet leaf column and its cross-language logical type.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParquetSchemaFieldV1 {
    pub name: String,
    pub nullable: bool,
    #[serde(rename = "type")]
    pub logical_type: String,
}

/// Errors from constructing or validating a trusted schema fingerprint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum ParquetSchemaError {
    #[error("unsupported Parquet schema descriptor version")]
    UnsupportedVersion,
    #[error("invalid schema ID or column name")]
    InvalidName,
    #[error("schema descriptor has no columns or duplicate names")]
    InvalidFields,
    #[error("unsupported cross-language logical type")]
    UnsupportedLogicalType,
    #[error("schema ID is not registered")]
    UnknownSchemaId,
    #[error("schema descriptor does not match its trusted schema ID")]
    TrustedSchemaMismatch,
}

impl ParquetSchemaDescriptorV1 {
    /// Validate the descriptor grammar and serialize compact JSON with lexically ordered keys.
    pub fn canonical_json(&self) -> Result<String, ParquetSchemaError> {
        self.validate_shape()?;
        serde_json::to_string(self).map_err(|_| ParquetSchemaError::InvalidFields)
    }

    /// Hash the descriptor bytes after the frozen versioned domain separator.
    pub fn fingerprint_sha256(&self) -> Result<String, ParquetSchemaError> {
        let json = self.canonical_json()?;
        let mut hasher = Sha256::new();
        hasher.update(PARQUET_SCHEMA_FINGERPRINT_PREFIX.as_bytes());
        hasher.update(json.as_bytes());
        Ok(lower_hex(&hasher.finalize()))
    }

    /// Require exact equality with the descriptor registered for `schema_id`.
    pub fn validate_trusted_schema(&self) -> Result<(), ParquetSchemaError> {
        self.validate_shape()?;
        let trusted = trusted_parquet_schema(&self.schema_id)?;
        if self == &trusted {
            Ok(())
        } else {
            Err(ParquetSchemaError::TrustedSchemaMismatch)
        }
    }

    fn validate_shape(&self) -> Result<(), ParquetSchemaError> {
        if self.schema_version != 1 {
            return Err(ParquetSchemaError::UnsupportedVersion);
        }
        if !valid_token(&self.schema_id, 128) {
            return Err(ParquetSchemaError::InvalidName);
        }
        if self.fields.is_empty() {
            return Err(ParquetSchemaError::InvalidFields);
        }
        let mut names = HashSet::with_capacity(self.fields.len());
        for field in &self.fields {
            if !valid_field_name(&field.name) || !names.insert(field.name.as_str()) {
                return Err(ParquetSchemaError::InvalidFields);
            }
            if !is_supported_logical_type(&field.logical_type) {
                return Err(ParquetSchemaError::UnsupportedLogicalType);
            }
        }
        Ok(())
    }
}

/// Return one of the schema descriptors registered by trading-core.
pub fn trusted_parquet_schema(
    schema_id: &str,
) -> Result<ParquetSchemaDescriptorV1, ParquetSchemaError> {
    let columns: &[(&str, &str, bool)] = match schema_id {
        MARKET_EVENT_PARQUET_SCHEMA_ID => MARKET_EVENT_FIELDS,
        US_EQUITY_TRADE_BAR_1M_SCHEMA_ID => US_EQUITY_TRADE_BAR_1M_FIELDS,
        _ => return Err(ParquetSchemaError::UnknownSchemaId),
    };
    Ok(ParquetSchemaDescriptorV1 {
        fields: columns
            .iter()
            .map(|(name, logical_type, nullable)| ParquetSchemaFieldV1 {
                name: (*name).to_owned(),
                nullable: *nullable,
                logical_type: (*logical_type).to_owned(),
            })
            .collect(),
        schema_id: schema_id.to_owned(),
        schema_version: 1,
    })
}

/// Compute a trusted schema's fingerprint by its registered semantic identity.
pub fn trusted_schema_fingerprint(schema_id: &str) -> Result<String, ParquetSchemaError> {
    let schema = trusted_parquet_schema(schema_id)?;
    schema.fingerprint_sha256()
}

fn valid_token(value: &str, max_bytes: usize) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value.len() <= max_bytes
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn valid_field_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && value.len() <= 128
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_supported_logical_type(value: &str) -> bool {
    matches!(
        value,
        "bool"
            | "date_iso8601"
            | "decimal_string"
            | "sha256_hex"
            | "timestamp_ns_utc"
            | "uint32"
            | "uint64"
            | "utf8"
    )
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

const MARKET_EVENT_FIELDS: &[(&str, &str, bool)] = &[
    ("schema_version", "uint32", false),
    ("provider", "utf8", false),
    ("feed", "utf8", false),
    ("entitlement", "utf8", false),
    ("numeric_encoding", "utf8", false),
    ("source_record_id", "utf8", true),
    ("raw_frame_sha256", "sha256_hex", true),
    ("generation", "uint64", false),
    ("sequence", "uint64", false),
    ("source_timestamp", "timestamp_ns_utc", true),
    ("received_timestamp", "timestamp_ns_utc", false),
    ("event_kind", "utf8", false),
    ("symbol", "utf8", false),
    ("price", "decimal_string", true),
    ("size", "decimal_string", true),
    ("bid", "decimal_string", true),
    ("ask", "decimal_string", true),
    ("bid_size", "decimal_string", true),
    ("ask_size", "decimal_string", true),
];

const US_EQUITY_TRADE_BAR_1M_FIELDS: &[(&str, &str, bool)] = &[
    ("schema_version", "uint32", false),
    ("source_provider", "utf8", false),
    ("source_feed", "utf8", false),
    ("source_entitlement", "utf8", false),
    ("source_numeric_encoding", "utf8", false),
    ("symbol", "utf8", false),
    ("bar_start_utc", "timestamp_ns_utc", false),
    ("bar_end_exclusive_utc", "timestamp_ns_utc", false),
    ("available_at_utc", "timestamp_ns_utc", false),
    ("trade_date", "date_iso8601", false),
    ("session_id", "utf8", false),
    ("session_timezone", "utf8", false),
    ("session_policy_id", "utf8", false),
    ("session_policy_sha256", "sha256_hex", false),
    ("session_start_utc", "timestamp_ns_utc", false),
    ("session_end_exclusive_utc", "timestamp_ns_utc", false),
    ("window_start_utc", "timestamp_ns_utc", false),
    ("window_end_exclusive_utc", "timestamp_ns_utc", false),
    ("open", "decimal_string", false),
    ("high", "decimal_string", false),
    ("low", "decimal_string", false),
    ("close", "decimal_string", false),
    ("volume", "decimal_string", false),
    ("trade_count", "uint64", false),
    ("quote_events_excluded", "uint64", false),
    ("source_timestamp_missing_rows", "uint64", false),
    ("sequence_gap_count", "uint64", false),
    ("late_event_count", "uint64", false),
    ("window_expected_minutes", "uint64", false),
    ("window_empty_trade_minutes", "uint64", false),
    ("source_start_utc", "timestamp_ns_utc", false),
    ("source_end_exclusive_utc", "timestamp_ns_utc", false),
    ("window_input_eof", "bool", false),
    ("source_pages_exhausted", "bool", true),
    ("completion_mode", "utf8", false),
    ("nbbo_input_status", "utf8", false),
];

#[cfg(test)]
mod tests {
    use super::{
        ParquetSchemaDescriptorV1, ParquetSchemaError, ParquetSchemaFieldV1,
        trusted_parquet_schema, trusted_schema_fingerprint,
    };
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GoldenFixture {
        fingerprint_prefix: String,
        generic_golden: GoldenSchema,
        schemas: Vec<GoldenSchema>,
        invalid_descriptors: Vec<InvalidDescriptor>,
    }

    #[derive(Deserialize)]
    struct InvalidDescriptor {
        name: String,
        descriptor: serde_json::Value,
    }

    #[derive(Deserialize)]
    struct GoldenSchema {
        descriptor: ParquetSchemaDescriptorV1,
        canonical_json: String,
        sha256: String,
    }

    #[test]
    fn trusted_schema_descriptors_match_shared_canonical_hash_fixture() {
        let fixture: GoldenFixture = serde_json::from_str(include_str!(
            "../../../schemas/fixtures/parquet-schema-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture.fingerprint_prefix, "LQEpoch-Parquet-Schema-v1\n");
        verify_golden(fixture.generic_golden, false);
        for golden in fixture.schemas {
            verify_golden(golden, true);
        }
    }

    fn verify_golden(golden: GoldenSchema, trusted: bool) {
        if trusted {
            golden.descriptor.validate_trusted_schema().unwrap();
            assert_eq!(
                trusted_schema_fingerprint(&golden.descriptor.schema_id).unwrap(),
                golden.sha256
            );
            assert_eq!(
                trusted_parquet_schema(&golden.descriptor.schema_id).unwrap(),
                golden.descriptor
            );
        }
        assert_eq!(
            golden.descriptor.canonical_json().unwrap(),
            golden.canonical_json
        );
        assert_eq!(
            golden.descriptor.fingerprint_sha256().unwrap(),
            golden.sha256
        );
    }

    #[test]
    fn schema_fingerprint_changes_when_order_type_width_or_nullability_changes() {
        let base = trusted_parquet_schema("lqepoch.market_event.v1").unwrap();
        let base_hash = base.fingerprint_sha256().unwrap();

        let mut reordered = base.clone();
        reordered.fields.swap(1, 2);
        assert_ne!(reordered.fingerprint_sha256().unwrap(), base_hash);
        assert_eq!(
            reordered.validate_trusted_schema(),
            Err(ParquetSchemaError::TrustedSchemaMismatch)
        );

        let mut narrower_integer = base.clone();
        narrower_integer
            .fields
            .iter_mut()
            .find(|field| field.name == "generation")
            .unwrap()
            .logical_type = "uint32".to_owned();
        assert_ne!(narrower_integer.fingerprint_sha256().unwrap(), base_hash);

        let mut required_timestamp = base.clone();
        required_timestamp
            .fields
            .iter_mut()
            .find(|field| field.name == "source_timestamp")
            .unwrap()
            .nullable = false;
        assert_ne!(required_timestamp.fingerprint_sha256().unwrap(), base_hash);
    }

    #[test]
    fn schema_descriptor_rejects_unknown_ids_fields_and_types() {
        assert_eq!(
            trusted_parquet_schema("unregistered.v1"),
            Err(ParquetSchemaError::UnknownSchemaId)
        );

        let mut invalid = trusted_parquet_schema("lqepoch.market_event.v1").unwrap();
        invalid.fields[0].logical_type = "float64".to_owned();
        assert_eq!(
            invalid.canonical_json(),
            Err(ParquetSchemaError::UnsupportedLogicalType)
        );

        let duplicate = ParquetSchemaDescriptorV1 {
            schema_id: "test.v1".to_owned(),
            schema_version: 1,
            fields: vec![
                ParquetSchemaFieldV1 {
                    name: "symbol".to_owned(),
                    nullable: false,
                    logical_type: "utf8".to_owned(),
                },
                ParquetSchemaFieldV1 {
                    name: "symbol".to_owned(),
                    nullable: true,
                    logical_type: "utf8".to_owned(),
                },
            ],
        };
        assert_eq!(
            duplicate.canonical_json(),
            Err(ParquetSchemaError::InvalidFields)
        );

        for schema_id in [".test.v1", ":test.v1", "_test.v1", "-test.v1"] {
            let invalid = ParquetSchemaDescriptorV1 {
                fields: vec![ParquetSchemaFieldV1 {
                    name: "symbol".to_owned(),
                    nullable: false,
                    logical_type: "utf8".to_owned(),
                }],
                schema_id: schema_id.to_owned(),
                schema_version: 1,
            };
            assert_eq!(
                invalid.canonical_json(),
                Err(ParquetSchemaError::InvalidName),
                "schema id {schema_id} must start with ASCII alphanumeric"
            );
        }
    }

    #[test]
    fn shared_invalid_schema_descriptor_fixtures_are_rejected() {
        let fixture: GoldenFixture = serde_json::from_str(include_str!(
            "../../../schemas/fixtures/parquet-schema-v1.json"
        ))
        .unwrap();
        for invalid in fixture.invalid_descriptors {
            let parsed = serde_json::from_value::<ParquetSchemaDescriptorV1>(invalid.descriptor);
            let rejected = match parsed {
                Ok(descriptor) => descriptor.canonical_json().is_err(),
                Err(_) => true,
            };
            assert!(rejected, "accepted invalid descriptor {}", invalid.name);
        }
        assert!(serde_json::from_str::<ParquetSchemaDescriptorV1>(
            r#"{"schema_version":1.0,"schema_id":"test.v1","fields":[{"name":"symbol","nullable":false,"type":"utf8"}]}"#
        )
        .is_err());
    }

    #[test]
    fn sha256_hex_rows_require_lowercase_64_character_values() {
        assert!(super::validate_sha256_hex(&"a".repeat(64)));
        assert!(!super::validate_sha256_hex(&"A".repeat(64)));
        assert!(!super::validate_sha256_hex(&"a".repeat(63)));
        assert!(!super::validate_sha256_hex(&format!("{}g", "a".repeat(63))));
    }
}
