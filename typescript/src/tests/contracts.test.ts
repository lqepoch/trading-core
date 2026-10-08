import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { Ajv2020 } from "ajv/dist/2020.js";
import { toJsonString } from "@bufbuild/protobuf";
import type { JsonValue } from "@bufbuild/protobuf";
import {
  parseCanonicalUint64Json,
  parseDatasetManifestProtoJson,
  parseMarketEventProtoJson,
  parsePredictionEnvelopeProtoJson,
  validateUint64JsonPaths,
} from "../contracts/uint64-json.js";
import {
  canonicalSchemaJson,
  fingerprintSchemaSha256,
  type SchemaDescriptor,
} from "../contracts/schema-fingerprint.js";
import { MarketEventEnvelopeV1Schema } from "../gen/lqepoch/market/v1/market_pb.js";

function readFixture<T>(path: string): T {
  return JSON.parse(readFileSync(resolve(process.cwd(), "..", path), "utf8")) as T;
}

const require = createRequire(import.meta.url);
const addFormats = require("ajv-formats").default as (ajv: Ajv2020) => Ajv2020;

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function rejects(callback: () => unknown, message: string): void {
  let rejected = false;
  try {
    callback();
  } catch {
    rejected = true;
  }
  assert(rejected, message);
}

type NamedValue = { name: string; value: unknown };
type UInt64Fixture = { valid: NamedValue[]; invalid: NamedValue[] };
const uint64Fixture = readFixture<UInt64Fixture>("schemas/fixtures/uint64-json-v1.json");

for (const testCase of uint64Fixture.valid) {
  const actual = parseCanonicalUint64Json(testCase.value);
  assert(actual.toString() === testCase.value, `uint64 fixture failed: ${testCase.name}`);
}
for (const testCase of uint64Fixture.invalid) {
  rejects(() => parseCanonicalUint64Json(testCase.value), `accepted ${testCase.name}`);
}

const marketJson = {
  schemaVersion: 1,
  source: {
    provider: "synthetic",
    feed: "synthetic",
    entitlement: "unknown",
    numericEncoding: "NUMERIC_ENCODING_DECIMAL_TOKEN",
  },
  generation: "9007199254740993",
  sequence: "18446744073709551615",
  receivedTimestamp: "2026-10-08T14:30:00Z",
  event: { optionQuote: { symbol: "QQQ261016C00600000", bid: "1.25" } },
};
const marketMessage = parseMarketEventProtoJson(marketJson);
assert(marketMessage.generation === 9_007_199_254_740_993n, "generation lost precision");
assert(marketMessage.sequence === 18_446_744_073_709_551_615n, "sequence lost precision");
const marketRoundtrip = JSON.parse(
  toJsonString(MarketEventEnvelopeV1Schema, marketMessage),
) as Record<string, unknown>;
assert(marketRoundtrip.generation === marketJson.generation, "ProtoJSON generation changed");
assert(marketRoundtrip.sequence === marketJson.sequence, "ProtoJSON sequence changed");
for (const testCase of uint64Fixture.invalid) {
  rejects(
    () => parseMarketEventProtoJson({ ...marketJson, generation: testCase.value }),
    `market envelope accepted ${testCase.name}`,
  );
}

const max = "18446744073709551615";
const datasetJson = {
  schemaVersion: 1,
  datasetId: "dataset:version-1",
  source: {
    provider: "synthetic",
    feed: "synthetic",
    entitlement: "unknown",
    numericEncoding: "NUMERIC_ENCODING_DECIMAL_TOKEN",
  },
  symbols: ["QQQ"],
  sourceTimestampMissingRows: max,
  rowCount: max,
  object: {
    objectName: "fixture.parquet",
    objectId: "local-test:fixture-1",
    sizeBytes: max,
    contentSha256: "a".repeat(64),
    parquetSchemaSha256: "b".repeat(64),
    parquetFooterRows: max,
    transport: "local_test",
  },
  completion: {
    inputEof: true,
    sourcePagesExhausted: true,
    readbackSha256: "a".repeat(64),
    verifiedBeforePublish: true,
  },
};
const datasetMessage = parseDatasetManifestProtoJson(datasetJson);
assert(datasetMessage.rowCount === BigInt(max), "dataset row count lost precision");
assert(datasetMessage.object?.sizeBytes === BigInt(max), "object size lost precision");

