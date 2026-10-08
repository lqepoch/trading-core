#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root/typescript"
npm ci
[[ "$(protoc --version)" == "libprotoc 3.21.12" ]] || {
  printf 'protoc 3.21.12 is required; found %s\n' "$(protoc --version)" >&2
  exit 1
}
rm -rf src/gen
mkdir -p src/gen
protoc \
  --proto_path="$repo_root/proto" \
  --proto_path=/usr/include \
  --plugin=protoc-gen-es="$repo_root/typescript/node_modules/.bin/protoc-gen-es" \
  --es_out=target=ts,import_extension=js:src/gen \
  lqepoch/market/v1/market.proto \
  lqepoch/market/v2/trade_bar.proto \
  lqepoch/dataset/v1/manifest.proto \
  lqepoch/dataset/v2/manifest.proto \
  lqepoch/prediction/v1/prediction.proto
python3 - <<'PY'
from pathlib import Path

for path in Path("src/gen").rglob("*.ts"):
    path.write_text(path.read_text(encoding="utf-8").rstrip() + "\n", encoding="utf-8")
PY
