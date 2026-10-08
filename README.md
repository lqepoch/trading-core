# Trading Core

trading-core is the shared source of truth for exact financial values, canonical provider-neutral domain types, analytical option models, and versioned market-data contracts. Broker connectors and the trading engine consume these crates at pinned Git revisions.

## Crates

- exact-decimal: bounded base-10 values with checked, non-rounding arithmetic.
- domain: option candidates and qualified instrument keys, exact prices, quantities, execution identities, and immutable order intents.
- pricing: bounded European Black-Scholes models, evidence checks, Greeks aggregation, singleflight, and a bounded worker pool.
- market-contracts: compatibility DTOs plus versioned event, trade-bar, control, and dataset-manifest contracts. DatasetManifestV2 separates stored-object readback from finite-input, provider-watermark, and diagnostic completion evidence. BarV2 is additive, keeps BarV1 unchanged, and binds each row to the manifest completion oneof with a structural hash reference. Protobuf owns the cross-language market, dataset, and read-only prediction wire schemas; OpenAPI documents their HTTP JSON projections. Generated Python and TypeScript bindings and strict `uint64` JSON projection helpers are checked in and tested against shared fixtures. V2 validates finite receipt hash-to-projection binding, but parsing never proves receipt issuer authenticity or provider completeness; consumers still validate full domain semantics before treating decoded messages as trusted evidence.
- DatasetManifestV2 JSON accepts one spelling per field (`camelCase` or protobuf `snake_case`), named enum strings only, and RFC3339 timestamps with at most nine fractional digits; duplicate aliases, numeric enum values, leap-second labels, and timestamp precision loss are rejected consistently by Rust, Python, and TypeScript.

Python research consumers can pin the reusable validators and generated bindings from the `python/` subdirectory to a reviewed Git revision with uv:

```toml
[tool.uv.sources]
lqepoch-trading-core-contracts = { git = "https://github.com/lqepoch/trading-core", rev = "<reviewed-sha>", subdirectory = "python" }
```

The package exports strict `uint64` JSON helpers, manifest identity checks, canonical Parquet fingerprinting, the trusted Parquet schema registry, and validated ProtoJSON entry points. Python consumers can load the installed registry with `load_trusted_parquet_schema_registry()` and resolve a descriptor or fingerprint by schema ID; there is no second hand-maintained descriptor table. The optional `arrow` extra pins PyArrow for checking actual physical schemas against that registry. Rust data producers can use `market_contracts::wire_u64` with Serde and `NumericEncodingV1::as_str()` for the same wire spellings. BarV2 consumers can call `validate_us_equity_trade_bar_v2_against_manifest`; the helper checks row-to-manifest consistency but does not grant source or receipt authority. Raw V2/V3 validators reject unpaired surrogate code points before UTF-8 byte accounting; the legacy timestamp parser preserves V1 wire behavior and stores only an in-memory precision-quality bit, not the original timestamp string.

## TypeScript server consumer

The repository root is also a private npm Git package, `@lqepoch/trading-core-contracts`. It is not published to an npm registry. A Node.js server can install an exact reviewed commit:

```bash
npm install --save-exact @lqepoch/trading-core-contracts@git+https://github.com/lqepoch/trading-core.git#<40-character-sha>
```

The Git install runs the pinned `typescript/package-lock.json` build and packages the generated `typescript/dist` output, package metadata, README, and license notices; tests and TypeScript source are excluded. The root package exports `parsePredictionEnvelopeProtoJsonText`, `parsePredictionEnvelopeProtoJson`, `PredictionEnvelopeV1`, and its generated schema. Use the bounded raw-text parser for untrusted request bodies; it rejects duplicate JSON keys and camel/snake aliases before ProtoJSON conversion, along with numeric `uint64` values and unknown protobuf fields. The object parser is for already parsed objects and cannot recover duplicate keys discarded by `JSON.parse`. Neither parser performs complete semantic, manifest-readback, evidence-issuer, or research qualification checks. Keep this Node-only package in the server/BFF path because its current parser uses `node:crypto`.

After committing the package revision, verify a clean Git consumer with:

```bash
scripts/test-typescript-package-consumer.sh <40-character-sha>
```

The consumer test installs the exact Git SHA, compiles against exported TypeScript declarations, parses a shared uint64 fixture, and checks lossy-input rejection. `typescript/package.json` remains a private build/test package; the installable private artifact is defined at the repository root for npm's Git dependency lifecycle.

## Safety and evidence limits

OCC parsing is candidate creation only. It does not qualify an economic contract or prove provider entitlement. Pricing requires explicit quote, dividend, time, and model evidence. Positive-time production American pricing remains unavailable with `AmericanPricingAccuracyUnverified`. The retained CRR candidate is available only through the public offline diagnostic API, whose result is explicitly accuracy-unverified, diagnostic-only, and not tradable; it does not produce a production solver outcome or implied volatility. European model metrics are estimates, not executable quotes or risk authority.

The imported legacy DTOs preserve the prior EqoBoard JSON API and its floating-point fields for compatibility. New market event envelopes use exact decimal strings and separate source time from local receive time, plus provider/feed, numeric encoding, generation, and sequence. Binary-float inputs retain a raw-frame digest and remain explicitly projected rather than source-exact. Legacy OCC DTOs do not qualify a contract.

No provider client, OAuth, account-authority coordinator, request-budget scheduler, OMS, persistence implementation, or order transport is included. This repository contains no production broker connectivity. The byte-exact MessagePack/JSON raw-frame and event-correlation schemas describe storage rows; Rust `market-contracts` provides V1/V2 raw and V2/V3 event validators. The Python wheel and TypeScript expose bounded V2/V3 row-shape and raw/event pairing checks; Rust remains the authority for full market-event semantic and OCC validation. These contracts do not implement a collector, Parquet writer, readback, or publication. A subscription ACK is separate from connection state; neither the DTO nor its validator proves SIP/OPRA entitlement or an active subscription.

## Source and license

Selected source files from lqepoch/schwab_auto_bot and lqepoch/EqoBoard are recorded in SOURCE-MANIFEST.json. This new repository is licensed under MIT OR Apache-2.0 only for the explicitly listed, owner-authorized source. Third-party dependencies retain their upstream licenses; see NOTICE.md.

## Local validation

Use Rust 1.98.1 with locked dependencies. Run `scripts/validate-rust.sh` for formatting, workspace tests, strict Clippy, and cargo-deny advisory/license/source/bans checks. Run `scripts/validate-schemas.sh` when Protobuf, OpenAPI, or Parquet registry changes; it checks the generated registry copies, compiles protoc text fixtures, regenerates the pinned Python/TypeScript bindings, verifies Arrow physical mappings with the optional PyArrow extra, runs both generated-consumer suites, clean-builds and installs the Python wheel, and validates OpenAPI JSON including uint64 bounds. Schema generation pins protoc 3.21.12, protobuf Python 7.36.2, and exact npm dependencies in `typescript/package-lock.json`.

See [docs/development.md](docs/development.md) for local and cross-target checks, [docs/parquet-schema-v1.md](docs/parquet-schema-v1.md) for physical mappings and row constraints, [docs/dataset-completion-v2.md](docs/dataset-completion-v2.md) for V2 completion semantics, and [SECURITY.md](SECURITY.md) for the repository security boundary.
