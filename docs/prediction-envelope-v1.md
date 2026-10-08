# PredictionEnvelopeV1 Rust projection

`proto/lqepoch/prediction/v1/prediction.proto` is the wire-schema authority. The Rust
`market-contracts` crate exposes the frozen message as `PredictionEnvelopeV1` and parses bounded
ProtoJSON with `parse_prediction_envelope_v1_protojson(&[u8])` or
`PredictionEnvelopeV1::parse_json(&[u8])`. This adds a typed Rust consumer without copying the
forecast contract into the trading engine or changing the protobuf schema.

The parser rejects documents over 2 MiB, unknown fields, duplicate fields and camel/snake aliases
for the same field, numeric or unknown enum names, non-string `uint64` values, noncanonical
`uint64` strings, malformed timestamps, and timestamp precision beyond nanoseconds. Optional
protobuf fields retain presence; absent optional values remain `None`. It validates exact decimal
strings, lowercase SHA-256 shapes, supported source encodings, research-scope identity, causal
clock order, the OOS-selection prohibition, and matching model, dataset, horizon, code, and source
manifest references.

The Python and TypeScript generated consumers use the same shared fixtures for ProtoJSON syntax,
enum, timestamp, alias, duplicate-key, and `uint64` behavior. Their prediction entry points do not
perform every Rust cross-field semantic check. A caller that needs the Rust structural bindings
must use the Rust projection or implement an explicitly reviewed equivalent; none of these parsers
authenticates an issuer or reads the referenced files.

The Python and TypeScript ProtoJSON helpers preserve protobuf wire compatibility for partial
messages: absent submessages or enum fields remain absent/defaulted, while any enum value that is
present must use a supported named value. The shared `wire-partial-max` fixture is intentionally
only `forecast.sequence`; Rust rejects it because Rust is the complete-envelope validator and
requires the full semantic fields. A separate fixture confirms absent enum defaults in present
messages. Version strings follow the existing
quant-research producer pattern, not full SemVer 2.0.0: for example, `01.2.3` and `1.2.3-..` are
accepted by that source pattern, while the valid SemVer 2.0.0 form `1.2.3-alpha+build.2` is not.
Changing that source syntax requires a separately coordinated contract migration.

This is a structure and identity check, not a research admission decision. It does not resolve the
quant horizon registry or session calendar, verify manifest/receipt bytes, prove source entitlement
or historical completeness, establish model quality or alpha, apply consumer freshness/expiry
policy, or authorize an order. A `PASS` quality enum, a hash-shaped string, and a matching model
identity remain producer claims until a separate trusted consumer verifies the referenced evidence.
