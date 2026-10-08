# Pricing crate rules

- This crate owns deterministic, provider-neutral analytical pricing and bounded Greek work. It does not own market-data transport, contract qualification, broker account state, order decisions, persistence, or execution.
- Preserve exact `Price`, `Strike`, and `Money` values at boundaries. Floating-point arithmetic is limited to bounded analytical solvers, with finite/range checks and typed unavailable results.
- Keep positive-time American pricing fail-closed as `AmericanPricingAccuracyUnverified`; the CRR implementation remains test-only. Do not describe American pricing accuracy as verified.
- Quote and dividend evidence are caller-supplied. Validation does not authenticate a provider, qualify a contract, or establish production readiness.
- Tests use synthetic values only. Do not include private strategy thresholds, account/order traces, provider credentials, OAuth, or live connectivity.
- Validate with `CARGO_BUILD_JOBS=2 cargo +1.98.1 test -p pricing --locked`; the repository-wide Rust gate is documented in `docs/development.md`.
- New or modified code comments are written in English and followed by a Simplified Chinese translation.
