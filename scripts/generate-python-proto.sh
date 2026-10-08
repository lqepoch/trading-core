#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
uv sync --project python --python 3.14 --locked
[[ "$(protoc --version)" == "libprotoc 3.21.12" ]] || {
  printf 'protoc 3.21.12 is required; found %s\n' "$(protoc --version)" >&2
  exit 1
}
protoc \
  --proto_path=proto \
  --proto_path=/usr/include \
  --python_out=python \
  lqepoch/market/v1/market.proto \
  lqepoch/market/v2/trade_bar.proto \
  lqepoch/dataset/v1/manifest.proto \
  lqepoch/dataset/v2/manifest.proto \
  lqepoch/prediction/v1/prediction.proto
find python/lqepoch -type d ! -name __pycache__ -exec touch {}/__init__.py \;
