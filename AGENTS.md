# Trading Core repository rules

This repository owns provider-neutral financial values, deterministic models, and versioned cross-language data contracts.

## Ownership boundaries

- exact-decimal and domain own exact numeric values and canonical instrument, order-intent, routing, and account identifiers.
- pricing owns deterministic analytical models and bounded model execution. Model outputs are observations, not market facts, execution prices, or risk authority.
- market-contracts owns serializable market DTOs, legacy compatibility exports, and the trusted Parquet logical-schema registry. Protobuf, OpenAPI, and Parquet registry resources under proto/ and schemas/ are versioned here; raw-frame DTOs describe storage rows but do not implement transport or persistence.
- Broker SDKs, provider adapters, account-authority coordinators, request schedulers, OMS, persistence, and order dispatch belong to their owning repositories. Do not create dependencies on them here.

## Safety

- Parsing an OCC or provider symbol creates a candidate. A qualified contract also requires trading class, currency, multiplier, deliverable, exercise style, settlement type, and provider evidence.
- Preserve exact decimal values at API boundaries. Do not silently convert invalid or missing values to zero.
- Keep unverified models fail closed. Positive-time American IV/Greeks remain unavailable until the independent accuracy gate is accepted.
- This repository must not add broker network clients, OAuth, account credentials, mutation transports, or order dispatch.

## Validation

- Use the toolchain in rust-toolchain.toml, locked dependencies, and synthetic fixtures.
- Run the narrowest relevant package tests, formatting and strict Clippy for changed crates, and scripts/validate-schemas.sh for schema changes.
- Record exact commands and any unavailable language generators as NOT RUN or NOT IMPLEMENTED.
- No repository-defined GitHub Actions workflow is currently configured. GitHub's default CodeQL analysis may run without repository secrets; the local validation scripts remain this repository's project validation gate. If custom workflows are added later, they may build, test, lint, and scan only public source and fixtures. They must not read private repositories, credentials, private market archives, account data, or invoke any broker or trading operation.

## Provenance and licensing

- Every imported source file must be listed in SOURCE-MANIFEST.json with its upstream repository, immutable commit, source path, target path, and content digest.
- The root MIT and Apache-2.0 licenses apply only to source explicitly authorized for this repository. Preserve third-party notices and licenses. Never copy private source without explicit owner authorization.
- Update NOTICE.md and the source manifest when importing or changing upstream-derived code.
