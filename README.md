# Trading Core

trading-core is the shared source of truth for exact financial values, canonical provider-neutral domain types, analytical option models, and versioned market-data contracts. Broker connectors and the trading engine consume these crates at pinned Git revisions.

## Crates

- exact-decimal: bounded base-10 values with checked, non-rounding arithmetic.
- domain: option candidates and qualified instrument keys, exact prices, quantities, execution identities, and immutable order intents.
- pricing: bounded European Black-Scholes models, evidence checks, Greeks aggregation, singleflight, and a bounded worker pool.
- market-contracts: compatibility DTOs plus versioned event, control, and dataset-manifest contracts. DatasetManifestV2 separates stored-object readback from finite-input, provider-watermark, and diagnostic completion evidence. Protobuf owns the cross-language market, dataset, and read-only prediction wire schemas; OpenAPI documents their HTTP JSON projections. Generated Python and TypeScript bindings and strict `uint64` JSON projection helpers are checked in and tested against shared fixtures. V2 completion parsing is structural and never proves receipt authenticity or provider completeness; consumers still validate full domain semantics before treating decoded messages as trusted evidence.

Python research consumers can pin the reusable validators and generated bindings from the `python/` subdirectory to a reviewed Git revision with uv:

```toml
[tool.uv.sources]
lqepoch-trading-core-contracts = { git = "https://github.com/lqepoch/trading-core", rev = "<reviewed-sha>", subdirectory = "python" }
```

The package exports strict `uint64` JSON helpers, manifest identity checks, canonical Parquet fingerprinting, the trusted Parquet schema registry, and validated ProtoJSON entry points. Python consumers can load the installed registry with `load_trusted_parquet_schema_registry()` and resolve a descriptor or fingerprint by schema ID; there is no second hand-maintained descriptor table. The optional `arrow` extra pins PyArrow for checking actual physical schemas against that registry. Rust data producers can use `market_contracts::wire_u64` with Serde and `NumericEncodingV1::as_str()` for the same wire spellings.

## Safety and evidence limits

OCC parsing is candidate creation only. It does not qualify an economic contract or prove provider entitlement. Pricing requires explicit quote, dividend, time, and model evidence. Positive-time American pricing remains unavailable with AmericanPricingAccuracyUnverified; the retained CRR candidate is test-only. European model metrics are estimates, not executable quotes or risk authority.

The imported legacy DTOs preserve the prior EqoBoard JSON API and its floating-point fields for compatibility. New market event envelopes use exact decimal strings and separate source time from local receive time, plus provider/feed, numeric encoding, generation, and sequence. Binary-float inputs retain a raw-frame digest and remain explicitly projected rather than source-exact. Legacy OCC DTOs do not qualify a contract.

No provider client, OAuth, account-authority coordinator, request-budget scheduler, OMS, persistence implementation, or order transport is included. This repository contains no production broker connectivity. The byte-exact MessagePack/JSON raw-frame and event-correlation schemas describe storage rows; they do not implement a collector, Parquet writer, readback, or publication. A subscription ACK is separate from connection state; neither the DTO nor its validator proves SIP/OPRA entitlement or an active subscription.

## Source and license

Selected source files from lqepoch/schwab_auto_bot and lqepoch/EqoBoard are recorded in SOURCE-MANIFEST.json. This new repository is licensed under MIT OR Apache-2.0 only for the explicitly listed, owner-authorized source. Third-party dependencies retain their upstream licenses; see NOTICE.md.

## Local validation

Use Rust 1.98.1 with locked dependencies. Run `scripts/validate-rust.sh` for formatting, workspace tests, strict Clippy, and cargo-deny advisory/license/source/bans checks. Run `scripts/validate-schemas.sh` when Protobuf, OpenAPI, or Parquet registry changes; it checks the generated registry copies, compiles protoc text fixtures, regenerates the pinned Python/TypeScript bindings, verifies Arrow physical mappings with the optional PyArrow extra, runs both generated-consumer suites, clean-builds and installs the Python wheel, and validates OpenAPI JSON including uint64 bounds. Schema generation pins protoc 3.21.12, protobuf Python 7.36.2, and exact npm dependencies in `typescript/package-lock.json`.

See [docs/development.md](docs/development.md) for local and cross-target checks, [docs/parquet-schema-v1.md](docs/parquet-schema-v1.md) for physical mappings and row constraints, [docs/dataset-completion-v2.md](docs/dataset-completion-v2.md) for V2 completion semantics, and [SECURITY.md](SECURITY.md) for the repository security boundary.
