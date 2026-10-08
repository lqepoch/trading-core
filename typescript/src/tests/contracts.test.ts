import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { Ajv2020 } from "ajv/dist/2020.js";
import { create, fromJson, toJsonString } from "@bufbuild/protobuf";
import type { JsonValue } from "@bufbuild/protobuf";
import {
  parseCanonicalUint64Json,
  finiteBatchSealReceiptProtojsonBytes,
  finiteBatchSealReceiptSha256,
  datasetCompletionEvidenceV2ProtojsonBytes,
  datasetCompletionEvidenceV2Sha256,
  parseDatasetManifestProtoJson,
  parseDatasetManifestV2Json,
  parseDatasetManifestV2ProtoJson,
  datasetManifestV2ProtojsonBytes,
  parseEngineStatusResponseV1ProtoJsonText,
  parseMarketEventProtoJson,
  parsePredictionEnvelopeProtoJson,
  parsePredictionEnvelopeProtoJsonText,
  parseUsEquityTradeBarV2ProtoJson,
  parseSyntheticOfflinePreviewV1ProtoJsonText,
  validateUint64JsonPaths,
  validateBarV2CompletionEvidenceReference,
  validateUsEquityTradeBarV2AgainstManifest,
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
import {
  MARKET_EVENT_V3_SCHEMA_ID,
  MARKET_RAW_FRAME_V2_SCHEMA_ID,
  MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID,
  MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES,
  MAX_RAW_FRAME_EVENT_COUNT,
  MAX_RAW_FRAME_SYMBOLS_JSON_BYTES,
  validateCaptureInstanceIdV2,
  validateEventAgainstRawFrameRowV2,
  validateMarketEventRowV3,
  validateRawEventChunkV2,
  validateRawFrameRowV2,
  type ParquetRow,
} from "../contracts/raw-frame.js";
import {
  FiniteBatchCompletionV2Schema,
  FiniteBatchSourceKindV2,
} from "../gen/lqepoch/dataset/v2/manifest_pb.js";
import { MarketEventEnvelopeV1Schema } from "../gen/lqepoch/market/v1/market_pb.js";

function readFixture<T>(path: string): T {
  return JSON.parse(readFileSync(resolve(process.cwd(), "..", path), "utf8")) as T;
}

const require = createRequire(import.meta.url);
const addFormats = require("ajv-formats").default as (ajv: Ajv2020) => Ajv2020;

type RawFrameFixture = {
  row: Record<string, unknown>;
  frame_bytes_hex: string;
};
type RawFrameCaptureFixture = {
  messagepack_schema_id: string;
  messagepack_frames: RawFrameFixture[];
  messagepack_events: Record<string, unknown>[];
  json_schema_id: string;
  json_frame: RawFrameFixture;
  json_event: Record<string, unknown>;
};

function materializeRawFrame(fixture: RawFrameFixture): Record<string, unknown> {
  return {
    ...fixture.row,
    frame_bytes: Uint8Array.from(Buffer.from(fixture.frame_bytes_hex, "hex")),
  };
}

const rawFrameCapture = readFixture<RawFrameCaptureFixture>(
  "schemas/fixtures/raw-frame-capture-v2.json",
);
const messagepackFrames = rawFrameCapture.messagepack_frames.map(materializeRawFrame);
const messagepackEvents = structuredClone(rawFrameCapture.messagepack_events);
const jsonFrame = materializeRawFrame(rawFrameCapture.json_frame);
const jsonEvent = structuredClone(rawFrameCapture.json_event);
assert(rawFrameCapture.messagepack_schema_id === MARKET_RAW_FRAME_V2_SCHEMA_ID, "raw V2 schema ID changed");
assert(rawFrameCapture.json_schema_id === MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID, "raw JSON V2 schema ID changed");
validateRawEventChunkV2(messagepackFrames, messagepackEvents, MARKET_RAW_FRAME_V2_SCHEMA_ID);
validateRawFrameRowV2(jsonFrame, MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID);
validateEventAgainstRawFrameRowV2(jsonEvent, jsonFrame, MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID);
validateMarketEventRowV3(jsonEvent);
assert(MARKET_EVENT_V3_SCHEMA_ID === "lqepoch.market_event.v3", "event V3 schema ID changed");

type TimestampBoundaryCase = { name: string; timestamp_utc: string; epoch_nanoseconds?: string };
type TimestampBoundaryFixture = { valid: TimestampBoundaryCase[]; invalid: TimestampBoundaryCase[] };
const timestampBoundaries = readFixture<TimestampBoundaryFixture>(
  "schemas/fixtures/raw-frame-timestamp-ns-v2.json",
);
for (const testCase of timestampBoundaries.valid) {
  const frame = structuredClone(messagepackFrames[0]!);
  frame.received_timestamp_utc = testCase.timestamp_utc;
  validateRawFrameRowV2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID);

  const event = structuredClone(messagepackEvents[0]!);
  event.received_timestamp = testCase.timestamp_utc;
  event.source_timestamp = testCase.timestamp_utc;
  validateMarketEventRowV3(event);
}
for (const testCase of timestampBoundaries.invalid) {
  const frame = structuredClone(messagepackFrames[0]!);
  frame.received_timestamp_utc = testCase.timestamp_utc;
  rejects(
    () => validateRawFrameRowV2(frame, MARKET_RAW_FRAME_V2_SCHEMA_ID),
    `raw V2 accepted ${testCase.name}`,
  );

  const invalidReceived = structuredClone(messagepackEvents[0]!);
  invalidReceived.received_timestamp = testCase.timestamp_utc;
  rejects(
    () => validateMarketEventRowV3(invalidReceived),
    `event V3 accepted ${testCase.name} in received_timestamp`,
  );

  const invalidSource = structuredClone(messagepackEvents[0]!);
  invalidSource.source_timestamp = testCase.timestamp_utc;
  rejects(
    () => validateMarketEventRowV3(invalidSource),
    `event V3 accepted ${testCase.name} in source_timestamp`,
  );
}
const invalidEventSources = readFixture<{
  invalid: Array<{
    name: string;
    provider: string;
    feed: string;
    numeric_encoding: string;
    raw_frame_sha256: string | null;
  }>;
}>("schemas/fixtures/raw-frame-event-source-invalid-v3.json");
for (const testCase of invalidEventSources.invalid) {
  const event = structuredClone(messagepackEvents[0]!);
  event.provider = testCase.provider;
  event.feed = testCase.feed;
  event.numeric_encoding = testCase.numeric_encoding;
  event.raw_frame_sha256 = testCase.raw_frame_sha256;
  for (const field of [
    "raw_frame_capture_instance_id",
    "raw_frame_source_generation",
    "raw_frame_generation",
    "raw_frame_sequence",
    "raw_frame_event_ordinal",
    "raw_frame_event_count",
  ]) {
    event[field] = null;
  }
  rejects(() => validateMarketEventRowV3(event), `event V3 accepted ${testCase.name}`);
}
const invalidUnicodeScalars = readFixture<{
  valid_unicode_symbols_json: string;
  valid_unicode_symbols_json_utf8_bytes: number;
  invalid_symbols_json: string;
  invalid_event_symbol_json: string;
}>("schemas/fixtures/raw-frame-unicode-scalar-v2-v3.json");
const unicodeFrame = { ...messagepackFrames[0]!, symbols_json: invalidUnicodeScalars.valid_unicode_symbols_json };
validateRawFrameRowV2(unicodeFrame, MARKET_RAW_FRAME_V2_SCHEMA_ID);
assert(
  new TextEncoder().encode(unicodeFrame.symbols_json as string).byteLength ===
    invalidUnicodeScalars.valid_unicode_symbols_json_utf8_bytes,
  "UTF-8 metadata byte fixture differs",
);
rejects(
  () => validateRawFrameRowV2(
    { ...messagepackFrames[0]!, symbols_json: invalidUnicodeScalars.invalid_symbols_json },
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "raw-frame validator accepted an unpaired surrogate in symbols_json",
);
rejects(
  () => validateRawFrameRowV2(
    {
      ...messagepackFrames[0]!,
      symbols_json: invalidUnicodeScalars.invalid_symbols_json.replace("\\ud800", "\ud800"),
    },
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "raw-frame validator accepted a symbols_json string with an unpaired surrogate",
);
const nativeTextEncoder = globalThis.TextEncoder;
function rejectsTextBeforeEncoding(value: string, validate: () => void, message: string): void {
  let encodedValue = false;
  class TrackingTextEncoder extends nativeTextEncoder {
    override encode(input?: string) {
      if (input === value) encodedValue = true;
      return super.encode(input);
    }
  }
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "TextEncoder");
  assert(descriptor !== undefined, "TextEncoder descriptor is unavailable");
  Object.defineProperty(globalThis, "TextEncoder", { ...descriptor, value: TrackingTextEncoder });
  try {
    rejects(validate, message);
    assert(!encodedValue, "oversized or invalid text reached UTF-8 encoding");
  } finally {
    Object.defineProperty(globalThis, "TextEncoder", descriptor);
  }
}
const oversizedSymbolsJson = "x".repeat(MAX_RAW_FRAME_SYMBOLS_JSON_BYTES + 1);
rejectsTextBeforeEncoding(
  oversizedSymbolsJson,
  () => validateRawFrameRowV2(
    { ...messagepackFrames[0]!, symbols_json: oversizedSymbolsJson },
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "raw-frame validator accepted oversized symbols_json",
);
const loneSurrogateSymbolsJson = '["\ud800"]';
rejectsTextBeforeEncoding(
  loneSurrogateSymbolsJson,
  () => validateRawFrameRowV2(
    { ...messagepackFrames[0]!, symbols_json: loneSurrogateSymbolsJson },
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "raw-frame validator accepted an unpaired surrogate before UTF-8 encoding",
);
const oversizedSourceRecordId = "x".repeat(129);
rejectsTextBeforeEncoding(
  oversizedSourceRecordId,
  () => validateMarketEventRowV3({
    ...messagepackEvents[0]!,
    source_record_id: oversizedSourceRecordId,
  }),
  "event validator accepted an oversized source record identity",
);
const oversizedEventSymbol = "X".repeat(257);
rejectsTextBeforeEncoding(
  oversizedEventSymbol,
  () => validateMarketEventRowV3({
    ...messagepackEvents[0]!,
    symbol: oversizedEventSymbol,
  }),
  "event validator accepted an oversized symbol identity",
);
rejects(
  () => validateMarketEventRowV3({
    ...messagepackEvents[0]!,
    source_record_id: "é".repeat(65),
  }),
  "event validator accepted a source record identity over its UTF-8 byte limit",
);
rejects(
  () => validateMarketEventRowV3({
    ...messagepackEvents[0]!,
    symbol: "é".repeat(129),
  }),
  "event validator accepted a symbol over its UTF-8 byte limit",
);
const byteOversizedSymbolsJson = `["${"é".repeat(
  Math.floor((MAX_RAW_FRAME_SYMBOLS_JSON_BYTES - 4) / 2) + 1,
)}"]`;
assert(
  byteOversizedSymbolsJson.length <= MAX_RAW_FRAME_SYMBOLS_JSON_BYTES &&
    new nativeTextEncoder().encode(byteOversizedSymbolsJson).byteLength > MAX_RAW_FRAME_SYMBOLS_JSON_BYTES,
  "multibyte over-limit fixture does not isolate the bounded UTF-8 check",
);
rejects(
  () => validateRawFrameRowV2(
    { ...messagepackFrames[0]!, symbols_json: byteOversizedSymbolsJson },
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "raw-frame validator accepted symbols_json above its UTF-8 byte cap",
);
rejects(
  () => validateMarketEventRowV3({
    ...messagepackEvents[0]!,
    symbol: JSON.parse(invalidUnicodeScalars.invalid_event_symbol_json) as string,
  }),
  "event validator accepted an unpaired surrogate symbol",
);
rejects(
  () => validateMarketEventRowV3({ ...messagepackEvents[0]!, source_record_id: "record-\ud800" }),
  "event validator accepted an unpaired surrogate source identity",
);
rejects(
  () => validateRawEventChunkV2(
    new Array<ParquetRow>(1),
    [],
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "capture validator accepted a sparse raw-frame array",
);
rejects(
  () => validateRawEventChunkV2(
    [messagepackFrames[0]!],
    new Array<ParquetRow>(1),
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "capture validator accepted a sparse event array",
);

for (const invalid of [
  "0123456789AB4def8123456789abcdef",
  "0123456789ab3def8123456789abcdef",
  "0123456789ab4def7123456789abcdef",
  "01234567-89ab-4def-8123-456789abcdef",
]) {
  rejects(() => validateCaptureInstanceIdV2(invalid), `accepted invalid capture ID ${invalid}`);
}
const badHashFrame = structuredClone(messagepackFrames[0]!);
badHashFrame.frame_sha256 = "0".repeat(64);
rejects(
  () => validateRawFrameRowV2(badHashFrame, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "raw-frame validator accepted a mismatched exact-byte SHA",
);
const badCaptureFrame = structuredClone(messagepackFrames[0]!);
badCaptureFrame.capture_instance_id = "0123456789ab4def7123456789abcdef";
rejects(
  () => validateRawFrameRowV2(badCaptureFrame, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "raw-frame validator accepted a non-RFC capture ID",
);
const invalidClockFrame = structuredClone(messagepackFrames[0]!);
invalidClockFrame.received_timestamp_utc = "2026-10-08T25:00:00Z";
rejects(
  () => validateRawFrameRowV2(invalidClockFrame, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "raw-frame validator accepted an invalid UTC clock time",
);
const newlineGenerationEvent = structuredClone(messagepackEvents[0]!);
newlineGenerationEvent.generation = "19\n";
rejects(
  () => validateMarketEventRowV3(newlineGenerationEvent),
  "event validator accepted uint64 with a terminal newline",
);
const badReceivedAtEvent = structuredClone(messagepackEvents[0]!);
badReceivedAtEvent.received_timestamp = "2026-10-08T14:30:00.000000001Z";
rejects(
  () => validateEventAgainstRawFrameRowV2(
    badReceivedAtEvent,
    messagepackFrames[0]!,
    MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "raw/event validator accepted a receive-time mismatch",
);
const missingProjection = messagepackEvents.slice(0, 1);
rejects(
  () => validateRawEventChunkV2(messagepackFrames, missingProjection, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted a missing normalized projection",
);
const gapFrames = structuredClone(messagepackFrames);
gapFrames[1]!.source_frame_sequence = "13";
rejects(
  () => validateRawEventChunkV2(gapFrames, messagepackEvents, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted a source-frame sequence gap",
);
const mixedCaptureFrames = structuredClone(messagepackFrames);
mixedCaptureFrames[1]!.capture_instance_id = "fedcba9876544def8123456789abcdef";
rejects(
  () => validateRawEventChunkV2(mixedCaptureFrames, messagepackEvents, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted mixed capture IDs",
);
const mixedGenerationFrames = structuredClone(messagepackFrames);
mixedGenerationFrames[1]!.source_generation = "8";
rejects(
  () => validateRawEventChunkV2(mixedGenerationFrames, messagepackEvents, MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted mixed source generations",
);
const duplicateOrdinalFrame = structuredClone(messagepackFrames[0]!);
duplicateOrdinalFrame.event_count = 2;
const duplicateOrdinalEvents = [
  structuredClone(messagepackEvents[0]!),
  structuredClone(messagepackEvents[0]!),
];
duplicateOrdinalEvents[1]!.sequence = "112";
duplicateOrdinalEvents[1]!.raw_frame_event_count = 2;
rejects(
  () => validateRawEventChunkV2(
    [duplicateOrdinalFrame], duplicateOrdinalEvents, MARKET_RAW_FRAME_V2_SCHEMA_ID,
  ),
  "capture validator accepted duplicate event ordinals",
);
const tooManyFrames = Array.from({ length: 1025 }, () => structuredClone(messagepackFrames[0]!));
rejects(
  () => validateRawEventChunkV2(tooManyFrames, [], MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted more than 1024 raw frames",
);
const largePayload = new Uint8Array(1024 * 1024).fill(120);
const largePayloadSha = createHash("sha256").update(largePayload).digest("hex");
const largeChunk = Array.from({ length: 17 }, (_, index) => ({
  ...structuredClone(messagepackFrames[0]!),
  source_frame_sequence: String(index + 1),
  frame_bytes: largePayload,
  frame_sha256: largePayloadSha,
  event_count: 0,
  disposition: "unknown_message",
  symbols_json: "[]",
}));
rejects(
  () => validateRawEventChunkV2(largeChunk, [], MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted a raw chunk above 16 MiB",
);
const largeSymbols = Array.from(
  { length: MAX_RAW_FRAME_EVENT_COUNT },
  (_, index) => `A${'"'.repeat(250)}${index.toString(16).padStart(5, "0")}`,
);
const largeSymbolsJson = JSON.stringify(largeSymbols);
assert(
  new TextEncoder().encode(largeSymbolsJson).byteLength <= MAX_RAW_FRAME_SYMBOLS_JSON_BYTES,
  "per-frame symbols fixture exceeds its own bound",
);
assert(
  new TextEncoder().encode(largeSymbolsJson).byteLength * 65 > MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES,
  "aggregate symbols fixture does not exceed the chunk bound",
);
const largeMetadataChunk = Array.from({ length: 65 }, (_, index) => ({
  ...structuredClone(messagepackFrames[0]!),
  source_frame_sequence: String(index + 1),
  event_count: MAX_RAW_FRAME_EVENT_COUNT,
  symbols_json: largeSymbolsJson,
}));
rejects(
  () => validateRawEventChunkV2(largeMetadataChunk, [], MARKET_RAW_FRAME_V2_SCHEMA_ID),
  "capture validator accepted symbols metadata above 16 MiB",
);

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

type DatasetManifestProtoJsonGoldenCases = {
  cases: Array<{
    name: string;
    input_fixture: string;
    canonical_fixture: string;
    sha256: string;
  }>;
};
const datasetManifestProtoJsonCases = readFixture<DatasetManifestProtoJsonGoldenCases>(
  "schemas/fixtures/dataset-manifest-v2-protojson-cases.json",
);
for (const testCase of datasetManifestProtoJsonCases.cases) {
  const manifest = parseDatasetManifestV2ProtoJson(
    readFixture<Record<string, unknown>>(`schemas/fixtures/${testCase.input_fixture}`),
  );
  const bytes = datasetManifestV2ProtojsonBytes(manifest);
  const expected = readFileSync(
    resolve(process.cwd(), "..", "schemas/fixtures", testCase.canonical_fixture),
  );
  assert(Buffer.from(bytes).equals(expected), `canonical ProtoJSON changed for ${testCase.name}`);
  assert(bytes.at(-1) !== 10, `${testCase.name} unexpectedly has a trailing LF`);
  assert(createHash("sha256").update(bytes).digest("hex") === testCase.sha256,
    `canonical manifest SHA changed for ${testCase.name}`);
  const roundtrip = parseDatasetManifestV2Json(new TextDecoder().decode(bytes));
  assert(
    Buffer.from(datasetManifestV2ProtojsonBytes(roundtrip)).equals(expected),
    `manifest re-serialization changed for ${testCase.name}`,
  );
}
const invalidManifestForWriter = {
  ...datasetV2Message,
  schemaVersion: 1,
} as typeof datasetV2Message;
rejects(
  () => datasetManifestV2ProtojsonBytes(invalidManifestForWriter),
  "manifest ProtoJSON writer accepted an unsupported schema version",
);
for (const fixture of [
  "dataset-manifest-v2-invalid-symbol-lone-surrogate.json",
  "dataset-manifest-v2-invalid-source-id-lone-surrogate.json",
]) {
  const text = readFileSync(resolve(process.cwd(), "..", "schemas/fixtures", fixture), "utf8");
  rejects(() => parseDatasetManifestV2Json(text), `dataset manifest parser accepted ${fixture}`);
}
const invalidUnicodeSymbols = {
  ...datasetV2Message,
  symbols: ["QQQ \uD800"],
} as typeof datasetV2Message;
rejects(
  () => datasetManifestV2ProtojsonBytes(invalidUnicodeSymbols),
  "manifest ProtoJSON writer encoded an unpaired surrogate in symbols",
);
if (datasetV2Message.source === undefined) throw new Error("dataset v2 source missing from fixture");
const invalidUnicodeSourceId = {
  ...datasetV2Message,
  source: { ...datasetV2Message.source, sourceRecordId: "synthetic-record\uD800" },
} as typeof datasetV2Message;
rejects(
  () => datasetManifestV2ProtojsonBytes(invalidUnicodeSourceId),
  "manifest ProtoJSON writer encoded an unpaired surrogate in source identity",
);
const invalidUnicodeProvider = {
  ...datasetV2Message,
  source: { ...datasetV2Message.source, provider: "alpaca\uD800" },
} as typeof datasetV2Message;
rejects(
  () => datasetManifestV2ProtojsonBytes(invalidUnicodeProvider),
  "manifest ProtoJSON writer encoded an unpaired surrogate in provider identity",
);
const invalidUnicodeObjectId = {
  ...datasetV2Message,
  object: {
    ...datasetV2Message.object,
    objectId: "drive-object\uD800",
    transport: "rclone_google_drive",
  },
} as typeof datasetV2Message;
rejects(
  () => datasetManifestV2ProtojsonBytes(invalidUnicodeObjectId),
  "manifest ProtoJSON writer encoded an unpaired surrogate in object identity",
);
if (datasetV2Message.completionEvidence.evidence.case !== "finiteBatch") {
  throw new Error("dataset v2 finite receipt fixture selected the wrong oneof case");
}
const finiteBatchReceipt = datasetV2Message.completionEvidence.evidence.value;
const receiptBytes = finiteBatchSealReceiptProtojsonBytes(finiteBatchReceipt);
const expectedReceiptBytes = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/finite-batch-seal-receipt-v2.protojson"),
);
const expectedReceiptSha256 = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/finite-batch-seal-receipt-v2.sha256"),
  "ascii",
).trim();
assert(Buffer.from(receiptBytes).equals(expectedReceiptBytes), "finite receipt ProtoJSON bytes changed");
assert(receiptBytes.at(-1) !== 10, "finite receipt projection unexpectedly has a trailing LF");
assert(!new TextDecoder().decode(receiptBytes).includes("sealReceiptSha256"), "finite receipt is self-referential");
assert(finiteBatchSealReceiptSha256(finiteBatchReceipt) === expectedReceiptSha256, "finite receipt SHA changed");
assert(finiteBatchReceipt.sealReceiptSha256 === expectedReceiptSha256, "manifest receipt SHA changed");
const completionEvidenceHashes = readFixture<Record<string, string>>(
  "schemas/fixtures/dataset-completion-evidence-v2-sha256.json",
);
const finiteEvidenceBytes = datasetCompletionEvidenceV2ProtojsonBytes(datasetV2Message);
assert(finiteEvidenceBytes.at(-1) !== 10, "completion evidence projection unexpectedly has a trailing LF");
assert(
  datasetCompletionEvidenceV2Sha256(datasetV2Message) === completionEvidenceHashes.finite_batch,
  "finite completion evidence hash differs from the shared golden",
);
for (const [path, evidenceCase] of [
  ["schemas/fixtures/dataset-manifest-v2-provider-watermark.json", "provider_watermark"],
  ["schemas/fixtures/dataset-manifest-v2-diagnostic-stream.json", "diagnostic_stream"],
  ["schemas/fixtures/dataset-manifest-v2-provider-watermark-zero-lateness.json", "provider_watermark_zero_lateness"],
] as const) {
  const candidate = parseDatasetManifestV2ProtoJson(readFixture<Record<string, unknown>>(path));
  const payload = datasetCompletionEvidenceV2ProtojsonBytes(candidate);
  assert(
    datasetCompletionEvidenceV2Sha256(candidate) === completionEvidenceHashes[evidenceCase],
    `${evidenceCase} completion evidence hash differs from the shared golden`,
  );
  if (evidenceCase === "provider_watermark_zero_lateness") {
    assert(Buffer.from(payload).equals(readFileSync(resolve(
      process.cwd(), "..", "schemas/fixtures/dataset-completion-evidence-v2-provider-zero-lateness.protojson",
    ))), "zero allowed lateness ProtoJSON bytes differ from the shared golden");
    assert(new TextDecoder().decode(payload).includes('"allowedLatenessNs":"0"'), "zero allowed lateness was omitted");
  }
}
const barV2Json = readFixture<Record<string, unknown>>(
  "schemas/fixtures/us-equity-trade-bar-v2.json",
);
const barV2 = parseUsEquityTradeBarV2ProtoJson(barV2Json);
validateBarV2CompletionEvidenceReference(barV2, datasetV2Message);
validateUsEquityTradeBarV2AgainstManifest(barV2, datasetV2Message);
const barV2Snake = parseUsEquityTradeBarV2ProtoJson(
  readFixture<Record<string, unknown>>("schemas/fixtures/us-equity-trade-bar-v2-snake.json"),
);
assert(barV2Snake.sourceProvider === barV2.sourceProvider, "BarV2 snake_case fixture changed semantics");
validateUsEquityTradeBarV2AgainstManifest(barV2Snake, datasetV2Message);
for (const [caseName, manifestName] of [
  ["provider-watermark", "provider-watermark"],
  ["diagnostic-stream", "diagnostic-stream"],
  ["historical-non-paged", "historical-non-paged"],
  ["synthetic-replay", "synthetic-replay"],
  ["provider-watermark-zero-lateness", "provider-watermark-zero-lateness"],
] as const) {
  const caseBar = parseUsEquityTradeBarV2ProtoJson(
    readFixture<Record<string, unknown>>(`schemas/fixtures/us-equity-trade-bar-v2-${caseName}.json`),
  );
  const caseManifest = parseDatasetManifestV2ProtoJson(
    readFixture<Record<string, unknown>>(`schemas/fixtures/dataset-manifest-v2-${manifestName}.json`),
  );
  validateUsEquityTradeBarV2AgainstManifest(caseBar, caseManifest);
}
const invalidDateBar = { ...barV2, tradeDate: "2026-02-30" };
rejects(
  () => validateUsEquityTradeBarV2AgainstManifest(invalidDateBar, datasetV2Message),
  "BarV2 generated-message validator accepted an invalid trade date",
);
const badBarEvidence = structuredClone(barV2Json) as Record<string, unknown>;
badBarEvidence.completionEvidenceSha256 = "e".repeat(64);
rejects(
  () => validateBarV2CompletionEvidenceReference(parseUsEquityTradeBarV2ProtoJson(badBarEvidence), datasetV2Message),
  "BarV2 accepted a different manifest completion digest",
);
rejects(
  () => parseUsEquityTradeBarV2ProtoJson({ ...barV2Json, tradeCount: 4 }),
  "BarV2 accepted uint64 JSON number",
);
for (const [field, invalid] of [
  ["high", "9.00"],
  ["open", "0"],
  ["volume", "-1"],
  ["barEndExclusiveUtc", "2026-10-08T14:31:00.000000001Z"],
] as const) {
  rejects(
    () => parseUsEquityTradeBarV2ProtoJson({ ...barV2Json, [field]: invalid }),
    `BarV2 accepted invalid ${field}`,
  );
}
rejects(
  () => parseUsEquityTradeBarV2ProtoJson({ ...barV2Json, unrecognized: true }),
  "BarV2 accepted an unknown field",
);
rejects(
  () => parseUsEquityTradeBarV2ProtoJson({ ...barV2Json, schema_version: 2 }),
  "BarV2 accepted both camelCase and snake_case spellings",
);
rejects(
  () => parseUsEquityTradeBarV2ProtoJson({ ...barV2Json, nbboInputStatus: "x".repeat(65_537) }),
  "BarV2 accepted a row above its byte limit",
);
rejects(
  () => validateUsEquityTradeBarV2AgainstManifest(
    parseUsEquityTradeBarV2ProtoJson({ ...barV2Json, sourceProvider: "another-provider" }),
    datasetV2Message,
  ),
  "BarV2 accepted source fields that differ from its manifest",
);
const barWithoutPageEvidence = { ...barV2Json };
delete barWithoutPageEvidence.sourcePagesExhausted;
rejects(
  () => validateUsEquityTradeBarV2AgainstManifest(
    parseUsEquityTradeBarV2ProtoJson(barWithoutPageEvidence),
    datasetV2Message,
  ),
  "BarV2 accepted missing page-exhaustion presence for a paged manifest",
);
const sourceBoundsFixture = readFixture<{
  manifestRange: { startInclusive: string; endExclusive: string };
  cases: Array<{ name: string; field: string; value: string }>;
}>("schemas/fixtures/us-equity-trade-bar-v2-source-bounds-invalid.json");
const wideManifestJson = readFixture<Record<string, any>>(
  "schemas/fixtures/dataset-manifest-v2-provider-watermark.json",
);
wideManifestJson.timeRange = sourceBoundsFixture.manifestRange;
wideManifestJson.completionEvidence.providerWatermark.completeUpToExclusive =
  sourceBoundsFixture.manifestRange.endExclusive;
const wideManifest = parseDatasetManifestV2ProtoJson(wideManifestJson);
const wideBarJson = readFixture<Record<string, unknown>>(
  "schemas/fixtures/us-equity-trade-bar-v2-provider-watermark.json",
);
wideBarJson.completionEvidenceSha256 = datasetCompletionEvidenceV2Sha256(wideManifest);
for (const sourceBoundsCase of sourceBoundsFixture.cases) {
  assert(
    sourceBoundsCase.value > sourceBoundsFixture.manifestRange.startInclusive &&
      sourceBoundsCase.value < sourceBoundsFixture.manifestRange.endExclusive,
    `${sourceBoundsCase.name} must remain inside the wider manifest range`,
  );
  rejects(
    () => parseUsEquityTradeBarV2ProtoJson({
      ...wideBarJson,
      [sourceBoundsCase.field]: sourceBoundsCase.value,
    }),
    `BarV2 accepted ${sourceBoundsCase.name}`,
  );
}
const pagingCases = readFixture<{
  datasets: Array<{ name: string; manifest: string; bar: string }>;
  unexpectedPresenceValues: boolean[];
}>("schemas/fixtures/us-equity-trade-bar-v2-paging-cases.json");
for (const pagingCase of pagingCases.datasets) {
  const pagingManifest = parseDatasetManifestV2ProtoJson(
    readFixture<Record<string, unknown>>(pagingCase.manifest),
  );
  const pagingBarJson = readFixture<Record<string, unknown>>(pagingCase.bar);
  const pagingBar = parseUsEquityTradeBarV2ProtoJson(pagingBarJson);
  assert(pagingBar.sourcePagesExhausted === undefined, `${pagingCase.name} must omit page exhaustion`);
  validateUsEquityTradeBarV2AgainstManifest(pagingBar, pagingManifest);
  for (const presentValue of pagingCases.unexpectedPresenceValues) {
    const invalidPagingBarJson = { ...pagingBarJson, sourcePagesExhausted: presentValue };
    if (!presentValue) {
      rejects(
        () => parseUsEquityTradeBarV2ProtoJson(invalidPagingBarJson),
        `${pagingCase.name} accepted an explicit false page-exhaustion field`,
      );
    } else {
      const invalidPagingBar = parseUsEquityTradeBarV2ProtoJson(invalidPagingBarJson);
      rejects(
        () => validateUsEquityTradeBarV2AgainstManifest(invalidPagingBar, pagingManifest),
        `${pagingCase.name} accepted an unexpected true page-exhaustion field`,
      );
    }
  }
}
const nonpagedReceipt = create(FiniteBatchCompletionV2Schema, {
  ...finiteBatchReceipt,
  sourceKind: FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_HISTORICAL_NON_PAGED,
  inputSizeBytes: 18_446_744_073_709_551_615n,
  inputRecordCount: 18_446_744_073_709_551_615n,
  consumedRecordCount: 18_446_744_073_709_551_615n,
  pageCount: undefined,
  pagesExhausted: undefined,
  pageSetSha256: undefined,
});
const nonpagedReceiptBytes = finiteBatchSealReceiptProtojsonBytes(nonpagedReceipt);
const expectedNonpagedReceiptBytes = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/finite-batch-seal-receipt-v2-nonpaged-u64.protojson"),
);
const expectedNonpagedReceiptSha256 = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/finite-batch-seal-receipt-v2-nonpaged-u64.sha256"),
  "ascii",
).trim();
assert(Buffer.from(nonpagedReceiptBytes).equals(expectedNonpagedReceiptBytes), "non-paged max-u64 receipt bytes changed");
assert(
  finiteBatchSealReceiptSha256(nonpagedReceipt) === expectedNonpagedReceiptSha256,
  "non-paged max-u64 receipt SHA changed",
);
assert(!new TextDecoder().decode(nonpagedReceiptBytes).includes("pageCount"), "non-paged receipt emitted pageCount");
const invalidReceiptHash = structuredClone(datasetV2Json) as Record<string, unknown>;
const invalidReceiptEvidence = invalidReceiptHash.completionEvidence as Record<string, unknown>;
const invalidReceiptFinite = invalidReceiptEvidence.finiteBatch as Record<string, unknown>;
invalidReceiptFinite.sealReceiptSha256 = "e".repeat(64);
rejects(
  () => parseDatasetManifestV2ProtoJson(invalidReceiptHash),
  "dataset v2 accepted a seal hash that did not match the canonical projection",
);
for (const path of [
  "schemas/fixtures/dataset-manifest-v2-snake.json",
  "schemas/fixtures/dataset-manifest-v2-provider-watermark-snake.json",
]) {
  assert(
    parseDatasetManifestV2ProtoJson(readFixture<Record<string, unknown>>(path)).schemaVersion === 2,
    `full snake-case fixture failed: ${path}`,
  );
}

for (const [path, mutate] of [
  ["schema_version", (candidate: Record<string, unknown>) => {
    candidate.schema_version = candidate.schemaVersion;
  }],
  ["object_name", (candidate: Record<string, unknown>) => {
    const object = candidate.object as Record<string, unknown>;
    object.object_name = object.objectName;
  }],
] as const) {
  const candidate = structuredClone(datasetV2Json) as Record<string, unknown>;
  mutate(candidate);
  rejects(() => parseDatasetManifestV2ProtoJson(candidate), `accepted dual spellings for ${path}`);
}

const datasetV2WatermarkForAliases = readFixture<Record<string, unknown>>(
  "schemas/fixtures/dataset-manifest-v2-provider-watermark.json",
);
for (const [camel, snake] of [
  ["firstSequence", "first_sequence"],
  ["lastSequence", "last_sequence"],
  ["sequenceCount", "sequence_count"],
] as const) {
  const candidate = structuredClone(datasetV2WatermarkForAliases) as Record<string, unknown>;
  const completion = candidate.completionEvidence as Record<string, unknown>;
  const watermark = completion.providerWatermark as Record<string, unknown>;
  watermark[snake] = watermark[camel];
  rejects(() => parseDatasetManifestV2ProtoJson(candidate), `accepted both ${camel} spellings`);
}

const timestampV2Fixture = readFixture<{
  valid: Array<{ name: string; value: string }>;
  invalid: Array<{ name: string; value: string }>;
}>("schemas/fixtures/proto-timestamp-v2.json");
for (const testCase of timestampV2Fixture.valid) {
  const candidate = structuredClone(datasetV2Json) as Record<string, unknown>;
  const completion = candidate.completionEvidence as Record<string, unknown>;
  const finite = completion.finiteBatch as Record<string, unknown>;
  finite.completedAt = testCase.value;
  finite.sealReceiptSha256 = finiteBatchSealReceiptSha256(
    fromJson(FiniteBatchCompletionV2Schema, finite as JsonValue),
  );
  parseDatasetManifestV2ProtoJson(candidate);
}
for (const testCase of timestampV2Fixture.invalid) {
  const candidate = structuredClone(datasetV2Json) as Record<string, unknown>;
  const completion = candidate.completionEvidence as Record<string, unknown>;
  const finite = completion.finiteBatch as Record<string, unknown>;
  finite.completedAt = testCase.value;
  rejects(() => parseDatasetManifestV2ProtoJson(candidate), `accepted ${testCase.name}`);
}

const enumInvalidFixture = readFixture<{
  source_numeric_encoding: Array<{ name: string; value: number }>;
  finite_source_kind: Array<{ name: string; value: number }>;
}>("schemas/fixtures/dataset-manifest-v2-enum-invalid.json");
for (const testCase of enumInvalidFixture.source_numeric_encoding) {
  const candidate = structuredClone(datasetV2Json) as Record<string, unknown>;
  (candidate.source as Record<string, unknown>).numericEncoding = testCase.value;
  rejects(
    () => parseDatasetManifestV2ProtoJson(candidate),
    `accepted numeric source enum ${testCase.name}`,
  );
}
for (const testCase of enumInvalidFixture.finite_source_kind) {
  const candidate = structuredClone(datasetV2Json) as Record<string, unknown>;
  const completion = candidate.completionEvidence as Record<string, unknown>;
  (completion.finiteBatch as Record<string, unknown>).sourceKind = testCase.value;
  rejects(
    () => parseDatasetManifestV2ProtoJson(candidate),
    `accepted numeric finite enum ${testCase.name}`,
  );
}

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
const syntheticWatermark = structuredClone(datasetV2Watermark) as Record<string, unknown>;
const syntheticSource = syntheticWatermark.source as Record<string, unknown>;
syntheticSource.provider = "synthetic";
syntheticSource.feed = "synthetic";
syntheticSource.numericEncoding = "NUMERIC_ENCODING_DECIMAL_TOKEN";
const syntheticCompletion = syntheticWatermark.completionEvidence as Record<string, unknown>;
const syntheticProvider = syntheticCompletion.providerWatermark as Record<string, unknown>;
syntheticProvider.provider = "synthetic";
syntheticProvider.feed = "synthetic";
rejects(
  () => parseDatasetManifestV2ProtoJson(syntheticWatermark),
  "accepted synthetic source as provider watermark evidence",
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
const rawFrameV2SchemaSha256 = trustedParquetSchemaSha256("lqepoch.market_raw_frame.v2");
const rawJsonFrameV2SchemaSha256 = trustedParquetSchemaSha256("lqepoch.market_raw_json_frame.v2");
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
const predictionEnvelopeFixture = readFixture<Record<string, any>>(
  "schemas/fixtures/prediction-envelope-v1-quant-export.json",
);
function copyPredictionEnvelopeFixture(): Record<string, any> {
  return JSON.parse(JSON.stringify(predictionEnvelopeFixture)) as Record<string, any>;
}
const rawMessagePackPrediction = copyPredictionEnvelopeFixture();
rawMessagePackPrediction.source = rawManifest.source;
rejects(
  () => parsePredictionEnvelopeProtoJson(rawMessagePackPrediction),
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
  () => {
    const prediction = copyPredictionEnvelopeFixture();
    prediction.source = rawJsonManifest.source;
    return parsePredictionEnvelopeProtoJson(prediction);
  },
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
    () => {
      const prediction = copyPredictionEnvelopeFixture();
      prediction.source = snakeSource;
      return parsePredictionEnvelopeProtoJson(prediction);
    },
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

const predictionJson = copyPredictionEnvelopeFixture();
const predictionMessage = parsePredictionEnvelopeProtoJson(predictionJson);
assert(predictionMessage.forecast?.sequence === BigInt(max), "prediction sequence lost precision");
const predictionPartialWireFixture = readFixture<Record<string, unknown>>(
  "schemas/fixtures/prediction-envelope-v1-wire-partial-max.json",
);
assert(
  parsePredictionEnvelopeProtoJson(predictionPartialWireFixture).forecast?.sequence === BigInt(max),
  "wire-compatible partial prediction lost max uint64 precision",
);
const predictionText = JSON.stringify(predictionJson);
const predictionTextMessage = parsePredictionEnvelopeProtoJsonText(predictionText);
assert(
  predictionTextMessage.forecast?.sequence === BigInt(max),
  "raw prediction ProtoJSON text lost max uint64 precision",
);
assert(
  parsePredictionEnvelopeProtoJsonText(JSON.stringify(predictionPartialWireFixture)).forecast?.sequence === BigInt(max),
  "raw-text wire-compatible partial prediction lost max uint64 precision",
);
const predictionDefaultEnumsFixture = readFixture<Record<string, unknown>>(
  "schemas/fixtures/prediction-envelope-v1-wire-default-enums.json",
);
const predictionDefaultEnums = parsePredictionEnvelopeProtoJson(predictionDefaultEnumsFixture);
assert(
  predictionDefaultEnums.source?.numericEncoding === 0 &&
    predictionDefaultEnums.quality?.status === 0 &&
    predictionDefaultEnums.horizon?.unit === 0 &&
    predictionDefaultEnums.forecast?.forecastHorizon?.unit === 0,
  "wire parser did not preserve absent enum defaults",
);
rejects(
  () => {
    const numeric = copyPredictionEnvelopeFixture();
    numeric.forecast.sequence = 1;
    return parsePredictionEnvelopeProtoJsonText(JSON.stringify(numeric));
  },
  "raw prediction ProtoJSON text accepted numeric uint64",
);
rejects(
  () => parsePredictionEnvelopeProtoJsonText(
    predictionText.replace(
      '"predictionId":"prediction-1"',
      '"predictionId":"prediction-1","predictionId":"prediction-1"',
    ),
  ),
  "raw prediction ProtoJSON text accepted duplicate object keys",
);
rejects(
  () => parsePredictionEnvelopeProtoJsonText(
    predictionText.replace(
      '"predictionId":"prediction-1"',
      '"predictionId":"prediction-1","prediction_id":"prediction-1"',
    ),
  ),
  "raw prediction ProtoJSON text accepted conflicting camel/snake aliases",
);
rejects(
  () => parsePredictionEnvelopeProtoJsonText(
    JSON.stringify({ ...predictionJson, unknownField: "x" }),
  ),
  "raw prediction ProtoJSON text accepted an unknown field",
);
const oversizedPredictionJsonText = " ".repeat(2 * 1024 * 1024 + 1);
rejectsTextBeforeEncoding(
  oversizedPredictionJsonText,
  () => parsePredictionEnvelopeProtoJsonText(oversizedPredictionJsonText),
  "raw prediction ProtoJSON parser accepted text over its byte limit",
);
for (const testCase of uint64Fixture.invalid) {
  rejects(
    () => {
      const invalid = copyPredictionEnvelopeFixture();
      invalid.forecast.sequence = testCase.value;
      return parsePredictionEnvelopeProtoJson(invalid);
    },
    `prediction envelope accepted ${testCase.name}`,
  );
}

type PredictionProtoJsonMutation = {
  name: string;
  path: string[];
  value: unknown;
};
type PredictionProtoJsonTextReplacement = {
  name: string;
  needle: string;
  replacement: string;
};
type PredictionProtoJsonCases = {
  invalid_mutations: PredictionProtoJsonMutation[];
  invalid_text_replacements: PredictionProtoJsonTextReplacement[];
};
const predictionEnvelope = parsePredictionEnvelopeProtoJson(predictionEnvelopeFixture);
assert(
  predictionEnvelope.forecast?.sequence === BigInt(max),
  "quant prediction exporter fixture did not preserve maximum uint64",
);
const predictionNanosecondText = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/prediction-envelope-v1-nanosecond.json"),
  "utf8",
);
const predictionNanosecond = parsePredictionEnvelopeProtoJsonText(predictionNanosecondText);
assert(
  predictionNanosecond.createdAt?.nanos === 123_456_789,
  "prediction parser lost timestamp nanoseconds",
);
const predictionSnakeFixture = readFixture<Record<string, unknown>>(
  "schemas/fixtures/prediction-envelope-v1-snake.json",
);
assert(
  parsePredictionEnvelopeProtoJson(predictionSnakeFixture).forecast?.sequence === BigInt(max),
  "prediction parser rejected the shared snake-case ProtoJSON fixture",
);

const engineStatusText = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/engine-status-response-v1.json"),
  "utf8",
).trim();
const engineStatusFixture = JSON.parse(engineStatusText) as Record<string, unknown>;
const engineStatus = parseEngineStatusResponseV1ProtoJsonText(engineStatusText);
const parseEngineStatusFixture = (value: Record<string, unknown>) =>
  parseEngineStatusResponseV1ProtoJsonText(JSON.stringify(value));
assert(
  engineStatus.apiVersion === "v1" &&
    engineStatus.service === "offline-persist-preview" &&
    engineStatus.executionEnabled === false &&
    engineStatus.mutationRoutesEnabled === false &&
    engineStatus.pendingUnknownCount === 1 &&
    engineStatus.pendingUnknownConsumedRiskCount === 1 &&
    engineStatus.pendingUnknownUnverifiedRiskCount === 0,
  "engine status producer fixture changed or lost explicit safe values",
);
assert(
  parseEngineStatusFixture(engineStatusFixture).schemaVersion === 15,
  "engine status object parser changed the source schema version",
);
assert(
  parseEngineStatusFixture({ ...engineStatusFixture, schema_version: 0xffff_ffff })
    .schemaVersion === 0xffff_ffff,
  "engine status rejected the uint32 schema-version maximum",
);
rejects(
  () => parseEngineStatusResponseV1ProtoJsonText(
    engineStatusText.replace('"api_version": "v1"', '"apiVersion": "v1"'),
  ),
  "engine status accepted a non-wire camelCase alias",
);
rejects(
  () => parseEngineStatusResponseV1ProtoJsonText(
    engineStatusText.replace('"api_version": "v1"', '"api_version": "v1", "api_version": "v1"'),
  ),
  "engine status accepted duplicate JSON keys",
);
rejects(
  () => parseEngineStatusResponseV1ProtoJsonText(
    engineStatusText.replace('"api_version": "v1"', '"api_version": "v1", "apiVersion": "v1"'),
  ),
  "engine status accepted camel/snake aliases together",
);
rejects(
  () => parseEngineStatusFixture({ ...engineStatusFixture, execution_enabled: true }),
  "engine status accepted an execution-enabled projection",
);
rejects(
  () => {
    const missing = { ...engineStatusFixture };
    delete missing.mutation_routes_enabled;
    return parseEngineStatusFixture(missing);
  },
  "engine status accepted an absent explicit false safety field",
);
rejects(
  () => parseEngineStatusFixture({ ...engineStatusFixture, schema_version: 0 }),
  "engine status accepted a zero source schema version",
);
rejects(
  () => parseEngineStatusFixture({
    ...engineStatusFixture,
    schema_version: 0x1_0000_0000,
  }),
  "engine status accepted schema-version uint32 overflow",
);
rejects(
  () => parseEngineStatusFixture({
    ...engineStatusFixture,
    pending_unknown_count: 257,
    pending_unknown_count_capped: true,
    pending_unknown_consumed_risk_count: 257,
  }),
  "engine status accepted a sample above the 256-row bound",
);
rejects(
  () => parseEngineStatusFixture({
    ...engineStatusFixture,
    pending_unknown_consumed_risk_count: 0,
  }),
  "engine status accepted counts that do not partition the sample",
);
rejects(
  () => parseEngineStatusFixture({
    ...engineStatusFixture,
    pending_unknown_count_capped: true,
  }),
  "engine status accepted an inconsistent sample cap flag",
);
const cappedEngineStatus = parseEngineStatusFixture({
  ...engineStatusFixture,
  pending_unknown_count: 256,
  pending_unknown_count_capped: true,
  pending_unknown_consumed_risk_count: 128,
  pending_unknown_unverified_risk_count: 128,
});
assert(cappedEngineStatus.pendingUnknownCountCapped, "engine status rejected the exact cap boundary");
const oversizedEngineResponseText = " ".repeat(2 * 1024 * 1024 + 1);
rejectsTextBeforeEncoding(
  oversizedEngineResponseText,
  () => parseEngineStatusResponseV1ProtoJsonText(oversizedEngineResponseText),
  "engine status parser accepted text above the configured byte limit",
);

const syntheticPreviewText = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/synthetic-offline-preview-v1.json"),
  "utf8",
).trim();
const syntheticPreviewFixture = JSON.parse(syntheticPreviewText) as Record<string, unknown>;
const syntheticPreview = parseSyntheticOfflinePreviewV1ProtoJsonText(syntheticPreviewText);
const parseSyntheticPreviewFixture = (value: Record<string, unknown>) =>
  parseSyntheticOfflinePreviewV1ProtoJsonText(JSON.stringify(value));
assert(
  syntheticPreview.previewKind === "synthetic_session_state" &&
    syntheticPreview.executionEnabled === false &&
    syntheticPreview.orderMutationsEnabled === false &&
    syntheticPreview.accountDataLoaded === false &&
    syntheticPreview.marketDataConnected === false &&
    syntheticPreview.disposition === "reconciliation_required",
  "synthetic engine preview fixture changed or lost explicit safe values",
);
assert(
  parseSyntheticPreviewFixture(syntheticPreviewFixture).sourceSchemaVersion === 15,
  "synthetic engine preview object parser changed the source schema version",
);
rejects(
  () => parseSyntheticPreviewFixture({ ...syntheticPreviewFixture, source_schema_version: 0 }),
  "synthetic engine preview accepted a zero source schema version",
);
rejects(
  () => parseSyntheticOfflinePreviewV1ProtoJsonText(
    syntheticPreviewText.replace('"api_version": "v1"', '"api_version": "v1", "account_id": "x"'),
  ),
  "synthetic engine preview accepted an identity field",
);
rejects(
  () => parseSyntheticPreviewFixture({
    ...syntheticPreviewFixture,
    market_data_connected: true,
  }),
  "synthetic engine preview accepted connected market data",
);
rejects(
  () => parseSyntheticPreviewFixture({
    ...syntheticPreviewFixture,
    pending_unknown_unverified_risk_count: 1,
  }),
  "synthetic engine preview accepted a disposition inconsistent with risk evidence",
);
const unknownRiskPreview = parseSyntheticPreviewFixture({
  ...syntheticPreviewFixture,
  pending_unknown_consumed_risk_count: 0,
  pending_unknown_unverified_risk_count: 1,
  disposition: "unknown_reservation_state",
});
assert(
  unknownRiskPreview.disposition === "unknown_reservation_state",
  "synthetic preview rejected the unverified-risk classification",
);
const emptySyntheticPreview = parseSyntheticPreviewFixture({
  ...syntheticPreviewFixture,
  pending_unknown_count: 0,
  pending_unknown_count_capped: false,
  pending_unknown_consumed_risk_count: 0,
  pending_unknown_unverified_risk_count: 0,
  disposition: "no_pending_unknown_in_sample",
});
assert(
  emptySyntheticPreview.disposition === "no_pending_unknown_in_sample",
  "synthetic engine preview rejected the empty bounded sample classification",
);

const predictionCases = readFixture<PredictionProtoJsonCases>(
  "schemas/fixtures/prediction-envelope-v1-protojson-cases.json",
);
const crossLanguageInvalidCases = new Set([
  "numeric uint64",
  "leading-zero uint64",
  "uint64 overflow",
  "numeric quality enum",
  "unknown quality enum",
  "numeric horizon enum",
  "unknown horizon enum",
  "numeric numeric-encoding enum",
  "unknown numeric-encoding enum",
  "unspecified numeric encoding",
  "raw JSON encoding",
  "raw MessagePack encoding",
  "unknown field",
  "numeric timestamp",
  "timestamp over nanosecond precision",
  "leap-second timestamp",
]);
function setPredictionPath(document: Record<string, unknown>, path: string[], value: unknown): void {
  let current = document;
  for (const key of path.slice(0, -1)) {
    const nested = current[key];
    if (typeof nested !== "object" || nested === null || Array.isArray(nested)) {
      throw new Error(`prediction fixture path is not an object: ${key}`);
    }
    current = nested as Record<string, unknown>;
  }
  current[path.at(-1)!] = value;
}
for (const mutation of predictionCases.invalid_mutations) {
  if (!crossLanguageInvalidCases.has(mutation.name)) continue;
  const document = JSON.parse(JSON.stringify(predictionEnvelopeFixture)) as Record<string, unknown>;
  setPredictionPath(document, mutation.path, mutation.value);
  rejects(
    () => parsePredictionEnvelopeProtoJson(document),
    `prediction parser accepted shared invalid case: ${mutation.name}`,
  );
}
const predictionExportText = readFileSync(
  resolve(process.cwd(), "..", "schemas/fixtures/prediction-envelope-v1-quant-export.json"),
  "utf8",
);
for (const replacement of predictionCases.invalid_text_replacements) {
  assert(predictionExportText.includes(replacement.needle), "prediction fixture replacement needle is absent");
  const invalid = predictionExportText.replace(replacement.needle, replacement.replacement);
  rejects(
    () => parsePredictionEnvelopeProtoJsonText(invalid),
    `prediction text parser accepted shared invalid case: ${replacement.name}`,
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
const validateFiniteBatchReceiptHttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/FiniteBatchSealReceiptV2",
});
const validateBarV2HttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/UsEquityTradeBarV2",
});
const validateEngineStatusHttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/EngineStatusResponseV1",
});
const validateSyntheticPreviewHttpJson = ajv.compile({
  $ref: "lqepoch-openapi#/components/schemas/SyntheticOfflinePreviewV1",
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
assert(validateEngineStatusHttpJson(engineStatusFixture), "OpenAPI rejects the engine status fixture");
assert(
  validateSyntheticPreviewHttpJson(syntheticPreviewFixture),
  "OpenAPI rejects the synthetic engine preview fixture",
);
assert(
  !validateEngineStatusHttpJson({ ...engineStatusFixture, execution_enabled: true }),
  "OpenAPI accepted an execution-enabled status projection",
);
assert(
  !validateSyntheticPreviewHttpJson({ ...syntheticPreviewFixture, account_id: "x" }),
  "OpenAPI accepted an identity field in the synthetic preview",
);
const barV2HttpJson = toOpenApiSnakeCase(barV2Json) as Record<string, unknown>;
assert(validateBarV2HttpJson(barV2HttpJson), "OpenAPI rejects the BarV2 fixture");
for (const caseName of ["provider-watermark", "diagnostic-stream"] as const) {
  const caseHttpJson = toOpenApiSnakeCase(
    readFixture<Record<string, unknown>>(`schemas/fixtures/us-equity-trade-bar-v2-${caseName}.json`),
  );
  assert(validateBarV2HttpJson(caseHttpJson), `OpenAPI rejects the BarV2 ${caseName} fixture`);
}
assert(
  !validateBarV2HttpJson({ ...barV2HttpJson, source_pages_exhausted: false }),
  "OpenAPI accepted a false page-exhaustion claim",
);
assert(
  !validateBarV2HttpJson({ ...barV2HttpJson, open: "0" }),
  "OpenAPI accepted a zero trade price",
);
const receiptHttpJson = toOpenApiSnakeCase(
  JSON.parse(readFileSync(resolve(process.cwd(), "..", "schemas/fixtures/finite-batch-seal-receipt-v2.protojson"), "utf8")),
) as Record<string, unknown>;
assert(validateFiniteBatchReceiptHttpJson(receiptHttpJson), "OpenAPI rejects the finite-batch receipt projection");
assert(
  !validateFiniteBatchReceiptHttpJson({ ...receiptHttpJson, seal_receipt_sha256: "e".repeat(64) }),
  "OpenAPI accepted a self-referential finite-batch receipt projection",
);
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
const rawMessagepackDatasetV2HttpJson = structuredClone(datasetV2HttpJson);
const rawMessagepackV2Source = rawMessagepackDatasetV2HttpJson.source as Record<string, unknown>;
const rawMessagepackV2Object = rawMessagepackDatasetV2HttpJson.object as Record<string, unknown>;
rawMessagepackV2Source.numeric_encoding = "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES";
rawMessagepackDatasetV2HttpJson.source_timestamp_missing_rows = "1";
delete rawMessagepackDatasetV2HttpJson.time_range;
rawMessagepackV2Object.parquet_schema_sha256 = rawFrameV2SchemaSha256;
assert(
  validateDatasetV2HttpJson(rawMessagepackDatasetV2HttpJson),
  "OpenAPI rejects valid raw-MessagePack V2",
);
assert(
  !validateDatasetV2HttpJson({
    ...rawMessagepackDatasetV2HttpJson,
    object: { ...rawMessagepackV2Object, parquet_schema_sha256: rawJsonFrameV2SchemaSha256 },
  }),
  "OpenAPI accepted a crossed raw-JSON fingerprint for MessagePack bytes",
);
const rawJsonDatasetV2HttpJson = structuredClone(datasetV2HttpJson);
const rawJsonV2Source = rawJsonDatasetV2HttpJson.source as Record<string, unknown>;
const rawJsonV2Object = rawJsonDatasetV2HttpJson.object as Record<string, unknown>;
rawJsonV2Source.numeric_encoding = "NUMERIC_ENCODING_RAW_JSON_BYTES";
rawJsonDatasetV2HttpJson.source_timestamp_missing_rows = "1";
delete rawJsonDatasetV2HttpJson.time_range;
rawJsonV2Object.parquet_schema_sha256 = rawJsonFrameV2SchemaSha256;
assert(validateDatasetV2HttpJson(rawJsonDatasetV2HttpJson), "OpenAPI rejects valid raw-JSON V2");
assert(
  !validateDatasetV2HttpJson({ ...rawJsonDatasetV2HttpJson, time_range: datasetV2HttpJson.time_range }),
  "OpenAPI accepted a raw V2 frame manifest with source-time range",
);
assert(
  !validateDatasetV2HttpJson({
    ...rawJsonDatasetV2HttpJson,
    object: { ...rawJsonV2Object, parquet_schema_sha256: rawFrameV2SchemaSha256 },
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
assert(
  validateDatasetHttpJson({
    ...rawDatasetHttpJson,
    object: { ...rawDatasetHttpJson.object, parquet_schema_sha256: rawFrameV2SchemaSha256 },
  }),
  "OpenAPI rejects an additive raw-frame V2 manifest",
);
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
assert(
  validateDatasetHttpJson({
    ...rawJsonDatasetHttpJson,
    object: { ...rawJsonDatasetHttpJson.object, parquet_schema_sha256: rawJsonFrameV2SchemaSha256 },
  }),
  "OpenAPI rejects an additive raw-JSON V2 manifest",
);
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
