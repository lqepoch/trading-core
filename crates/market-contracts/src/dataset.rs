//! Immutable, read-only market dataset publication contracts.

use crate::{MarketDataSourceV1, UtcTimestamp, v1::valid_identifier};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Dataset manifest schema version.
pub const DATASET_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Failure to validate a published dataset manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum DatasetManifestError {
    #[error("unsupported dataset manifest schema version")]
    UnsupportedVersion,
    #[error("invalid or mutable dataset identity")]
    InvalidDatasetId,
    #[error("source identity is invalid")]
    InvalidSource,
    #[error("symbols must be non-empty, valid, sorted, and unique")]
    InvalidSymbols,
    #[error("dataset source-time range is inconsistent")]
    InvalidTimeRange,
    #[error("dataset row counts are inconsistent")]
    InvalidRowCount,
    #[error("invalid dataset object identity or name")]
    InvalidObject,
    #[error("invalid dataset object hash")]
    InvalidHash,
    #[error("dataset completion evidence is incomplete or inconsistent")]
    InvalidCompletion,
}

/// Transport used for the immutable Parquet object referenced by a manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetTransportV1 {
    /// Object stored through the Google Drive backend of rclone.
    RcloneGoogleDrive,
    /// Synthetic test object addressed by an explicit `local-test:` identity.
    LocalTest,
}

/// Half-open interval of source timestamps present in the dataset.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DatasetTimeRangeV1 {
    /// Earliest represented source timestamp, inclusive.
    pub start_inclusive: UtcTimestamp,
    /// Exclusive upper bound after the latest represented source timestamp.
    pub end_exclusive: UtcTimestamp,
}

/// One immutable Parquet object and its verified storage identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DatasetObjectV1 {
    /// Stable object basename, not a directory path.
    pub object_name: String,
    /// Opaque provider object identity, or an explicit `local-test:` test identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    /// Exact stored object size in bytes.
    #[serde(with = "crate::wire_u64")]
    pub size_bytes: u64,
    /// SHA-256 of the complete stored Parquet object.
    pub content_sha256: String,
    /// SHA-256 fingerprint of the Parquet schema.
    pub parquet_schema_sha256: String,
    /// Row count read from the Parquet footer.
    #[serde(with = "crate::wire_u64")]
    pub parquet_footer_rows: u64,
    /// Storage transport identity.
    pub transport: DatasetTransportV1,
}

/// Completion and readback evidence required before publishing the manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DatasetCompletionEvidenceV1 {
    /// The finite input stream was completely consumed.
    pub input_eof: bool,
    /// For paginated sources, `Some(true)` is required before publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pages_exhausted: Option<bool>,
    /// SHA-256 measured by reading the stored object back.
    pub readback_sha256: String,
    /// The object was verified before this manifest was published.
    pub verified_before_publish: bool,
}

/// Versioned immutable manifest for exactly one Parquet object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DatasetManifestV1 {
    /// Wire schema version; this contract currently accepts only version 1.
    pub schema_version: u32,
    /// Immutable dataset version identity; it is not a mutable dataset family name.
    pub dataset_id: String,
    /// Provider/feed provenance for the rows in this dataset.
    pub source: MarketDataSourceV1,
    /// Exact symbol set, sorted lexically and without duplicates.
    pub symbols: Vec<String>,
    /// Optional source-time coverage interval. Receive time never fills source-time gaps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<DatasetTimeRangeV1>,
    /// Rows whose source timestamp was absent.
    #[serde(with = "crate::wire_u64")]
    pub source_timestamp_missing_rows: u64,
    /// Total data rows in the Parquet object.
    #[serde(with = "crate::wire_u64")]
    pub row_count: u64,
    /// The single immutable stored object described by this manifest.
    pub object: DatasetObjectV1,
    /// Input exhaustion, pagination, and verified readback evidence.
    pub completion: DatasetCompletionEvidenceV1,
}

