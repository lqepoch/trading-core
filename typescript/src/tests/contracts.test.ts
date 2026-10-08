import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { Ajv2020 } from "ajv/dist/2020.js";
import { toJsonString } from "@bufbuild/protobuf";
import type { JsonValue } from "@bufbuild/protobuf";
import {
  parseCanonicalUint64Json,
  parseDatasetManifestProtoJson,
  parseDatasetManifestV2Json,
  parseDatasetManifestV2ProtoJson,
  parseMarketEventProtoJson,
  parsePredictionEnvelopeProtoJson,
  validateUint64JsonPaths,
} from "../contracts/uint64-json.js";
import {
  canonicalSchemaJson,
  fingerprintSchemaSha256,
  PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY,
  PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY,
  trustedParquetSchemaSha256,
  trustedParquetSchemaMetadata,
  validateOptionalParquetSchemaMetadata,
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
validateUint64JsonPaths({ row_count: "1" }, [["rowCount"]]);
rejects(
  () => validateUint64JsonPaths({ row_count: 1 }, [["rowCount"]]),
  "snake-case uint64 accepted a JSON number",
);
rejects(
  () => validateUint64JsonPaths({ rowCount: "1", row_count: "1" }, [["rowCount"]]),
  "uint64 accepted both ProtoJSON field spellings",
);

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

const datasetV2Json = readFixture<Record<string, unknown>>(
  "schemas/fixtures/dataset-manifest-v2.json",
);
const datasetV2Message = parseDatasetManifestV2ProtoJson(datasetV2Json);
assert(datasetV2Message.schemaVersion === 2, "dataset v2 schema version changed");
assert(datasetV2Message.rowCount === 1n, "dataset v2 row count changed");
assert(datasetV2Message.completionEvidence?.evidence.case === "finiteBatch", "dataset v2 oneof lost");
assert(
  datasetV2Message.completionEvidence.evidence.value.completedAt?.nanos === 123_456_789,
  "dataset v2 nanosecond timestamp was truncated",
);
const datasetV2DiagnosticJson = readFixture<Record<string, unknown>>(
  "schemas/fixtures/dataset-manifest-v2-diagnostic-stream.json",
);
const datasetV2Diagnostic = parseDatasetManifestV2ProtoJson(datasetV2DiagnosticJson);
const datasetV2DiagnosticEvidence = datasetV2Diagnostic.completionEvidence?.evidence;
if (
  datasetV2DiagnosticEvidence?.case !== "diagnosticStream" ||
  datasetV2DiagnosticEvidence.value.observedMaxSourceTimestamp === undefined ||
  datasetV2DiagnosticEvidence.value.localPolicyCutoff === undefined
) {
  throw new Error("dataset v2 diagnostic oneof/timestamps were not preserved");
}
assert(
  datasetV2DiagnosticEvidence.value.observedMaxSourceTimestamp.seconds ===
    datasetV2DiagnosticEvidence.value.localPolicyCutoff.seconds + 5n,
  "dataset v2 diagnostic cutoff incorrectly discarded a later observation",
);
const datasetV2JsonText = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/dataset-manifest-v2.json"),
  "utf8",
);
assert(parseDatasetManifestV2Json(datasetV2JsonText).rowCount === 1n, "raw JSON V2 parser changed rows");
rejects(
  () => parseDatasetManifestV2Json(datasetV2JsonText.replace('"rowCount": "1"', '"rowCount":"1","rowCount":"1"')),
  "dataset v2 accepted duplicate raw JSON keys",
);

