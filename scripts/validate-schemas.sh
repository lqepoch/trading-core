#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
[[ "$(protoc --version)" == "libprotoc 3.21.12" ]] || {
  printf 'protoc 3.21.12 is required; found %s\n' "$(protoc --version)" >&2
  exit 1
}

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
protoc \
  --proto_path=proto \
  --proto_path=/usr/include \
  --descriptor_set_out="$scratch/contracts.pb" \
  --include_imports \
  lqepoch/market/v1/market.proto \
  lqepoch/dataset/v1/manifest.proto \
  lqepoch/prediction/v1/prediction.proto

protoc --proto_path=proto --proto_path=/usr/include \
  --encode=lqepoch.market.v1.MarketEventEnvelopeV1 \
  lqepoch/market/v1/market.proto \
  < proto/fixtures/market-event-v1.textproto > "$scratch/market-event.pb"
protoc --proto_path=proto --proto_path=/usr/include \
  --encode=lqepoch.market.v1.ControlEventEnvelopeV1 \
  lqepoch/market/v1/market.proto \
  < proto/fixtures/control-ack-v1.textproto > "$scratch/control-ack.pb"
protoc --proto_path=proto --proto_path=/usr/include \
  --encode=lqepoch.dataset.v1.DatasetManifestV1 \
  lqepoch/dataset/v1/manifest.proto \
  < proto/fixtures/dataset-manifest-v1.textproto > "$scratch/dataset-manifest.pb"
protoc --proto_path=proto --proto_path=/usr/include \
  --encode=lqepoch.prediction.v1.PredictionEnvelopeV1 \
  lqepoch/prediction/v1/prediction.proto \
  < proto/fixtures/prediction-envelope-v1.textproto > "$scratch/prediction-envelope.pb"

"$repo_root/scripts/generate-typescript-proto.sh"
"$repo_root/scripts/generate-python-proto.sh"
npm --prefix "$repo_root/typescript" test
uv build --project "$repo_root/python" --out-dir "$scratch/python-dist"
uv venv --python 3.14 "$scratch/python-venv"
uv pip install --python "$scratch/python-venv/bin/python" "protobuf==7.36.2"
wheel="$(find "$scratch/python-dist" -maxdepth 1 -name '*.whl' -print -quit)"
[[ -n "$wheel" ]] || { printf 'Python wheel was not built\n' >&2; exit 1; }
uv pip install --python "$scratch/python-venv/bin/python" "$wheel"
(cd "$scratch" && "$scratch/python-venv/bin/python" -c 'from lqepoch.market.v1 import market_pb2; from lqepoch_contracts.uint64_json import parse_uint64_json; assert market_pb2.MarketEventEnvelopeV1().generation == 0; assert parse_uint64_json("18446744073709551615") == (1 << 64) - 1')
uv run --project "$repo_root/python" --locked \
  python -m unittest discover -s "$repo_root/python/tests" -v
