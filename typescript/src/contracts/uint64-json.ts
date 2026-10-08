import { fromJson, type JsonValue } from "@bufbuild/protobuf";
import {
  MarketEventEnvelopeV1Schema,
  NumericEncodingV1,
  type MarketEventEnvelopeV1,
} from "../gen/lqepoch/market/v1/market_pb.js";
import {
  DatasetManifestV1Schema,
  type DatasetManifestV1,
} from "../gen/lqepoch/dataset/v1/manifest_pb.js";
import {
  PredictionEnvelopeV1Schema,
  type PredictionEnvelopeV1,
} from "../gen/lqepoch/prediction/v1/prediction_pb.js";
import {
  DatasetManifestV2Schema,
  FiniteBatchSourceKindV2,
  type FiniteBatchCompletionV2,
  type DatasetManifestV2,
  type DatasetTimeRangeV2,
} from "../gen/lqepoch/dataset/v2/manifest_pb.js";
import { trustedParquetSchemaSha256 } from "./schema-fingerprint.js";

const UINT64_MAX = 18_446_744_073_709_551_615n;
const CANONICAL_UINT64 = /^(0|[1-9][0-9]*)$/;
const MAX_DATASET_MANIFEST_V2_JSON_BYTES = 2 * 1024 * 1024;
const MAX_DATASET_MANIFEST_V2_SYMBOLS = 4096;
const MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS = 60_000_000_000n;
const SHA256 = /^[0-9a-f]{64}$/;

export type JsonPath = readonly string[];

/** Parse the canonical JSON string projection without passing through Number. */
export function parseCanonicalUint64Json(value: unknown): bigint {
  if (
    typeof value !== "string" ||
    value.length > 20 ||
    /[^0-9]/.test(value) ||
    !CANONICAL_UINT64.test(value)
  ) {
    throw new TypeError("uint64 JSON values must be canonical decimal strings");
  }
  const parsed = BigInt(value);
  if (parsed > UINT64_MAX) {
    throw new RangeError("uint64 JSON value exceeds the unsigned 64-bit maximum");
  }
  return parsed;
}

/** Check all declared uint64 projections before handing data to a ProtoJSON parser. */
export function validateUint64JsonPaths(value: unknown, paths: readonly JsonPath[]): void {
  for (const path of paths) {
    let current: unknown = value;
    for (const part of path) {
      if (!isRecord(current)) {
        throw new TypeError(`missing uint64 JSON field: ${path.join(".")}`);
      }
      const record = current;
      const snakeCase = part.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
      const spellings = snakeCase === part ? [part] : [part, snakeCase];
      const present = spellings.filter((spelling) => spelling in record);
      if (present.length !== 1) {
        throw new TypeError(
          present.length > 1
            ? `uint64 JSON field uses multiple ProtoJSON spellings: ${path.join(".")}`
            : `missing uint64 JSON field: ${path.join(".")}`,
        );
      }
      current = record[present[0]!];
    }
    parseCanonicalUint64Json(current);
  }
}

export function parseMarketEventProtoJson(value: unknown): MarketEventEnvelopeV1 {
  validateUint64JsonPaths(value, [["generation"], ["sequence"]]);
  rejectDuplicateNumericEncodingAliases(value);
  const message = fromJson(MarketEventEnvelopeV1Schema, value as JsonValue);
  if (rawFrameSchemaId(message.source?.numericEncoding) !== undefined) {
    throw new TypeError("raw byte-frame encodings are not normalized market-event encodings");
  }
  return message;
}