const datasetV2PagedInvalid = structuredClone(datasetV2Json) as Record<string, unknown>;
const datasetV2Completion = datasetV2PagedInvalid.completionEvidence as Record<string, unknown>;
const datasetV2Finite = datasetV2Completion.finiteBatch as Record<string, unknown>;
datasetV2Finite.pagesExhausted = false;
rejects(
  () => parseDatasetManifestV2ProtoJson(datasetV2PagedInvalid),
  "dataset v2 accepted an unexhausted historical page set",
);
const datasetV2CutoffInvalid = structuredClone(datasetV2Json) as Record<string, unknown>;
const cutoffInvalidEvidence = datasetV2CutoffInvalid.completionEvidence as Record<string, unknown>;
const cutoffInvalidFinite = cutoffInvalidEvidence.finiteBatch as Record<string, unknown>;
cutoffInvalidFinite.dataCutoffExclusive = "2026-10-08T14:30:59.999999999Z";
rejects(
  () => parseDatasetManifestV2ProtoJson(datasetV2CutoffInvalid),
  "dataset v2 accepted source-time coverage after the finite-batch cutoff",
);

const datasetV2OneofInvalid = structuredClone(datasetV2Json) as Record<string, unknown>;
const datasetV2MultiEvidence = datasetV2OneofInvalid.completionEvidence as Record<string, unknown>;
datasetV2MultiEvidence.diagnosticStream = {
  sourceInstanceId: "local-session-1",
  generation: "1",
  observedLastSequence: "1",
  localPolicyCutoff: "2026-10-08T14:31:00Z",
  diagnosticPolicySha256: "d".repeat(64),
  diagnosticReceiptSha256: "e".repeat(64),
};
rejects(
  () => parseDatasetManifestV2ProtoJson(datasetV2OneofInvalid),
  "dataset v2 accepted multiple oneof cases",
);

const datasetV2AliasInvalid = structuredClone(datasetV2Json) as Record<string, unknown>;
datasetV2AliasInvalid.row_count = "1";
rejects(
  () => parseDatasetManifestV2ProtoJson(datasetV2AliasInvalid),
  "dataset v2 accepted camel and snake aliases together",
);

const datasetV2NumericInvalid = structuredClone(datasetV2Json) as Record<string, unknown>;
datasetV2NumericInvalid.rowCount = 1;
rejects(
  () => parseDatasetManifestV2ProtoJson(datasetV2NumericInvalid),
  "dataset v2 accepted a lossy uint64 JSON number",
);
const datasetV2UnknownField = structuredClone(datasetV2Json) as Record<string, unknown>;
const unknownFieldEvidence = datasetV2UnknownField.completionEvidence as Record<string, unknown>;
const unknownFieldFinite = unknownFieldEvidence.finiteBatch as Record<string, unknown>;
unknownFieldFinite.callerQualified = true;
rejects(
  () => parseDatasetManifestV2ProtoJson(datasetV2UnknownField),
  "dataset v2 accepted an unknown completion-evidence field",
);

