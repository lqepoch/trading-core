# Python contracts package

This versioned package is published from the `python/` subdirectory of the `lqepoch/trading-core` repository. Consumers should depend on an immutable reviewed Git SHA, for example:

```toml
[project]
dependencies = [
  "lqepoch-trading-core-contracts @ git+https://github.com/lqepoch/trading-core@<reviewed-sha>#subdirectory=python",
]
```

With uv, the equivalent source entry is:

```toml
[tool.uv.sources]
lqepoch-trading-core-contracts = { git = "https://github.com/lqepoch/trading-core", rev = "<reviewed-sha>", subdirectory = "python" }
```

The package contains generated `lqepoch.*` Protobuf messages and reusable `lqepoch_contracts` validation helpers:

- `uint64_json`: strict canonical string parsing and nested JSON field validation for camelCase and protobuf snake_case names before ProtoJSON parsing; duplicate spellings are rejected.
- `protojson`: market event, dataset manifest, and prediction envelope entry points that validate `uint64` projections and selected enum/cross-field invariants after Protobuf resolves field names. They do not replace full domain-semantic validation by a consumer.
- `identities`: shared UTF-8, immutable dataset, symbol, object basename, and transport identity rules.
- `parquet_schema`: the shared canonical logical-type grammar and Parquet schema fingerprint algorithm, with the trusted registry loaded from the installed wheel resource. `verify_pyarrow_schema` checks physical Arrow types and validates optional registry metadata when the optional, locked `arrow` extra is installed. Old files with neither registry key remain eligible after physical-schema validation, including files with unrelated metadata; if either registry key is present, both must match.

Runtime dependency is pinned to protobuf 7.36.2. This package does not provide market-data transport, credentials, account authority, order mutation, or trading execution.

Protobuf `Timestamp` values retain nanoseconds in their `(seconds, nanos)` representation. A consumer converting them to Python `datetime` must reject values whose nanoseconds are not an exact multiple of 1,000 rather than silently dropping sub-microsecond precision.