export function parseDatasetManifestProtoJson(value: unknown): DatasetManifestV1 {
  validateUint64JsonPaths(value, [
    ["sourceTimestampMissingRows"],
    ["rowCount"],
    ["object", "sizeBytes"],
    ["object", "parquetFooterRows"],
  ]);
  rejectDuplicateNumericEncodingAliases(value);
  const message = fromJson(DatasetManifestV1Schema, value as JsonValue);
  const rawSchemaId = rawFrameSchemaId(message.source?.numericEncoding);
  const schemaSha256 = message.object?.parquetSchemaSha256;
  const rawSchemaIds = [
    "lqepoch.market_raw_frame.v1",
    "lqepoch.market_raw_json_frame.v1",
  ] as const;
  if (
    (rawSchemaId === undefined &&
      rawSchemaIds.some((schemaId) => schemaSha256 === trustedParquetSchemaSha256(schemaId))) ||
    (rawSchemaId !== undefined &&
      schemaSha256 !== trustedParquetSchemaSha256(rawSchemaId))
  ) {
    throw new TypeError(
      "raw byte-frame encodings must be bound to their registered Parquet schema",
    );
  }
  if (
    rawSchemaId !== undefined &&
    (message.timeRange !== undefined ||
      message.rowCount === 0n ||
      message.sourceTimestampMissingRows !== message.rowCount)
  ) {
    throw new TypeError(
      "raw-frame manifests require rows, no source-time range, and every frame timestamp missing",
    );
  }
  return message;
}

/** Parse a bounded DatasetManifestV2 ProtoJSON object and validate its structural bindings. */
export function parseDatasetManifestV2ProtoJson(value: unknown): DatasetManifestV2 {
  const inputSize = encodedJsonSize(value);
  if (inputSize > MAX_DATASET_MANIFEST_V2_JSON_BYTES) {
    throw new RangeError("dataset manifest v2 JSON exceeds the configured byte limit");
  }
  rejectDuplicateProtoFieldSpellings(value);
  const completion = getAliasedRecord(value, "completionEvidence", "completion_evidence");
  if (completion === undefined) throw new TypeError("dataset v2 requires completion evidence");
  const cases = ["finiteBatch", "providerWatermark", "diagnosticStream"] as const;
  const presentCases = cases.filter((name) => hasAliasedField(completion, name));
  if (presentCases.length !== 1) {
    throw new TypeError("completion evidence must contain exactly one oneof case");
  }
  const evidenceCase = presentCases[0]!;
  const paths: JsonPath[] = [
    ["sourceTimestampMissingRows"],
    ["rowCount"],
    ["object", "sizeBytes"],
    ["object", "parquetFooterRows"],
  ];
  if (evidenceCase === "finiteBatch") {
    paths.push(
      ["completionEvidence", "finiteBatch", "inputSizeBytes"],
      ["completionEvidence", "finiteBatch", "inputRecordCount"],
      ["completionEvidence", "finiteBatch", "consumedRecordCount"],
    );
    const finite = getAliasedRecord(completion, "finiteBatch", "finite_batch");
    if (finite !== undefined && hasAliasedField(finite, "pageCount")) {
      paths.push(["completionEvidence", "finiteBatch", "pageCount"]);
    }
  } else if (evidenceCase === "providerWatermark") {
    for (const name of [
      "generation",
      "firstSequence",
      "lastSequence",
      "sequenceCount",
      "allowedLatenessNs",
    ]) {
      paths.push(["completionEvidence", "providerWatermark", name]);
    }
  } else {
    paths.push(
      ["completionEvidence", "diagnosticStream", "generation"],
      ["completionEvidence", "diagnosticStream", "observedLastSequence"],
    );
  }
  validateUint64JsonPaths(value, paths);

  const message = fromJson(DatasetManifestV2Schema, value as JsonValue);
  validateDatasetManifestV2(message);
  return message;
}

/** Parse raw ProtoJSON text with a byte bound and duplicate-key rejection before JSON.parse. */
export function parseDatasetManifestV2Json(text: string): DatasetManifestV2 {
  if (new TextEncoder().encode(text).byteLength > MAX_DATASET_MANIFEST_V2_JSON_BYTES) {
    throw new RangeError("dataset manifest v2 JSON exceeds the configured byte limit");
  }
  rejectDuplicateJsonObjectKeys(text);
  return parseDatasetManifestV2ProtoJson(JSON.parse(text) as unknown);
}