const datasetV2Watermark = readFixture<Record<string, unknown>>(
  "schemas/fixtures/dataset-manifest-v2-provider-watermark.json",
);
assert(
  parseDatasetManifestV2ProtoJson(datasetV2Watermark).completionEvidence?.evidence.case ===
    "providerWatermark",
  "dataset v2 provider watermark structural evidence was rejected",
);
const datasetV2WatermarkMessage = parseDatasetManifestV2ProtoJson(datasetV2Watermark);
assert(
  datasetV2WatermarkMessage.completionEvidence?.evidence.case === "providerWatermark" &&
    datasetV2WatermarkMessage.completionEvidence.evidence.value.sequenceCount === 5n,
  "dataset v2 provider watermark sequence count changed",
);
const datasetV2WatermarkBoundary = readFixture<Record<string, unknown>>(
  "schemas/fixtures/dataset-manifest-v2-provider-watermark-u64-boundary.json",
);
const datasetV2WatermarkBoundaryMessage = parseDatasetManifestV2ProtoJson(datasetV2WatermarkBoundary);
const boundaryEvidence = datasetV2WatermarkBoundaryMessage.completionEvidence?.evidence;
if (boundaryEvidence?.case !== "providerWatermark") {
  throw new Error("dataset v2 max-u64 watermark fixture was not parsed");
}
assert(
  boundaryEvidence.value.generation === 18_446_744_073_709_551_615n &&
    boundaryEvidence.value.firstSequence === 1n &&
    boundaryEvidence.value.lastSequence === 18_446_744_073_709_551_615n &&
    boundaryEvidence.value.sequenceCount === 18_446_744_073_709_551_615n &&
    boundaryEvidence.value.allowedLatenessNs === 0n,
  "dataset v2 max-u64 sequence boundary lost precision",
);
for (const [field, invalid] of [
  ["generation", "0"],
  ["firstSequence", "0"],
  ["lastSequence", "0"],
] as const) {
  const candidate = structuredClone(datasetV2WatermarkBoundary) as Record<string, unknown>;
  const evidence = candidate.completionEvidence as Record<string, unknown>;
  const watermark = evidence.providerWatermark as Record<string, unknown>;
  watermark[field] = invalid;
  rejects(
    () => parseDatasetManifestV2ProtoJson(candidate),
    `dataset v2 accepted zero ${field}`,
  );
}
const overflowingWatermark = structuredClone(datasetV2WatermarkBoundary) as Record<string, unknown>;
const overflowingEvidence = overflowingWatermark.completionEvidence as Record<string, unknown>;
const overflowingValue = overflowingEvidence.providerWatermark as Record<string, unknown>;
overflowingValue.firstSequence = "0";
overflowingValue.lastSequence = max;
rejects(
  () => parseDatasetManifestV2ProtoJson(overflowingWatermark),
  "dataset v2 accepted an overflowing first-to-last sequence span",
);
const invalidWatermark = structuredClone(datasetV2Watermark) as Record<string, unknown>;
const invalidWatermarkEvidence = invalidWatermark.completionEvidence as Record<string, unknown>;
const invalidWatermarkValue = invalidWatermarkEvidence.providerWatermark as Record<string, unknown>;
invalidWatermarkValue.allowedLatenessNs = "60000000001";
rejects(
  () => parseDatasetManifestV2ProtoJson(invalidWatermark),
  "dataset v2 silently clamped an over-limit watermark lateness",
);

const rawSchemaSha256 = trustedParquetSchemaSha256("lqepoch.market_raw_frame.v1");
const rawJsonSchemaSha256 = trustedParquetSchemaSha256("lqepoch.market_raw_json_frame.v1");
assert(
  rawSchemaSha256 === "3dfcd21648d7a29e5717150f5470250c5a98db4e65f48f98a34594568fe01df6",
  "published MessagePack raw-frame fingerprint changed",
);
const rawManifest = {
  ...datasetJson,
  source: { ...datasetJson.source, numericEncoding: "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES" },
  sourceTimestampMissingRows: "1",
  rowCount: "1",
  object: { ...datasetJson.object, parquetSchemaSha256: rawSchemaSha256, parquetFooterRows: "1" },
};
const rawManifestMessage = parseDatasetManifestProtoJson(rawManifest);
assert(rawManifestMessage.source?.numericEncoding === 5, "raw manifest encoding changed");
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawManifest,
      object: { ...rawManifest.object, parquetSchemaSha256: "c".repeat(64) },
    }),
  "raw manifest accepted a non-raw Parquet schema fingerprint",
);
rejects(
  () => parseDatasetManifestProtoJson({ ...rawManifest, rowCount: "0", sourceTimestampMissingRows: "0" }),
  "raw manifest accepted an empty dataset",
);
rejects(
  () => {
    const { rowCount: _rowCount, ...missingRowCount } = rawManifest;
    return parseDatasetManifestProtoJson(missingRowCount);
  },
  "raw manifest accepted a missing row count",
);
rejects(
  () => parseDatasetManifestProtoJson({ ...rawManifest, sourceTimestampMissingRows: "0" }),
  "raw manifest accepted inconsistent missing timestamp rows",
);
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawManifest,
      timeRange: { startInclusive: "2026-10-08T00:00:00Z" },
    }),
  "raw manifest accepted a source-time range",
);
rejects(
  () => parseMarketEventProtoJson({ ...marketJson, source: rawManifest.source }),
  "market event accepted raw MessagePack as a numeric encoding",
);
rejects(
  () => parsePredictionEnvelopeProtoJson({ source: rawManifest.source, forecast: { sequence: "1" } }),
  "prediction accepted raw MessagePack as a numeric encoding",
);
const rawJsonManifest = {
  ...rawManifest,
  source: { ...rawManifest.source, numericEncoding: "NUMERIC_ENCODING_RAW_JSON_BYTES" },
  object: { ...rawManifest.object, parquetSchemaSha256: rawJsonSchemaSha256 },
};
const rawJsonManifestMessage = parseDatasetManifestProtoJson(rawJsonManifest);
assert(rawJsonManifestMessage.source?.numericEncoding === 6, "raw JSON encoding changed");
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawJsonManifest,
      object: { ...rawJsonManifest.object, parquetSchemaSha256: rawSchemaSha256 },
    }),
  "raw JSON manifest accepted the MessagePack schema fingerprint",
);
rejects(
  () => parseMarketEventProtoJson({ ...marketJson, source: rawJsonManifest.source }),
  "market event accepted raw JSON as a numeric encoding",
);
rejects(
  () => parsePredictionEnvelopeProtoJson({ source: rawJsonManifest.source, forecast: { sequence: "1" } }),
  "prediction accepted raw JSON as a numeric encoding",
);