impl DatasetManifestV1 {
    /// Validate identity, coverage, content-address, and completion invariants.
    pub fn validate(&self) -> Result<(), DatasetManifestError> {
        if self.schema_version != DATASET_MANIFEST_SCHEMA_VERSION {
            return Err(DatasetManifestError::UnsupportedVersion);
        }
        if !stable_identity(&self.dataset_id, 256) {
            return Err(DatasetManifestError::InvalidDatasetId);
        }
        self.source
            .validate()
            .map_err(|_| DatasetManifestError::InvalidSource)?;

        if self.symbols.is_empty()
            || self.symbols.iter().any(|symbol| {
                !valid_identifier(symbol, 256)
                    || symbol.contains('*')
                    || symbol.contains('?')
                    || symbol.eq_ignore_ascii_case("latest")
                    || symbol.eq_ignore_ascii_case("fallback")
            })
            || self.symbols.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(DatasetManifestError::InvalidSymbols);
        }

        if self.row_count == 0 || self.source_timestamp_missing_rows > self.row_count {
            return Err(DatasetManifestError::InvalidRowCount);
        }
        match (&self.time_range, self.source_timestamp_missing_rows) {
            (Some(range), missing) if missing < self.row_count => {
                if range.start_inclusive >= range.end_exclusive {
                    return Err(DatasetManifestError::InvalidTimeRange);
                }
            }
            (None, missing) if missing == self.row_count => {}
            _ => return Err(DatasetManifestError::InvalidTimeRange),
        }

        if !valid_object_name(&self.object.object_name)
            || self.object.size_bytes == 0
            || self.object.parquet_footer_rows != self.row_count
        {
            return Err(DatasetManifestError::InvalidObject);
        }
        if !valid_sha256(&self.object.content_sha256)
            || !valid_sha256(&self.object.parquet_schema_sha256)
            || !valid_sha256(&self.completion.readback_sha256)
        {
            return Err(DatasetManifestError::InvalidHash);
        }

        let object_id = self
            .object
            .object_id
            .as_deref()
            .filter(|value| valid_identifier(value, 512));
        let object_identity_matches_transport = match (self.object.transport, object_id) {
            (DatasetTransportV1::RcloneGoogleDrive, Some(value)) => {
                !value.starts_with("local-test:")
            }
            (DatasetTransportV1::LocalTest, Some(value)) => {
                value.strip_prefix("local-test:").is_some_and(|tail| {
                    !tail.is_empty()
                        && !tail.contains("..")
                        && tail.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'_' | b'.' | b':' | b'-')
                        })
                })
            }
            (_, None) => false,
        };
        if !object_identity_matches_transport {
            return Err(DatasetManifestError::InvalidObject);
        }

        if !self.completion.input_eof
            || self.completion.source_pages_exhausted == Some(false)
            || !self.completion.verified_before_publish
            || self.completion.readback_sha256 != self.object.content_sha256
        {
            return Err(DatasetManifestError::InvalidCompletion);
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_object_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn stable_identity(value: &str, max_bytes: usize) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
        && !value.contains("..")
        && !value.split(['-', '_', '.', ':']).any(|part| {
            part.eq_ignore_ascii_case("latest") || part.eq_ignore_ascii_case("fallback")
        })
}

#[cfg(test)]
mod tests {
    use super::{
        DatasetCompletionEvidenceV1, DatasetManifestError, DatasetManifestV1, DatasetObjectV1,
        DatasetTimeRangeV1, DatasetTransportV1,
    };
    use crate::{EntitlementState, MarketDataSourceV1, UtcTimestamp};

    fn manifest() -> DatasetManifestV1 {
        let content_sha256 = "a".repeat(64);
        DatasetManifestV1 {
            schema_version: 1,
            dataset_id: "qqq-opra-2026-10-08-v1".to_owned(),
            source: MarketDataSourceV1::new("alpaca", "opra", EntitlementState::Unknown, None)
                .unwrap(),
            symbols: vec!["QQQ261016C00600000".to_owned()],
            time_range: Some(DatasetTimeRangeV1 {
                start_inclusive: UtcTimestamp::parse("2026-10-08T14:30:00Z").unwrap(),
                end_exclusive: UtcTimestamp::parse("2026-10-08T14:31:00Z").unwrap(),
            }),
            source_timestamp_missing_rows: 0,
            row_count: 10,
            object: DatasetObjectV1 {
                object_name: "qqq-opra-2026-10-08.parquet".to_owned(),
                object_id: Some("local-test:fixture-qqq-opra-2026-10-08".to_owned()),
                size_bytes: 4096,
                content_sha256: content_sha256.clone(),
                parquet_schema_sha256: "b".repeat(64),
                parquet_footer_rows: 10,
                transport: DatasetTransportV1::LocalTest,
            },
            completion: DatasetCompletionEvidenceV1 {
                input_eof: true,
                source_pages_exhausted: Some(true),
                readback_sha256: content_sha256,
                verified_before_publish: true,
            },
        }
    }

