# Development and validation

The workspace pins Rust 1.98.1 in `rust-toolchain.toml` and all Rust dependencies in `Cargo.lock`. Use at most two Cargo build jobs on the project host.

Run `scripts/validate-rust.sh` for formatting, all workspace tests, warning-free Clippy, and `cargo deny` checks. Tests and linting use `--locked`; `deny.toml` allows only SPDX licenses required by the current lockfile, denies unknown registries and git sources, rejects wildcard version constraints, and has no advisory ignores. Workspace path dependencies state their exact workspace version. Multiple crate versions warn for review rather than being blanket-denied. During development, use a package-scoped command such as `CARGO_BUILD_JOBS=2 cargo +1.98.1 test -p market-contracts --locked` before the full gate.

Protobuf, OpenAPI, or Parquet registry changes use `scripts/validate-schemas.sh`. It requires `protoc` 3.21.12 and generates bindings from the checked-in `.proto` files. The TypeScript plugin and runtime are pinned in `typescript/package-lock.json`; Python package dependencies are pinned in `python/uv.lock` and `python/pyproject.toml` requires uv 0.12.3. The script verifies generated registry copies, compiles all five text fixtures, generates Python and TypeScript modules, builds and clean-installs the Python wheel, checks ProtoJSON `uint64` round trips, checks shared Parquet fingerprints and manifest identity boundaries in all language consumers, validates physical Arrow schemas using the pinned optional `arrow` extra, and validates HTTP JSON against OpenAPI 3.1 with a strict uint64 format validator.

The shared ProtoJSON helpers resolve either canonical camelCase JSON names or protobuf snake_case field names before validating `uint64` values and enum-based cross-field rules. Supplying both spellings of a checked field is rejected. The Python helpers preserve Protobuf `Timestamp.nanos`. If a downstream consumer maps a timestamp into Python `datetime`, it must require `nanos % 1000 == 0`; otherwise it would discard sub-microsecond evidence. DatasetManifestV2 finite-batch, provider-watermark, and diagnostic evidence is structural only; callers must verify exact receipt bytes in a fixed trusted composition root before any research qualification. See [dataset completion V2](dataset-completion-v2.md). The helpers validate the lossless `uint64` projection and selected wire invariants, not every Rust market or dataset semantic invariant.

For a validated `DatasetManifestV2`, use the shared full-byte writer in the producer language:
`market_contracts::dataset_manifest_v2_protojson_bytes(&manifest)` in Rust,
`dataset_manifest_v2_protojson_bytes(message)` in Python, or
`datasetManifestV2ProtojsonBytes(message)` in TypeScript. These return the same compact ProtoJSON
bytes, ordered by protobuf field number, with canonical `uint64` decimal strings and nanosecond
timestamps. Required zero-valued scalars are emitted, unset optional fields are omitted, and output
has a 2 MiB maximum with no trailing newline. The shared cases in
`schemas/fixtures/dataset-manifest-v2-protojson-cases.json` verify byte-for-byte output, manifest
SHA-256, and parse/serialize round trips. Hash the returned bytes when a stable manifest identity
is needed; that hash does not establish receipt issuer trust or data completeness.

To regenerate one language independently, run `npm --prefix typescript run generate` or `scripts/generate-python-proto.sh`. Python uses the ignored local environment at `python/.venv`. Generated modules are checked in; do not hand-edit them. Change the Protobuf source, regenerate both language bindings, and keep the Rust, Python, and TypeScript fixture checks passing together.

TypeScript consumers install the repository's private root npm Git package using a reviewed 40-character commit SHA. The root `prepare` lifecycle uses the nested `typescript/package-lock.json` to build JavaScript and declarations from the checked-in generated bindings; npm packs the whitelisted `typescript/dist` output, package metadata, README, and licenses, while excluding source and tests. The package is not published to an npm registry. Use `scripts/test-typescript-package-consumer.sh <sha>` to verify a clean external install, type import, max-`uint64` parsing, and rejection behavior. For untrusted raw HTTP bodies, call the exported `parsePredictionEnvelopeProtoJsonText(text)` entry point: it bounds input to 2 MiB, rejects duplicate JSON object keys and camel/snake aliases before ProtoJSON conversion, then rejects numeric `uint64` values and unknown protobuf fields. The object-taking parser remains available when the caller already owns a parsed object, but cannot recover duplicate keys discarded by an earlier `JSON.parse`. Both parsers provide structural contract validation only, not qualification or evidence authority.

## Cross-target Rust checks

The public core has no native provider SDK or FFI dependency. Install the Windows GNU and macOS targets for the pinned toolchain, then run:

```sh
rustup target add --toolchain 1.98.1 x86_64-pc-windows-gnu aarch64-apple-darwin
CARGO_BUILD_JOBS=2 cargo +1.98.1 check --workspace --locked --target x86_64-pc-windows-gnu
CARGO_BUILD_JOBS=2 cargo +1.98.1 build --workspace --locked --target x86_64-pc-windows-gnu
CARGO_BUILD_JOBS=2 cargo +1.98.1 check --workspace --locked --target aarch64-apple-darwin
CARGO_BUILD_JOBS=2 cargo +1.98.1 build --workspace --locked --target aarch64-apple-darwin
```

These are compile-only checks on the current host. They do not claim execution tests on Windows or macOS.

## SBOM

Regenerate `sbom.spdx.json` with `python3 scripts/generate-sbom.py`. It reads the locked Cargo graph, Python `uv.lock`, npm `package-lock.json`, and exact Python build-system pins in `python/pyproject.toml`; it records package versions, available upstream license declarations, dependency relationships, and lockfile checksums. Set `SOURCE_DATE_EPOCH` to a fixed epoch when a reproducible creation timestamp is required.
