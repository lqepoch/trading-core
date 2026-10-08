# Parquet logical schema v1

`market-contracts` owns the registered logical schemas in `schemas/fixtures/parquet-schema-v1.json`. The canonical descriptor has `schema_version`, `schema_id`, and an ordered `fields` list. Object keys are serialized lexically, field order is preserved, JSON is compact UTF-8 with non-ASCII characters unescaped, and the fingerprint is:

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

Unknown schema IDs, fields, and logical types are rejected. The current event and minute-bar descriptor bytes and digests are the single cross-language fixtures; a field order, type-width, or nullability change alters the hash and requires an explicit schema version review.

`lqepoch.market_event.v1` stores normalized event rows. `numeric_encoding` identifies the source value representation. When a binary float is shortest-decimal projected, `raw_frame_sha256` is required per event; the projection is not source-exact and remains unqualified absent independent tick evidence. `decimal_token` REST inputs must preserve the original JSON number token with arbitrary-precision parsing.

`lqepoch.us_equity_trade_bar_1m.v1` is a separate trade-only minute-bar schema. Quote and option events do not become OHLCV rows. Session and window identity must be explicit. Research consumers must not treat event Parquet as minute features or infer session calendars from timestamps.