for (const [encoding, label] of [
  ["NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES", "MessagePack"],
  ["NUMERIC_ENCODING_RAW_JSON_BYTES", "JSON"],
] as const) {
  const snakeSource = {
    provider: "synthetic",
    feed: "synthetic",
    entitlement: "unknown",
    numeric_encoding: encoding,
  };
  rejects(
    () => parseMarketEventProtoJson({ ...marketJson, source: snakeSource }),
    `market event accepted snake-case raw ${label} encoding`,
  );
  rejects(
    () => parsePredictionEnvelopeProtoJson({ source: snakeSource, forecast: { sequence: "1" } }),
    `prediction accepted snake-case raw ${label} encoding`,
  );
}
rejects(
  () =>
    parseMarketEventProtoJson({
      ...marketJson,
      source: {
        ...marketJson.source,
        numeric_encoding: "NUMERIC_ENCODING_RAW_JSON_BYTES",
      },
    }),
  "market event accepted conflicting numeric-encoding aliases",
);

const {
  parquetSchemaSha256: _discardedParquetSchemaSha256,
  parquetFooterRows: _discardedParquetFooterRows,
  ...rawJsonSnakeObjectWithoutHash
} = rawJsonManifest.object;
const rawJsonSnakeManifest = {
  ...datasetJson,
  source: {
    provider: "synthetic",
    feed: "synthetic",
    entitlement: "unknown",
    numeric_encoding: "NUMERIC_ENCODING_RAW_JSON_BYTES",
  },
  row_count: "1",
  source_timestamp_missing_rows: "1",
  object: {
    ...rawJsonSnakeObjectWithoutHash,
    parquet_schema_sha256: rawJsonSchemaSha256,
    parquet_footer_rows: "1",
  },
};
const { rowCount: _discardedRowCount, ...rawJsonSnakeManifestWithoutCanonicalRowCount } =
  rawJsonSnakeManifest;
const { sourceTimestampMissingRows: _discardedMissingRows, ...rawJsonSnakeManifestCanonical } =
  rawJsonSnakeManifestWithoutCanonicalRowCount;
