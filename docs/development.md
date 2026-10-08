# Development and validation

The workspace pins Rust 1.98.1 in `rust-toolchain.toml` and all Rust dependencies in `Cargo.lock`. Use at most two Cargo build jobs on the project host.

Run `scripts/validate-rust.sh` for formatting, all workspace tests, and warning-free Clippy. The command uses `--locked` for tests and linting. During development, use a package-scoped command such as `CARGO_BUILD_JOBS=2 cargo +1.98.1 test -p market-contracts --locked` before the full gate.

Protobuf and OpenAPI changes use `scripts/validate-schemas.sh`. It requires `protoc` 3.21.12 and generates bindings from the checked-in `.proto` files. The TypeScript plugin and runtime are pinned in `typescript/package-lock.json`; Python package dependencies are pinned in `python/uv.lock` and `python/pyproject.toml` requires uv 0.12.3. The script compiles all four text fixtures, generates Python and TypeScript modules, builds the Python package, runs ProtoJSON `uint64` round trips, checks the shared Parquet fingerprints and manifest identity boundaries in all language consumers, and validates HTTP JSON against OpenAPI 3.1 with a strict uint64 format validator.

The Python ProtoJSON helpers preserve Protobuf `Timestamp.nanos`. If a downstream consumer maps a timestamp into Python `datetime`, it must require `nanos % 1000 == 0`; otherwise it would discard sub-microsecond evidence. These helpers validate the lossless `uint64` projection and parse wire messages, not every Rust market or dataset semantic invariant.

To regenerate one language independently, run `npm --prefix typescript run generate` or `scripts/generate-python-proto.sh`. Python uses the ignored local environment at `python/.venv`. Generated modules are checked in; do not hand-edit them. Change the Protobuf source, regenerate both language bindings, and keep the Rust, Python, and TypeScript fixture checks passing together.

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
