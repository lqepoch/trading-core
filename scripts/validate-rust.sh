#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo +1.98.1 fmt --all -- --check
cargo +1.98.1 test --workspace --locked
cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings
cargo deny check