function validateDatasetManifestV2(message: DatasetManifestV2): void {
  if (message.schemaVersion !== 2) throw new TypeError("unsupported dataset manifest v2 version");
  if (!validDatasetId(message.datasetId)) throw new TypeError("invalid dataset v2 identity");
  if (
    message.symbols.length === 0 ||
    message.symbols.length > MAX_DATASET_MANIFEST_V2_SYMBOLS ||
    !validSortedSymbols(message.symbols)
  ) {
    throw new TypeError("dataset v2 symbols must be valid, sorted, unique, and bounded");
  }
  const source = message.source;
  const object = message.object;
  const storage = message.storageVerification;
  const evidence = message.completionEvidence?.evidence;
  if (source === undefined || object === undefined || storage === undefined || evidence?.case === undefined) {
    throw new TypeError("dataset v2 source, object, storage proof, and completion evidence are required");
  }
  if (
    !validSourceIdentity(source.provider) ||
    !validSourceIdentity(source.feed) ||
    !["unknown", "authorized", "unauthorized"].includes(source.entitlement) ||
    source.numericEncoding === NumericEncodingV1.NUMERIC_ENCODING_UNSPECIFIED ||
    (source.sourceRecordId !== undefined && !validSourceIdentity(source.sourceRecordId))
  ) {
    throw new TypeError("invalid dataset v2 source identity");
  }
  const syntheticMentioned = source.provider.toLowerCase() === "synthetic" || source.feed.toLowerCase() === "synthetic";
  if (
    syntheticMentioned &&
    (source.provider !== "synthetic" || source.feed !== "synthetic" ||
      source.numericEncoding !== NumericEncodingV1.NUMERIC_ENCODING_DECIMAL_TOKEN)
  ) {
    throw new TypeError("synthetic source identity is inconsistent");
  }

  if (message.rowCount === 0n || message.sourceTimestampMissingRows > message.rowCount) {
    throw new TypeError("dataset v2 row counts are inconsistent");
  }
  if (object.parquetFooterRows !== message.rowCount) throw new TypeError("Parquet footer row count differs");
  if (
    !validObjectName(object.objectName) ||
    object.sizeBytes === 0n ||
    !SHA256.test(object.contentSha256) ||
    !SHA256.test(object.parquetSchemaSha256)
  ) {
    throw new TypeError("invalid dataset v2 object");
  }
  if (
    (object.transport !== "local_test" && object.transport !== "rclone_google_drive") ||
    object.objectId === undefined ||
    !validObjectId(object.objectId, object.transport === "local_test")
  ) {
    throw new TypeError("invalid dataset v2 object identity or transport");
  }
  if (
    !storage.verifiedBeforePublish ||
    !SHA256.test(storage.readbackSha256) ||
    storage.readbackSha256 !== object.contentSha256
  ) {
    throw new TypeError("dataset v2 object readback verification is inconsistent");
  }

  const rawSchema = rawFrameSchemaId(source.numericEncoding);
  const rawSchemaIds = ["lqepoch.market_raw_frame.v1", "lqepoch.market_raw_json_frame.v1"] as const;
  if (
    (rawSchema === undefined && rawSchemaIds.some((id) => object.parquetSchemaSha256 === trustedParquetSchemaSha256(id))) ||
    (rawSchema !== undefined && object.parquetSchemaSha256 !== trustedParquetSchemaSha256(rawSchema))
  ) {
    throw new TypeError("raw byte-frame source must match its trusted schema fingerprint");
  }
  const hasTimeRange = message.timeRange !== undefined;
  if (rawSchema !== undefined) {
    if (hasTimeRange || message.sourceTimestampMissingRows !== message.rowCount) {
      throw new TypeError("raw-frame manifests require absent source time and all timestamps missing");
    }
  } else if (
    (message.sourceTimestampMissingRows === message.rowCount) === hasTimeRange
  ) {
    throw new TypeError("dataset v2 time range must match source timestamp coverage");
  }
  if (message.timeRange !== undefined) {
    const start = message.timeRange.startInclusive;
    const end = message.timeRange.endExclusive;
    if (start === undefined || end === undefined || compareTimestamp(start, end) >= 0) {
      throw new TypeError("dataset v2 time range must contain both increasing bounds");
    }
  }

  if (evidence.case === "finiteBatch") {
    validateFiniteBatchV2(evidence.value, source.provider, source.feed, message.timeRange);
  } else if (evidence.case === "providerWatermark") {
    const value = evidence.value;
    if (
      value.provider !== source.provider ||
      value.feed !== source.feed ||
      !validDatasetId(value.subscriptionInstanceId) ||
      value.generation === 0n ||
      value.firstSequence === 0n ||
      value.lastSequence < value.firstSequence ||
      value.lastSequence - value.firstSequence + 1n !== value.sequenceCount ||
      value.sequenceCount === 0n ||
      !SHA256.test(value.continuityReceiptSha256) ||
      value.allowedLatenessNs > MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS ||
      !SHA256.test(value.reviewedPolicySha256) ||
      !SHA256.test(value.sourceReceiptSha256) ||
      message.sourceTimestampMissingRows !== 0n ||
      message.timeRange === undefined ||
      value.completeUpToExclusive === undefined ||
      compareTimestamp(message.timeRange.endExclusive!, value.completeUpToExclusive) > 0 ||
      source.provider === "synthetic" ||
      source.feed === "synthetic"
    ) {
      throw new TypeError("provider watermark evidence is structurally inconsistent");
    }
  } else {
    const value = evidence.value;
    if (
      !validDatasetId(value.sourceInstanceId) ||
      value.generation === 0n ||
      value.observedLastSequence === 0n ||
      value.localPolicyCutoff === undefined ||
      !SHA256.test(value.diagnosticPolicySha256) ||
      !SHA256.test(value.diagnosticReceiptSha256)
    ) {
      throw new TypeError("diagnostic stream evidence is structurally inconsistent");
    }
  }
}