const predictionJson = { forecast: { sequence: max } };
const predictionMessage = parsePredictionEnvelopeProtoJson(predictionJson);
assert(predictionMessage.forecast?.sequence === BigInt(max), "prediction sequence lost precision");
for (const testCase of uint64Fixture.invalid) {
  rejects(
    () => parsePredictionEnvelopeProtoJson({ forecast: { sequence: testCase.value } }),
    `prediction envelope accepted ${testCase.name}`,
  );
}

type FingerprintFixture = {
  fingerprint_prefix: string;
  generic_golden: { descriptor: SchemaDescriptor; canonical_json: string; sha256: string };
  schemas: Array<{ descriptor: SchemaDescriptor; canonical_json: string; sha256: string }>;
  invalid_descriptors: Array<{ name: string; descriptor: unknown }>;
};
const parquetFixture = readFixture<FingerprintFixture>("schemas/fixtures/parquet-schema-v1.json");
assert(parquetFixture.fingerprint_prefix === "LQEpoch-Parquet-Schema-v1\n", "bad prefix");
for (const golden of [parquetFixture.generic_golden, ...parquetFixture.schemas]) {
  assert(canonicalSchemaJson(golden.descriptor) === golden.canonical_json, "canonical JSON differs");
  assert(fingerprintSchemaSha256(golden.descriptor) === golden.sha256, "SHA-256 differs");
}
for (const invalid of parquetFixture.invalid_descriptors) {
  rejects(() => canonicalSchemaJson(invalid.descriptor), `accepted ${invalid.name}`);
}

const openapi = JSON.parse(
  readFileSync(resolve(process.cwd(), "..", "schemas/openapi.json"), "utf8"),
) as object;
const ajv = new Ajv2020({ strict: false, allErrors: true });
addFormats(ajv);
ajv.addFormat("uint64", {
  type: "string",
  validate: (value: string) => {
    try {
      parseCanonicalUint64Json(value);
      return true;
    } catch {
      return false;
    }
  },
});
ajv.addSchema(openapi, "lqepoch-openapi");
const validateMarketHttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/MarketEventEnvelopeV1",
});
const marketHttpJson = {
  schema_version: 1,
  source: { provider: "synthetic", feed: "synthetic", entitlement: "unknown", numeric_encoding: "decimal_token" },
  generation: "9007199254740993",
  sequence: max,
  received_timestamp: "2026-10-08T14:30:00Z",
  event: { kind: "stock_trade", symbol: "QQQ", price: "1.25", size: "1" },
};
assert(validateMarketHttpJson(marketHttpJson), "OpenAPI rejects uint64 max-safe HTTP JSON");
for (const bad of [1, "01", "18446744073709551616", "1\n", "999999999999999999999"]) {
  const valid = validateMarketHttpJson({ ...marketHttpJson, generation: bad });
  assert(!valid, `OpenAPI accepted invalid HTTP uint64: ${String(bad)}`);
}
validateUint64JsonPaths({ row_count: max, object: { size_bytes: max } }, [
  ["row_count"],
  ["object", "size_bytes"],
]);
rejects(
  () => validateUint64JsonPaths({ row_count: 1 }, [["row_count"]]),
  "generic uint64 path validator accepted a number",
);

// Keep the imported ProtoJSON type in this compile target as a compile-time contract.
const _jsonTypecheck: JsonValue = marketJson;
void _jsonTypecheck;
