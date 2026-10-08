//! Canonical fingerprints for the Parquet logical schemas owned by this workspace.
//!
//! Fingerprints are over a small versioned descriptor, not a Rust/Python Arrow display string or
//! Parquet file metadata. Producers and consumers must match the descriptor to a trusted schema
//! ID before treating a manifest fingerprint as evidence of a particular dataset shape.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;
use thiserror::Error;

/// Prefix/domain separator prepended to canonical descriptor JSON before hashing.
pub const PARQUET_SCHEMA_FINGERPRINT_PREFIX: &str = "LQEpoch-Parquet-Schema-v1\n";
/// Optional Arrow/Parquet metadata key containing the registered canonical descriptor JSON.
pub const PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY: &str = "lqepoch.schema_descriptor.v1";
/// Optional Arrow/Parquet metadata key containing the registered lowercase fingerprint.
pub const PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY: &str = "lqepoch.schema_fingerprint_sha256";
/// Trusted schema ID for one normalized market event per Parquet row.
pub const MARKET_EVENT_PARQUET_SCHEMA_ID: &str = "lqepoch.market_event.v1";
/// Trusted schema ID for one-minute US equity trade bars.
pub const US_EQUITY_TRADE_BAR_1M_SCHEMA_ID: &str = "lqepoch.us_equity_trade_bar_1m.v1";
/// Trusted schema ID for one-minute US equity trade bars bound to manifest completion evidence.
pub const US_EQUITY_TRADE_BAR_1M_V2_SCHEMA_ID: &str = "lqepoch.us_equity_trade_bar_1m.v2";
/// Trusted schema ID for byte-exact provider MessagePack frames.
pub const MARKET_RAW_FRAME_PARQUET_SCHEMA_ID: &str = "lqepoch.market_raw_frame.v1";
/// Trusted schema ID for byte-exact provider JSON frames.
pub const MARKET_RAW_JSON_FRAME_PARQUET_SCHEMA_ID: &str = "lqepoch.market_raw_json_frame.v1";
/// Storage schema ID for normalized market events with raw-frame correlation.
pub const MARKET_EVENT_PARQUET_SCHEMA_V2_ID: &str = "lqepoch.market_event.v2";
/// Trusted schema ID for byte-exact MessagePack frames bound to a capture instance.
pub const MARKET_RAW_FRAME_PARQUET_SCHEMA_V2_ID: &str = "lqepoch.market_raw_frame.v2";
/// Trusted schema ID for byte-exact JSON frames bound to a capture instance.
pub const MARKET_RAW_JSON_FRAME_PARQUET_SCHEMA_V2_ID: &str = "lqepoch.market_raw_json_frame.v2";
/// Storage schema ID for normalized events with capture-instance correlation.
pub const MARKET_EVENT_PARQUET_SCHEMA_V3_ID: &str = "lqepoch.market_event.v3";

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
    #[error("schema metadata contains only one of the registered descriptor and fingerprint")]
    IncompleteSchemaMetadata,
    #[error("schema metadata differs from the registered descriptor or fingerprint")]
    SchemaMetadataMismatch,
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
    trusted_schemas()
        .get(schema_id)
        .cloned()
        .ok_or(ParquetSchemaError::UnknownSchemaId)
}

#[derive(Deserialize)]
struct TrustedRegistryFixture {
    schemas: Vec<TrustedRegistryEntry>,
}

#[derive(Deserialize)]
struct TrustedRegistryEntry {
    descriptor: ParquetSchemaDescriptorV1,
    canonical_json: String,
    sha256: String,
}

/// Read the checked-in registry once so the fixture remains the sole trusted descriptor source.
fn trusted_schemas() -> &'static HashMap<String, ParquetSchemaDescriptorV1> {
    static SCHEMAS: OnceLock<HashMap<String, ParquetSchemaDescriptorV1>> = OnceLock::new();
    SCHEMAS.get_or_init(|| {
        let registry: TrustedRegistryFixture = serde_json::from_str(include_str!(
            "../../../schemas/fixtures/parquet-schema-registry.json"
        ))
        .expect("checked-in Parquet schema registry must parse");
        let mut schemas = HashMap::with_capacity(registry.schemas.len());
        for entry in registry.schemas {
            let descriptor = entry.descriptor;
            assert_eq!(
                descriptor.canonical_json().as_deref(),
                Ok(entry.canonical_json.as_str()),
                "trusted schema canonical JSON must match its descriptor"
            );
            assert_eq!(
                descriptor.fingerprint_sha256().as_deref(),
                Ok(entry.sha256.as_str()),
                "trusted schema digest must match its canonical bytes"
            );
            let schema_id = descriptor.schema_id.clone();
            assert!(
                schemas.insert(schema_id, descriptor).is_none(),
                "trusted Parquet schema IDs must be unique"
            );
        }
        schemas
    })
}