function validateFiniteBatchV2(
  value: FiniteBatchCompletionV2,
  provider: string,
  feed: string,
  timeRange: DatasetTimeRangeV2 | undefined,
): void {
  if (
    value.sourceKind === FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_UNSPECIFIED ||
    !validDatasetId(value.inputIdentity) ||
    !SHA256.test(value.inputSha256) ||
    value.inputSizeBytes === 0n ||
    value.inputRecordCount === 0n ||
    value.inputRecordCount !== value.consumedRecordCount ||
    !SHA256.test(value.reviewedPolicySha256) ||
    !SHA256.test(value.sealReceiptSha256) ||
    value.dataCutoffExclusive === undefined ||
    value.sealedAt === undefined ||
    value.completedAt === undefined ||
    compareTimestamp(value.dataCutoffExclusive, value.sealedAt) > 0 ||
    compareTimestamp(value.sealedAt, value.completedAt) > 0 ||
    (timeRange?.endExclusive !== undefined &&
      compareTimestamp(timeRange.endExclusive, value.dataCutoffExclusive) > 0)
  ) {
    throw new TypeError("finite batch receipt is incomplete or inconsistent");
  }
  const paged = value.sourceKind === FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_HISTORICAL_PAGED;
  if (paged) {
    if (
      value.pageCount === undefined ||
      value.pageCount === 0n ||
      value.pagesExhausted !== true ||
      value.pageSetSha256 === undefined ||
      !SHA256.test(value.pageSetSha256)
    ) {
      throw new TypeError("paged finite input requires page count, exhaustion, and set hash");
    }
  } else if (
    value.pageCount !== undefined || value.pagesExhausted !== undefined || value.pageSetSha256 !== undefined
  ) {
    throw new TypeError("non-paged finite input must omit page evidence");
  }
  const synthetic = provider === "synthetic" && feed === "synthetic";
  if (
    (value.sourceKind === FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_SYNTHETIC_REPLAY && !synthetic) ||
    ((value.sourceKind === FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_HISTORICAL_PAGED ||
      value.sourceKind === FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_HISTORICAL_NON_PAGED) && synthetic)
  ) {
    throw new TypeError("finite input kind does not match source identity");
  }
}