    #[test]
    fn valid_manifest_roundtrips_and_binds_one_readback_verified_object() {
        let value = manifest();
        value.validate().unwrap();

        let json = serde_json::to_string(&value).unwrap();
        let decoded: DatasetManifestV1 = serde_json::from_str(&json).unwrap();
        decoded.validate().unwrap();
        let json = serde_json::to_value(decoded).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["object"]["transport"], "local_test");
        assert_eq!(json["object"]["size_bytes"], "4096");
        assert_eq!(json["object"]["parquet_footer_rows"], "10");
        assert_eq!(json["source_timestamp_missing_rows"], "0");
        assert_eq!(json["row_count"], "10");
        assert_eq!(json["completion"]["source_pages_exhausted"], true);
        assert!(json.get("dataset_version").is_none());
        assert!(value.validate().is_ok());
    }

    #[test]
    fn opaque_drive_identity_is_not_treated_as_a_local_path() {
        let mut value = manifest();
        value.object.transport = DatasetTransportV1::RcloneGoogleDrive;
        value.object.object_id = Some("drive:opaque/id+with@punctuation".to_owned());
        value.validate().unwrap();
    }

    #[test]
    fn manifest_json_uint64_projection_preserves_values_above_javascript_safe_range() {
        let mut value = manifest();
        value.object.size_bytes = u64::MAX;

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["object"]["size_bytes"], u64::MAX.to_string());
        let decoded: DatasetManifestV1 = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.object.size_bytes, u64::MAX);
    }

    #[test]
    fn manifest_rejects_unverified_or_inconsistent_publication_evidence() {
        let mut value = manifest();
        value.completion.readback_sha256 = "c".repeat(64);
        assert_eq!(
            value.validate(),
            Err(DatasetManifestError::InvalidCompletion)
        );

        let mut value = manifest();
        value.completion.input_eof = false;
        assert_eq!(
            value.validate(),
            Err(DatasetManifestError::InvalidCompletion)
        );

        let mut value = manifest();
        value.completion.source_pages_exhausted = Some(false);
        assert_eq!(
            value.validate(),
            Err(DatasetManifestError::InvalidCompletion)
        );

        let mut value = manifest();
        value.object.parquet_footer_rows = 9;
        assert_eq!(value.validate(), Err(DatasetManifestError::InvalidObject));

        let mut value = manifest();
        value.object.object_name = "..".to_owned();
        assert_eq!(value.validate(), Err(DatasetManifestError::InvalidObject));

        let mut value = manifest();
        value.object.object_name = "dataset?.parquet".to_owned();
        assert_eq!(value.validate(), Err(DatasetManifestError::InvalidObject));

        let mut value = manifest();
        value.object.object_id = Some("local-test:../escape".to_owned());
        assert_eq!(value.validate(), Err(DatasetManifestError::InvalidObject));
    }

    #[test]
    fn manifest_rejects_mutable_ids_unsorted_symbols_and_missing_source_time() {
        let mut value = manifest();
        value.dataset_id = "latest".to_owned();
        assert_eq!(
            value.validate(),
            Err(DatasetManifestError::InvalidDatasetId)
        );

        let mut value = manifest();
        value.dataset_id = "../dataset-v1".to_owned();
        assert_eq!(
            value.validate(),
            Err(DatasetManifestError::InvalidDatasetId)
        );

        let mut value = manifest();
        value.symbols = vec!["SPY".to_owned(), "QQQ".to_owned()];
        assert_eq!(value.validate(), Err(DatasetManifestError::InvalidSymbols));

        let mut value = manifest();
        value.source_timestamp_missing_rows = value.row_count;
        assert_eq!(
            value.validate(),
            Err(DatasetManifestError::InvalidTimeRange)
        );
    }
}