const rawJsonSnakeManifestMessage = parseDatasetManifestProtoJson(rawJsonSnakeManifestCanonical);
assert(rawJsonSnakeManifestMessage.source?.numericEncoding === 6, "snake-case raw JSON manifest encoding changed");
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawJsonSnakeManifestCanonical,
      object: { ...rawJsonSnakeManifestCanonical.object, parquet_schema_sha256: rawSchemaSha256 },
    }),
  "raw JSON manifest accepted snake-case MessagePack schema pairing",
);
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawJsonSnakeManifestCanonical,
      source: {
        provider: "synthetic",
        feed: "synthetic",
        entitlement: "unknown",
        numeric_encoding: "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES",
      },
    }),
  "raw MessagePack manifest accepted a snake-case JSON schema pairing",
);
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawManifest,
      source: {
        ...rawManifest.source,
        numeric_encoding: "NUMERIC_ENCODING_RAW_JSON_BYTES",
      },
    }),
  "dataset accepted conflicting numeric-encoding aliases",
);
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawJsonSnakeManifestCanonical,
      row_count: 1,
    }),
  "raw JSON manifest accepted snake-case numeric row count",
);
rejects(
  () =>
    parseDatasetManifestProtoJson({
      ...rawJsonSnakeManifestCanonical,
      time_range: {},
    }),
  "raw JSON manifest accepted a snake-case source-time range",
);

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
const parquetFixture = readFixture<FingerprintFixture>("schemas/fixtures/parquet-schema-registry.json");
assert(parquetFixture.fingerprint_prefix === "LQEpoch-Parquet-Schema-v1\n", "bad prefix");
for (const golden of [parquetFixture.generic_golden, ...parquetFixture.schemas]) {
  assert(canonicalSchemaJson(golden.descriptor) === golden.canonical_json, "canonical JSON differs");
  assert(fingerprintSchemaSha256(golden.descriptor) === golden.sha256, "SHA-256 differs");
  if (golden.descriptor.schema_id !== "test.v1") {
    assert(
      trustedParquetSchemaSha256(golden.descriptor.schema_id) === golden.sha256,
      "generated TypeScript registry differs from the root fixture",
    );
  }
}
for (const invalid of parquetFixture.invalid_descriptors) {
  rejects(() => canonicalSchemaJson(invalid.descriptor), `accepted ${invalid.name}`);
}