/// Compute a trusted schema's fingerprint by its registered semantic identity.
pub fn trusted_schema_fingerprint(schema_id: &str) -> Result<String, ParquetSchemaError> {
    let schema = trusted_parquet_schema(schema_id)?;
    schema.fingerprint_sha256()
}

/// Return the canonical Arrow/Parquet metadata pair computed from the trusted registry.
///
/// Writers should attach both entries to the Arrow schema and, where supported, the flat Parquet
/// footer. These entries are outside the logical fingerprint and therefore do not change a
/// schema's SHA-256.
pub fn trusted_parquet_schema_metadata(
    schema_id: &str,
) -> Result<BTreeMap<String, String>, ParquetSchemaError> {
    let schema = trusted_parquet_schema(schema_id)?;
    let canonical_json = schema.canonical_json()?;
    let fingerprint = schema.fingerprint_sha256()?;
    Ok(BTreeMap::from([
        (
            PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY.to_owned(),
            canonical_json,
        ),
        (
            PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.to_owned(),
            fingerprint,
        ),
    ]))
}

/// Validate optional schema metadata while accepting legacy files with no registry metadata.
///
/// Unrelated metadata is ignored. If either registry-owned key is present, both must be present
/// and byte-for-byte equal to the values derived from the trusted descriptor.
pub fn validate_optional_parquet_schema_metadata(
    schema_id: &str,
    metadata: Option<&HashMap<String, String>>,
) -> Result<(), ParquetSchemaError> {
    let schema = trusted_parquet_schema(schema_id)?;
    let Some(metadata) = metadata else {
        return Ok(());
    };
    let descriptor = metadata.get(PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY);
    let fingerprint = metadata.get(PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY);
    match (descriptor, fingerprint) {
        (None, None) => Ok(()),
        (Some(_), None) | (None, Some(_)) => Err(ParquetSchemaError::IncompleteSchemaMetadata),
        (Some(descriptor), Some(fingerprint)) => {
            let trusted_descriptor = schema.canonical_json()?;
            let trusted_fingerprint = schema.fingerprint_sha256()?;
            if descriptor == &trusted_descriptor && fingerprint == &trusted_fingerprint {
                Ok(())
            } else {
                Err(ParquetSchemaError::SchemaMetadataMismatch)
            }
        }
    }
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
        "binary"
            | "bool"
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

#[cfg(test)]
mod tests {
    use super::{
        PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY, PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY,
        ParquetSchemaDescriptorV1, ParquetSchemaError, ParquetSchemaFieldV1,
        trusted_parquet_schema, trusted_parquet_schema_metadata, trusted_schema_fingerprint,
        validate_optional_parquet_schema_metadata,
    };
    use serde::Deserialize;
    use std::collections::HashMap;

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
            "../../../schemas/fixtures/parquet-schema-registry.json"
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
        let base = trusted_parquet_schema(super::MARKET_EVENT_PARQUET_SCHEMA_ID).unwrap();
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

        let mut invalid = trusted_parquet_schema(super::MARKET_EVENT_PARQUET_SCHEMA_ID).unwrap();
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
            "../../../schemas/fixtures/parquet-schema-registry.json"
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

    #[test]
    fn optional_parquet_schema_metadata_is_legacy_compatible_and_strict_when_present() {
        let schema_id = super::MARKET_RAW_FRAME_PARQUET_SCHEMA_ID;
        assert_eq!(
            validate_optional_parquet_schema_metadata(schema_id, None),
            Ok(())
        );

        let unrelated = HashMap::from([("writer".to_owned(), "synthetic".to_owned())]);
        assert_eq!(
            validate_optional_parquet_schema_metadata(schema_id, Some(&unrelated)),
            Ok(())
        );

        let trusted = trusted_parquet_schema_metadata(schema_id).unwrap();
        let mut complete = HashMap::from_iter(trusted);
        assert_eq!(
            validate_optional_parquet_schema_metadata(schema_id, Some(&complete)),
            Ok(())
        );

        complete.remove(PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY);
        assert_eq!(
            validate_optional_parquet_schema_metadata(schema_id, Some(&complete)),
            Err(ParquetSchemaError::IncompleteSchemaMetadata)
        );

        complete.insert(
            PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.to_owned(),
            "0".repeat(64),
        );
        assert_eq!(
            validate_optional_parquet_schema_metadata(schema_id, Some(&complete)),
            Err(ParquetSchemaError::SchemaMetadataMismatch)
        );

        let trusted = trusted_parquet_schema_metadata(schema_id).unwrap();
        let wrong_descriptor = HashMap::from([
            (
                PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY.to_owned(),
                "{}".to_owned(),
            ),
            (
                PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY.to_owned(),
                trusted[PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY].clone(),
            ),
        ]);
        assert_eq!(
            validate_optional_parquet_schema_metadata(schema_id, Some(&wrong_descriptor)),
            Err(ParquetSchemaError::SchemaMetadataMismatch)
        );
    }
}
