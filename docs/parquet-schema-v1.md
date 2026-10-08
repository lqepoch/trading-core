# Parquet logical schema registry

`market-contracts` owns the registered logical schemas in `schemas/fixtures/parquet-schema-registry.json`; the same registry is packaged in the Python contracts wheel. The canonical descriptor has `schema_version`, `schema_id`, and an ordered `fields` list. Object keys are serialized lexically, field order is preserved, JSON is compact UTF-8 with non-ASCII characters unescaped, and the fingerprint is:

```text
SHA256(UTF8("LQEpoch-Parquet-Schema-v1\n") || canonical_descriptor_json)
```

Rust, Python, and TypeScript tests compute the same canonical bytes and compare the shared golden hashes. A matching descriptor hash identifies logical shape only; it does not prove that a file's actual Arrow schema, rows, completeness, provenance, entitlement, or contents are truthful. Storage readers must compare the actual physical schema to the trusted descriptor and independently hash the bytes they read.

| Logical type | Arrow physical type | Parquet physical/logical type | Row constraint |
| --- | --- | --- | --- |
| `utf8` | `Utf8` | `BYTE_ARRAY (STRING)` | Valid UTF-8 text; source identity and enum constraints apply at the owning DTO. |
| `uint32` | `UInt32` | `INT32 (UINT_32)` | Integer in `[0, 2^32-1]`. |
| `uint64` | `UInt64` | `INT64 (UINT_64)` | Integer in `[0, 2^64-1]`; JSON and ProtoJSON use canonical decimal strings, never JavaScript `number`. |
| `timestamp_ns_utc` | `Timestamp(Nanosecond, UTC)` | `INT64 (TIMESTAMP(isAdjustedToUTC=true, unit=NANOS))` | UTC epoch nanoseconds must fit signed `i64`; reject out-of-range timestamps and never truncate. |
| `date_iso8601` | `Utf8` | `BYTE_ARRAY (STRING)` | Exactly `YYYY-MM-DD`, a valid Gregorian date. |
| `decimal_string` | `Utf8` | `BYTE_ARRAY (STRING)` | Exact bounded base-10 value as provided by `DecimalString`; no binary-float conversion in storage. Field-specific price/size sign rules still apply. |
| `sha256_hex` | `Utf8` | `BYTE_ARRAY (STRING)` | Exactly 64 lowercase hexadecimal characters. This format does not establish hash truth. |
| `bool` | `Boolean` | `BOOLEAN` | Boolean value. |
| `binary` | `Binary` | `BYTE_ARRAY` | Exact bytes; row consumers enforce the 1 MiB raw-frame bound before hashing or decode. |

Unknown schema IDs, fields, and logical types are rejected. The current event and minute-bar descriptor bytes and digests are the single cross-language fixtures; a field order, type-width, or nullability change alters the hash and requires an explicit schema version review.

`lqepoch.market_event.v1` stores normalized event rows. `numeric_encoding` identifies the source value representation. When a binary float is shortest-decimal projected, `raw_frame_sha256` is required per event; the projection is not source-exact and remains unqualified absent independent tick evidence. `decimal_token` REST inputs must preserve the original JSON number token with arbitrary-precision parsing.

`lqepoch.market_raw_frame.v1` stores exact received MessagePack market-data frames. Its SHA-256 is recomputed from `frame_bytes`; each uncompressed frame is capped at 1 MiB and its expected normalized event count at 512. `symbols_json` is compact UTF-8 JSON of that frame's lexically sorted, unique normalized market symbols, capped at 512 entries and 263,681 bytes before parsing. Each positive expected event count requires at least one known symbol and cannot be smaller than the number of distinct symbols. Control rows must have zero expected market events and an empty symbol list. Unknown, malformed, or provider-error frames retain any known expected market-event count and decoded-symbol evidence. A single diagnostic/control frame may have an empty symbol list, but a formal dataset may be published only when the full batch symbol union is non-empty. Dataset manifests mark this object with `raw_messagepack_bytes`; the same marker is rejected in normalized market events and predictions. Raw-frame manifests require `time_range` to be absent and `source_timestamp_missing_rows` to equal `row_count`. `source_numeric_encoding` on a raw row is optional and describes only a uniform projection of the frame's numeric fields. The raw-frame schema has no source timestamp, so its manifest uses no time range and marks every row as missing a source timestamp.

`lqepoch.market_event.v2` is a storage-descriptor revision that appends nullable raw-frame correlation fields to the v1 event columns; it does not change `MarketEventEnvelopeV1`. Its existing `schema_version` column therefore retains the wire event value `1`. The four correlation columns are all present or all absent. When present, the event's raw-frame SHA is required, the raw generation equals the event generation, frame sequence is positive, and the 1-based ordinal is within the declared expected event count (at most 512). The expected count is not the count of events successfully delivered; downstream completeness checks must reject a missing projection rather than silently qualifying the raw frame. Generation scope and durability are owned by the adapter; these schemas do not assert that a generation is unique across process restarts.

The installed Python package includes this same generated registry as a wheel resource. Consumers should call `load_trusted_parquet_schema_registry`, `trusted_parquet_schema_descriptor`, or `trusted_parquet_schema_sha256` rather than copying the descriptors. Install the locked optional `arrow` extra to use `verify_pyarrow_schema`; the helper compares actual ordered field names, physical types, timestamp units/timezones, and nullability to the trusted descriptor. It does not hash data or verify row completeness.

New Arrow writers should attach `lqepoch.schema_descriptor.v1` (the exact canonical descriptor UTF-8 JSON) and `lqepoch.schema_fingerprint_sha256` (the exact lowercase SHA-256) to the Arrow schema metadata and flat Parquet footer. These metadata values are excluded from the logical fingerprint. Older files with neither registry key remain readable after registry and physical-schema verification, even if they contain unrelated Arrow metadata. If either registry key is present, both must be present with exact registry values; unrelated keys may coexist. `market_contracts::trusted_parquet_schema_metadata` provides the writer values, and `validate_optional_parquet_schema_metadata` checks reader metadata without requiring an Arrow dependency in core.

`lqepoch.us_equity_trade_bar_1m.v1` is a separate trade-only minute-bar schema. Quote and option events do not become OHLCV rows. Session and window identity must be explicit. Research consumers must not treat event Parquet as minute features or infer session calendars from timestamps.
