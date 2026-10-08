# Contributing

Keep changes small, reviewable, and compatible with the ownership boundaries in `AGENTS.md`. Reuse the existing domain and exact-decimal types instead of introducing duplicate financial models. Keep provider transport and account/runtime authority in their owning repositories.

Before opening a pull request, run `scripts/validate-rust.sh`. If changing Protobuf, OpenAPI, manifest rules, or a shared fixture, also run `scripts/validate-schemas.sh`. Report the exact commands and distinguish cross-target compilation from tests executed on the target operating system.

Use synthetic fixtures only. Never commit credentials, OAuth material, private market data, account information, provider responses, or order traces. Record imported source provenance and preserve upstream dependency licenses. Do not claim entitlement, contract qualification, pricing accuracy, broker readiness, or trading readiness from DTO validation or fixture tests.

Generated Python and TypeScript Protobuf files are checked in. Edit `.proto` sources, regenerate through the documented scripts, and keep all Rust/Python/TypeScript golden tests aligned. Changes to a shared schema need an explicit versioning and compatibility review.