function compareTimestamp(
  left: { readonly seconds: bigint; readonly nanos: number },
  right: { readonly seconds: bigint; readonly nanos: number },
): number {
  if (left.seconds !== right.seconds) return left.seconds < right.seconds ? -1 : 1;
  return left.nanos === right.nanos ? 0 : left.nanos < right.nanos ? -1 : 1;
}

function validDatasetId(value: string): boolean {
  if (!/^[A-Za-z0-9][A-Za-z0-9._:-]{0,255}$/.test(value) || value.includes("..")) return false;
  return !value.split(/[._:-]/).some((part) => part.toLowerCase() === "latest" || part.toLowerCase() === "fallback");
}

function validSourceIdentity(value: string): boolean {
  return value.length > 0 && new TextEncoder().encode(value).byteLength <= 128 &&
    value.trim() === value && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function validSortedSymbols(values: readonly string[]): boolean {
  return values.every((value) =>
    value.length > 0 && new TextEncoder().encode(value).byteLength <= 256 &&
    value.trim() === value && !/[\u0000-\u001f\u007f-\u009f]/u.test(value)
  ) && values.every((value, index) => index === 0 || compareUtf8(values[index - 1]!, value) < 0);
}

function compareUtf8(left: string, right: string): number {
  const leftBytes = new TextEncoder().encode(left);
  const rightBytes = new TextEncoder().encode(right);
  const commonLength = Math.min(leftBytes.length, rightBytes.length);
  for (let index = 0; index < commonLength; index += 1) {
    if (leftBytes[index] !== rightBytes[index]) return leftBytes[index]! - rightBytes[index]!;
  }
  return leftBytes.length - rightBytes.length;
}

function validObjectName(value: string): boolean {
  return /^[A-Za-z0-9_.-]{1,512}$/.test(value) && value !== "." && value !== "..";
}

function validObjectId(value: string, localTest: boolean): boolean {
  if (value.length === 0 || new TextEncoder().encode(value).byteLength > 512 ||
    value.trim() !== value || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) return false;
  if (!localTest) return !value.startsWith("local-test:");
  if (!value.startsWith("local-test:")) return false;
  const tail = value.slice("local-test:".length);
  return tail.length > 0 && !tail.includes("..") && /^[A-Za-z0-9_.:-]+$/.test(tail);
}

function getAliasedRecord(value: unknown, camel: string, snake: string): Record<string, unknown> | undefined {
  if (!isRecord(value)) return undefined;
  const field = value[camel] ?? value[snake];
  return isRecord(field) ? field : undefined;
}

function hasAliasedField(value: Record<string, unknown>, camel: string): boolean {
  const snake = camel.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
  return camel in value || snake in value;
}

function rejectDuplicateProtoFieldSpellings(value: unknown, depth = 0): void {
  if (depth > 64) throw new RangeError("dataset manifest v2 JSON nesting exceeds its bound");
  if (Array.isArray(value)) {
    for (const nested of value) rejectDuplicateProtoFieldSpellings(nested, depth + 1);
    return;
  }
  if (!isRecord(value)) return;
  const canonical = new Set<string>();
  for (const [key, nested] of Object.entries(value)) {
    const normalized = key.replace(/_([a-z])/g, (_match, letter: string) => letter.toUpperCase());
    if (canonical.has(normalized)) {
      throw new TypeError("ProtoJSON field must not use both camelCase and snake_case spellings");
    }
    canonical.add(normalized);
    rejectDuplicateProtoFieldSpellings(nested, depth + 1);
  }
}

function encodedJsonSize(value: unknown): number {
  let encoded: string | undefined;
  try {
    encoded = JSON.stringify(value);
  } catch {
    throw new TypeError("dataset manifest v2 must contain JSON-compatible values");
  }
  if (encoded === undefined) throw new TypeError("dataset manifest v2 must be a JSON object");
  return new TextEncoder().encode(encoded).byteLength;
}

function rejectDuplicateJsonObjectKeys(text: string): void {
  let offset = 0;
  const skipWhitespace = (): void => {
    while (offset < text.length && /\s/.test(text[offset]!)) offset += 1;
  };
  const parseString = (): string => {
    const start = offset;
    if (text[offset] !== '"') throw new SyntaxError("expected JSON string");
    offset += 1;
    while (offset < text.length) {
      const character = text[offset]!;
      offset += 1;
      if (character === '"') return JSON.parse(text.slice(start, offset)) as string;
      if (character === "\\") offset += 1;
    }
    throw new SyntaxError("unterminated JSON string");
  };
  const parseValue = (depth: number): void => {
    if (depth > 64) throw new RangeError("dataset manifest v2 JSON nesting exceeds its bound");
    skipWhitespace();
    const token = text[offset];
    if (token === '"') {
      parseString();
      return;
    }
    if (token === "{") {
      offset += 1;
      skipWhitespace();
      const keys = new Set<string>();
      if (text[offset] === "}") { offset += 1; return; }
      while (offset < text.length) {
        skipWhitespace();
        const key = parseString();
        if (keys.has(key)) throw new SyntaxError(`duplicate JSON object key: ${key}`);
        keys.add(key);
        skipWhitespace();
        if (text[offset] !== ":") throw new SyntaxError("expected JSON colon");
        offset += 1;
        parseValue(depth + 1);
        skipWhitespace();
        if (text[offset] === "}") { offset += 1; return; }
        if (text[offset] !== ",") throw new SyntaxError("expected JSON comma");
        offset += 1;
      }
      throw new SyntaxError("unterminated JSON object");
    }
    if (token === "[") {
      offset += 1;
      skipWhitespace();
      if (text[offset] === "]") { offset += 1; return; }
      while (offset < text.length) {
        parseValue(depth + 1);
        skipWhitespace();
        if (text[offset] === "]") { offset += 1; return; }
        if (text[offset] !== ",") throw new SyntaxError("expected JSON comma");
        offset += 1;
      }
      throw new SyntaxError("unterminated JSON array");
    }
    const start = offset;
    while (offset < text.length && !/[\s,\]}]/.test(text[offset]!)) offset += 1;
    if (offset === start) throw new SyntaxError("invalid JSON token");
  };
  parseValue(0);
  skipWhitespace();
  if (offset !== text.length) throw new SyntaxError("trailing JSON content");
}

