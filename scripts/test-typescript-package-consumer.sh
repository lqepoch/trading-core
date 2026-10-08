#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
revision="${1:-$(git -C "$repo_root" rev-parse HEAD)}"
core_git_url="${CORE_CONTRACTS_GIT_URL:-git+https://github.com/lqepoch/trading-core.git}"
if [[ ! "$revision" =~ ^[0-9a-f]{40}$ ]]; then
  echo "usage: $0 <40-character core commit SHA>" >&2
  exit 2
fi
if [[ "$core_git_url" != git+https://* && "$core_git_url" != git+file://* ]]; then
  echo "CORE_CONTRACTS_GIT_URL must use git+https:// or git+file://" >&2
  exit 2
fi

temporary_consumer="$(mktemp -d)"
trap 'rm -rf "$temporary_consumer"' EXIT
mkdir -p "$temporary_consumer/src"
cp "$repo_root/schemas/fixtures/uint64-json-v1.json" "$temporary_consumer/uint64-json-v1.json"
cat > "$temporary_consumer/package.json" <<EOF
{
  "name": "core-contracts-clean-consumer",
  "private": true,
  "type": "module",
  "scripts": {
    "build": "tsc --project tsconfig.json",
    "test": "npm run build && node dist/consumer.js"
  },
  "dependencies": {
    "@lqepoch/trading-core-contracts": "${core_git_url}#${revision}",
    "@types/node": "24.19.1",
    "typescript": "5.9.3"
  }
}
EOF
cat > "$temporary_consumer/tsconfig.json" <<'EOF'
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "skipLibCheck": true,
    "outDir": "dist",
    "rootDir": "src"
  },
  "include": ["src/**/*.ts"]
}
EOF
cat > "$temporary_consumer/src/consumer.ts" <<'EOF'
import { readFileSync } from "node:fs";
import {
  parsePredictionEnvelopeProtoJson,
  type PredictionEnvelopeV1,
} from "@lqepoch/trading-core-contracts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const uint64Fixture = JSON.parse(
  readFileSync(new URL("../uint64-json-v1.json", import.meta.url), "utf8"),
) as { valid: Array<{ value: string }> };
const max = uint64Fixture.valid.find(({ value }) => value === "18446744073709551615");
assert(max !== undefined, "max uint64 fixture is absent");

const prediction: PredictionEnvelopeV1 = parsePredictionEnvelopeProtoJson({
  forecast: { sequence: max.value },
});
assert(prediction.forecast?.sequence === 18_446_744_073_709_551_615n, "prediction sequence lost precision");

let rejectedNumericUint64 = false;
try {
  parsePredictionEnvelopeProtoJson({ forecast: { sequence: 1 } });
} catch {
  rejectedNumericUint64 = true;
}
assert(rejectedNumericUint64, "prediction parser accepted a lossy numeric uint64");

let rejectedRawFrameEncoding = false;
try {
  parsePredictionEnvelopeProtoJson({
    source: { numericEncoding: "NUMERIC_ENCODING_RAW_JSON_BYTES" },
    forecast: { sequence: "1" },
  });
} catch {
  rejectedRawFrameEncoding = true;
}
assert(rejectedRawFrameEncoding, "prediction parser accepted a raw-frame source encoding");
EOF

npm --prefix "$temporary_consumer" install
node --input-type=commonjs - "$revision" "$temporary_consumer/package-lock.json" <<'NODE'
const fs = require("node:fs");

const expectedRevision = process.argv[2];
const lockPath = process.argv[3];
const lock = JSON.parse(fs.readFileSync(lockPath, "utf8"));
const resolved = lock.packages?.["node_modules/@lqepoch/trading-core-contracts"]?.resolved;
if (typeof resolved !== "string" || !resolved.endsWith(`#${expectedRevision}`)) {
  throw new Error(`consumer lockfile does not pin the requested core revision: ${String(resolved)}`);
}
console.log(`consumer lockfile pins trading-core ${expectedRevision}`);
NODE
npm --prefix "$temporary_consumer" test