const rawRegistryMetadata = trustedParquetSchemaMetadata("lqepoch.market_raw_frame.v1");
validateOptionalParquetSchemaMetadata("lqepoch.market_raw_frame.v1", undefined);
validateOptionalParquetSchemaMetadata("lqepoch.market_raw_frame.v1", {});
validateOptionalParquetSchemaMetadata("lqepoch.market_raw_frame.v1", { writer: "legacy" });
validateOptionalParquetSchemaMetadata("lqepoch.market_raw_frame.v1", rawRegistryMetadata);
rejects(
  () =>
    validateOptionalParquetSchemaMetadata("lqepoch.market_raw_frame.v1", {
      [PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY]:
        rawRegistryMetadata[PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY],
    }),
  "schema metadata accepted only one registry key",
);
rejects(
  () =>
    validateOptionalParquetSchemaMetadata("lqepoch.market_raw_frame.v1", {
      ...rawRegistryMetadata,
      [PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY]: "0".repeat(64),
    }),
  "schema metadata accepted a mismatched fingerprint",
);

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
const validateDatasetHttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/DatasetManifestV1",
});
const validateDatasetV2HttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/DatasetManifestV2",
});
function toOpenApiSnakeCase(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(toOpenApiSnakeCase);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => [
        key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`),
        toOpenApiSnakeCase(nested),
      ]),
    );
  }
  return value;
}
const datasetV2HttpJson = toOpenApiSnakeCase(datasetV2Json) as Record<string, unknown>;
assert(validateDatasetV2HttpJson(datasetV2HttpJson), "OpenAPI rejects the finite-batch V2 fixture");
const datasetV2DiagnosticHttpJson = toOpenApiSnakeCase(datasetV2DiagnosticJson) as Record<string, unknown>;
assert(
  validateDatasetV2HttpJson(datasetV2DiagnosticHttpJson),
  "OpenAPI rejects the diagnostic-stream V2 fixture",
);
const datasetV2WatermarkHttpJson = toOpenApiSnakeCase(datasetV2Watermark) as Record<string, unknown>;
assert(
  validateDatasetV2HttpJson(datasetV2WatermarkHttpJson),
  "OpenAPI rejects the provider-watermark V2 fixture",
);
const watermarkBoundaryHttpJson = toOpenApiSnakeCase(datasetV2WatermarkBoundary) as Record<string, unknown>;
assert(
  validateDatasetV2HttpJson(watermarkBoundaryHttpJson),
  "OpenAPI rejects the V2 max-u64 watermark fixture",
);
const rawJsonDatasetV2HttpJson = structuredClone(datasetV2HttpJson);
const rawJsonV2Source = rawJsonDatasetV2HttpJson.source as Record<string, unknown>;
const rawJsonV2Object = rawJsonDatasetV2HttpJson.object as Record<string, unknown>;
rawJsonV2Source.numeric_encoding = "NUMERIC_ENCODING_RAW_JSON_BYTES";
rawJsonDatasetV2HttpJson.source_timestamp_missing_rows = "1";
delete rawJsonDatasetV2HttpJson.time_range;
rawJsonV2Object.parquet_schema_sha256 = rawJsonSchemaSha256;
assert(validateDatasetV2HttpJson(rawJsonDatasetV2HttpJson), "OpenAPI rejects valid raw-JSON V2");
assert(
  !validateDatasetV2HttpJson({ ...rawJsonDatasetV2HttpJson, time_range: datasetV2HttpJson.time_range }),
  "OpenAPI accepted a raw V2 frame manifest with source-time range",
);
assert(
  !validateDatasetV2HttpJson({
    ...rawJsonDatasetV2HttpJson,
    object: { ...rawJsonV2Object, parquet_schema_sha256: rawSchemaSha256 },
  }),
  "OpenAPI accepted a crossed raw-frame V2 schema fingerprint",
);
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
const rawDatasetHttpJson = {
  schema_version: 1,
  dataset_id: "synthetic-raw-frame-v1",
  source: {
    provider: "synthetic",
    feed: "synthetic",
    entitlement: "unknown",
    numeric_encoding: "raw_messagepack_bytes",
  },
  symbols: ["QQQ"],
  source_timestamp_missing_rows: "1",
  row_count: "1",
  object: {
    object_name: "raw.parquet",
    object_id: "local-test:raw-frame",
    size_bytes: "10",
    content_sha256: "a".repeat(64),
    parquet_schema_sha256: rawSchemaSha256,
    parquet_footer_rows: "1",
    transport: "local_test",
  },
  completion: {
    input_eof: true,
    readback_sha256: "a".repeat(64),
    verified_before_publish: true,
  },
};
assert(validateDatasetHttpJson(rawDatasetHttpJson), "OpenAPI rejects a registered raw-frame manifest");
for (const invalidRawManifest of [
  { ...rawDatasetHttpJson, time_range: {} },
  {
    ...rawDatasetHttpJson,
    source: { ...rawDatasetHttpJson.source, numeric_encoding: "decimal_token" },
  },
  {
    ...rawDatasetHttpJson,
    object: { ...rawDatasetHttpJson.object, parquet_schema_sha256: "b".repeat(64) },
  },
]) {
  assert(!validateDatasetHttpJson(invalidRawManifest), "OpenAPI accepted an inconsistent raw manifest");
}
const rawJsonDatasetHttpJson = {
  ...rawDatasetHttpJson,
  dataset_id: "synthetic-raw-json-frame-v1",
  source: { ...rawDatasetHttpJson.source, numeric_encoding: "raw_json_bytes" },
  object: { ...rawDatasetHttpJson.object, parquet_schema_sha256: rawJsonSchemaSha256 },
};
assert(validateDatasetHttpJson(rawJsonDatasetHttpJson), "OpenAPI rejects a registered raw-JSON manifest");
for (const invalidRawJsonManifest of [
  {
    ...rawJsonDatasetHttpJson,
    object: { ...rawJsonDatasetHttpJson.object, parquet_schema_sha256: rawSchemaSha256 },
  },
  {
    ...rawJsonDatasetHttpJson,
    source: { ...rawJsonDatasetHttpJson.source, numeric_encoding: "decimal_token" },
  },
]) {
  assert(!validateDatasetHttpJson(invalidRawJsonManifest), "OpenAPI accepted a crossed raw schema");
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