export function parsePredictionEnvelopeProtoJson(value: unknown): PredictionEnvelopeV1 {
  validateUint64JsonPaths(value, [["forecast", "sequence"]]);
  rejectDuplicateNumericEncodingAliases(value);
  const message = fromJson(PredictionEnvelopeV1Schema, value as JsonValue);
  if (rawFrameSchemaId(message.source?.numericEncoding) !== undefined) {
    throw new TypeError("raw byte-frame encodings cannot identify a normalized prediction source");
  }
  return message;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getRecordField(value: unknown, key: string): Record<string, unknown> | undefined {
  if (!isRecord(value)) return undefined;
  const field = value[key];
  return isRecord(field) ? field : undefined;
}

function rawFrameSchemaId(encoding: NumericEncodingV1 | undefined): string | undefined {
  switch (encoding) {
    case NumericEncodingV1.NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES:
      return "lqepoch.market_raw_frame.v1";
    case NumericEncodingV1.NUMERIC_ENCODING_RAW_JSON_BYTES:
      return "lqepoch.market_raw_json_frame.v1";
    default:
      return undefined;
  }
}

function rejectDuplicateNumericEncodingAliases(value: unknown): void {
  const source = getRecordField(value, "source");
  if (source !== undefined && "numericEncoding" in source && "numeric_encoding" in source) {
    throw new TypeError("source numeric encoding must not use both ProtoJSON field spellings");
  }
}
