import { createHash } from "node:crypto";
import { create, fromJson, toJsonString, type JsonValue } from "@bufbuild/protobuf";
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
  EngineStatusResponseV1Schema,
  SyntheticOfflinePreviewV1Schema,
  type EngineStatusResponseV1,
  type SyntheticOfflinePreviewV1,
} from "../gen/lqepoch/engine/v1/offline_preview_pb.js";
import {
  DatasetManifestV2Schema,
  DatasetCompletionEvidenceV2Schema,
  FiniteBatchSealReceiptV2Schema,
  FiniteBatchSourceKindV2,
  type FiniteBatchCompletionV2,
  type DatasetManifestV2,
  type DatasetTimeRangeV2,
} from "../gen/lqepoch/dataset/v2/manifest_pb.js";
import {
  UsEquityTradeBarV2Schema,
  type UsEquityTradeBarV2,
} from "../gen/lqepoch/market/v2/trade_bar_pb.js";
import { trustedParquetSchemaSha256 } from "./schema-fingerprint.js";
import { hasOnlyUnicodeScalars } from "./raw-frame.js";

const UINT64_MAX = 18_446_744_073_709_551_615n;
const PROTO_TIMESTAMP_MIN_SECONDS = -62_135_596_800n;
const PROTO_TIMESTAMP_MAX_SECONDS = 253_402_300_799n;
const CANONICAL_UINT64 = /^(0|[1-9][0-9]*)$/;
const MAX_PROTOJSON_TEXT_BYTES = 2 * 1024 * 1024;
const MAX_ENGINE_UNKNOWN_SAMPLE = 256;
const MAX_DATASET_MANIFEST_V2_SYMBOLS = 4096;
const MAX_PROVIDER_WATERMARK_ALLOWED_LATENESS_NS = 60_000_000_000n;
const SHA256 = /^[0-9a-f]{64}$/;
const EXACT_DECIMAL = /^(-?)(0|[1-9][0-9]*)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?$/;
const PROTO_TIMESTAMP_V2 = /^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]{1,9}))?(?:Z|([+-])([0-9]{2}):([0-9]{2}))$/;
const PROTO_TIMESTAMP_V2_FIELDS = new Set([
  "startInclusive", "start_inclusive", "endExclusive", "end_exclusive",
  "dataCutoffExclusive", "data_cutoff_exclusive", "sealedAt", "sealed_at",
  "completedAt", "completed_at", "completeUpToExclusive", "complete_up_to_exclusive",
  "localPolicyCutoff", "local_policy_cutoff", "observedMaxSourceTimestamp",
  "observed_max_source_timestamp",
  "barStartUtc", "bar_start_utc", "barEndExclusiveUtc", "bar_end_exclusive_utc",
  "availableAtUtc", "available_at_utc", "sessionStartUtc", "session_start_utc",
  "sessionEndExclusiveUtc", "session_end_exclusive_utc", "windowStartUtc", "window_start_utc",
  "windowEndExclusiveUtc", "window_end_exclusive_utc", "sourceStartUtc", "source_start_utc",
  "sourceEndExclusiveUtc", "source_end_exclusive_utc",
  "createdAt", "created_at", "dataCutoff", "data_cutoff", "eventTime", "event_time",
  "knowledgeAt", "knowledge_at", "availableAt", "available_at", "decisionAt", "decision_at",
  "validFrom", "valid_from", "validUntil", "valid_until",
]);
const NUMERIC_ENCODING_V2_NAMES = new Set([
  "NUMERIC_ENCODING_DECIMAL_TOKEN",
  "NUMERIC_ENCODING_INTEGER_TOKEN",
  "NUMERIC_ENCODING_BINARY_FLOAT64_SHORTEST_DECIMAL",
  "NUMERIC_ENCODING_BINARY_FLOAT32_SHORTEST_DECIMAL",
  "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES",
  "NUMERIC_ENCODING_RAW_JSON_BYTES",
]);
const FINITE_SOURCE_KIND_V2_NAMES = new Set([
  "FINITE_BATCH_SOURCE_KIND_SYNTHETIC_REPLAY",
  "FINITE_BATCH_SOURCE_KIND_HISTORICAL_PAGED",
  "FINITE_BATCH_SOURCE_KIND_HISTORICAL_NON_PAGED",
  "FINITE_BATCH_SOURCE_KIND_LOCAL_ARCHIVE",
]);

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
  if (rawFrameSchemaIds(message.source?.numericEncoding) !== undefined) {
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
  const rawSchemaIdsForEncoding = rawFrameSchemaIds(message.source?.numericEncoding);
  const schemaSha256 = message.object?.parquetSchemaSha256;
  const rawSchemaIds = [
    "lqepoch.market_raw_frame.v1",
    "lqepoch.market_raw_frame.v2",
    "lqepoch.market_raw_json_frame.v1",
    "lqepoch.market_raw_json_frame.v2",
  ] as const;
  if (
    (rawSchemaIdsForEncoding === undefined &&
      rawSchemaIds.some((schemaId) => schemaSha256 === trustedParquetSchemaSha256(schemaId))) ||
    (rawSchemaIdsForEncoding !== undefined &&
      !rawSchemaIdsForEncoding.some(
        (schemaId) => schemaSha256 === trustedParquetSchemaSha256(schemaId),
      ))
  ) {
    throw new TypeError(
      "raw byte-frame encodings must be bound to their registered Parquet schema",
    );
  }
  if (
    rawSchemaIdsForEncoding !== undefined &&
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
  if (inputSize > MAX_PROTOJSON_TEXT_BYTES) {
    throw new RangeError("dataset manifest v2 JSON exceeds the configured byte limit");
  }
  rejectDuplicateProtoFieldSpellings(value);
  validateProtoTimestampV2Fields(value);
  validateDatasetManifestV2EnumNames(value);
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
  assertBoundedProtoJsonText(text);
  rejectDuplicateJsonObjectKeys(text);
  return parseDatasetManifestV2ProtoJson(JSON.parse(text) as unknown);
}

/** Return bounded compact ProtoJSON bytes for the fully validated manifest. */
export function datasetManifestV2ProtojsonBytes(value: DatasetManifestV2): Uint8Array {
  validateDatasetManifestV2(value);
  const bytes = new TextEncoder().encode(
    toJsonString(DatasetManifestV2Schema, value, { alwaysEmitImplicit: true }),
  );
  if (bytes.byteLength > MAX_PROTOJSON_TEXT_BYTES) {
    throw new RangeError("dataset manifest v2 JSON exceeds the configured byte limit");
  }
  return bytes;
}

/** Return the shared compact ProtoJSON bytes hashed by `sealReceiptSha256`, without a final LF. */
export function finiteBatchSealReceiptProtojsonBytes(
  value: FiniteBatchCompletionV2,
): Uint8Array {
  validateFiniteBatchReceiptProjectionFields(value);
  const projection = create(FiniteBatchSealReceiptV2Schema, {
    sourceKind: value.sourceKind,
    inputIdentity: value.inputIdentity,
    inputSha256: value.inputSha256,
    inputSizeBytes: value.inputSizeBytes,
    inputRecordCount: value.inputRecordCount,
    consumedRecordCount: value.consumedRecordCount,
    reviewedPolicySha256: value.reviewedPolicySha256,
    dataCutoffExclusive: value.dataCutoffExclusive,
    sealedAt: value.sealedAt,
    completedAt: value.completedAt,
    pageCount: value.pageCount,
    pagesExhausted: value.pagesExhausted,
    pageSetSha256: value.pageSetSha256,
  });
  const bytes = new TextEncoder().encode(toJsonString(FiniteBatchSealReceiptV2Schema, projection));
  if (bytes.byteLength > 16 * 1024) {
    throw new RangeError("finite batch seal receipt exceeds the configured byte limit");
  }
  return bytes;
}

/** Return lowercase SHA-256 over the shared finite-batch receipt projection. */
export function finiteBatchSealReceiptSha256(value: FiniteBatchCompletionV2): string {
  return createHash("sha256").update(finiteBatchSealReceiptProtojsonBytes(value)).digest("hex");
}

/** Return bounded compact ProtoJSON bytes for the validated manifest's oneof completion evidence. */
export function datasetCompletionEvidenceV2ProtojsonBytes(
  manifest: DatasetManifestV2,
): Uint8Array {
  validateDatasetManifestV2(manifest);
  const evidence = manifest.completionEvidence;
  if (evidence === undefined) throw new TypeError("dataset completion evidence is required");
  const bytes = new TextEncoder().encode(
    toJsonString(DatasetCompletionEvidenceV2Schema, evidence, { alwaysEmitImplicit: true }),
  );
  if (bytes.byteLength > 16 * 1024) {
    throw new RangeError("dataset completion evidence exceeds the configured byte limit");
  }
  return bytes;
}

/** Return lowercase SHA-256 of the validated manifest completion oneof projection. */
export function datasetCompletionEvidenceV2Sha256(manifest: DatasetManifestV2): string {
  return createHash("sha256").update(datasetCompletionEvidenceV2ProtojsonBytes(manifest)).digest("hex");
}

/** Parse a lossless BarV2 ProtoJSON row, preserving all uint64 and nanosecond values. */
export function parseUsEquityTradeBarV2ProtoJson(value: unknown): UsEquityTradeBarV2 {
  requireProtoJsonFields(value, [
    "schemaVersion", "sourceProvider", "sourceFeed", "sourceEntitlement",
    "sourceNumericEncoding", "symbol", "barStartUtc", "barEndExclusiveUtc",
    "availableAtUtc", "tradeDate", "sessionId", "sessionTimezone", "sessionPolicyId",
    "sessionPolicySha256", "sessionStartUtc", "sessionEndExclusiveUtc", "windowStartUtc",
    "windowEndExclusiveUtc", "open", "high", "low", "close", "volume", "tradeCount",
    "quoteEventsExcluded", "sourceTimestampMissingRows", "sequenceGapCount", "lateEventCount",
    "windowExpectedMinutes", "windowEmptyTradeMinutes", "sourceStartUtc", "sourceEndExclusiveUtc",
    "windowInputEof", "completionMode", "nbboInputStatus", "completionEvidenceSha256",
  ], ["sourcePagesExhausted"]);
  const encoded = JSON.stringify(value);
  if (encoded === undefined || new TextEncoder().encode(encoded).byteLength > 64 * 1024) {
    throw new RangeError("BarV2 JSON exceeds the configured byte limit");
  }
  validateProtoTimestampV2Fields(value);
  validateUint64JsonPaths(value, [
    ["tradeCount"], ["quoteEventsExcluded"], ["sourceTimestampMissingRows"],
    ["sequenceGapCount"], ["lateEventCount"], ["windowExpectedMinutes"],
    ["windowEmptyTradeMinutes"],
  ]);
  if (!isRecord(value)) throw new TypeError("BarV2 JSON must be an object");
  const version = getAliasedValue(value, "schemaVersion", "schema_version");
  if (typeof version !== "number" || !Number.isInteger(version) || version !== 2) {
    throw new TypeError("BarV2 schema_version must be the integer 2");
  }
  const row = fromJson(UsEquityTradeBarV2Schema, value as JsonValue);
  if (
    row.schemaVersion !== 2 ||
    !SHA256.test(row.completionEvidenceSha256) ||
    !validMarketSymbol(row.symbol) ||
    !["unknown", "authorized", "unauthorized"].includes(row.sourceEntitlement) ||
    ![
      "decimal_token", "integer_token", "binary_float64_shortest_decimal",
      "binary_float32_shortest_decimal",
    ].includes(row.sourceNumericEncoding)
  ) {
    throw new TypeError("invalid BarV2 version or completion evidence digest");
  }
  validateUsEquityTradeBarV2Shape(row);
  return row;
}

/** Require the BarV2 row reference to match the validated manifest completion oneof. */
export function validateBarV2CompletionEvidenceReference(
  row: UsEquityTradeBarV2,
  manifest: DatasetManifestV2,
): void {
  if (row.schemaVersion !== 2 || row.completionEvidenceSha256 !== datasetCompletionEvidenceV2Sha256(manifest)) {
    throw new TypeError("BarV2 completion evidence digest does not match the dataset manifest");
  }
}

/** Validate BarV2 row semantics against the manifest that owns source and completion evidence. */
export function validateUsEquityTradeBarV2AgainstManifest(
  row: UsEquityTradeBarV2,
  manifest: DatasetManifestV2,
): void {
  validateUsEquityTradeBarV2Shape(row);
  validateDatasetManifestV2(manifest);
  const source = manifest.source;
  if (source === undefined) throw new TypeError("dataset manifest source is required");
  const encoding = numericEncodingName(source.numericEncoding);
  if (
    row.sourceProvider !== source.provider ||
    row.sourceFeed !== source.feed ||
    row.sourceEntitlement !== source.entitlement ||
    row.sourceNumericEncoding !== encoding ||
    !manifest.symbols.includes(row.symbol) ||
    row.sourceTimestampMissingRows !== manifest.sourceTimestampMissingRows
  ) {
    throw new TypeError("BarV2 source fields do not match the dataset manifest");
  }

  const completion = manifest.completionEvidence?.evidence;
  let expectedMode: string;
  let expectedEof: boolean;
  let expectedPages: boolean | undefined;
  switch (completion?.case) {
    case "finiteBatch":
      expectedMode = "finite_batch";
      expectedEof = true;
      expectedPages = completion.value.sourceKind ===
        FiniteBatchSourceKindV2.FINITE_BATCH_SOURCE_KIND_HISTORICAL_PAGED ? true : undefined;
      break;
    case "providerWatermark":
      expectedMode = "provider_watermark";
      expectedEof = false;
      expectedPages = undefined;
      break;
    case "diagnosticStream":
      expectedMode = "diagnostic_stream";
      expectedEof = false;
      expectedPages = undefined;
      break;
    default:
      throw new TypeError("dataset manifest has no completion evidence oneof");
  }
  if (
    row.completionMode !== expectedMode ||
    row.windowInputEof !== expectedEof ||
    (row.sourcePagesExhausted !== undefined) !== (expectedPages !== undefined) ||
    (expectedPages !== undefined && row.sourcePagesExhausted !== expectedPages)
  ) {
    throw new TypeError("BarV2 completion fields do not match the dataset manifest");
  }

  validateBarV2CompletionEvidenceReference(row, manifest);
  const range = manifest.timeRange;
  if (
    range?.startInclusive === undefined ||
    range.endExclusive === undefined ||
    compareTimestamp(row.sourceStartUtc, range.startInclusive) < 0 ||
    compareTimestamp(row.sourceEndExclusiveUtc, range.endExclusive) > 0
  ) {
    throw new TypeError("BarV2 source bounds exceed or lack the manifest time range");
  }
}

function validateUsEquityTradeBarV2Shape(row: UsEquityTradeBarV2): void {
  if (
    row.schemaVersion !== 2 ||
    !validIsoDate(row.tradeDate) ||
    !validSourceIdentity(row.sourceProvider) ||
    !validSourceIdentity(row.sourceFeed) ||
    !["unknown", "authorized", "unauthorized"].includes(row.sourceEntitlement) ||
    ![
      "decimal_token", "integer_token", "binary_float64_shortest_decimal",
      "binary_float32_shortest_decimal",
    ].includes(row.sourceNumericEncoding) ||
    !validMarketSymbol(row.symbol) ||
    !validCoreIdentity(row.sessionId, 128) ||
    !validSourceIdentity(row.sessionTimezone) ||
    !validCoreIdentity(row.sessionPolicyId, 256) ||
    !SHA256.test(row.sessionPolicySha256) ||
    !["finite_batch", "provider_watermark", "diagnostic_stream"].includes(row.completionMode) ||
    !validBoundedText(row.nbboInputStatus, 128) ||
    row.tradeCount === 0n ||
    row.windowExpectedMinutes === 0n ||
    row.windowEmptyTradeMinutes > row.windowExpectedMinutes ||
    row.sourcePagesExhausted === false
  ) {
    throw new TypeError("BarV2 identity, count, or completion fields are invalid");
  }

  const open = parseExactDecimal(row.open);
  const high = parseExactDecimal(row.high);
  const low = parseExactDecimal(row.low);
  const close = parseExactDecimal(row.close);
  const volume = parseExactDecimal(row.volume);
  if (
    open.coefficient <= 0n || high.coefficient <= 0n || low.coefficient <= 0n ||
    close.coefficient <= 0n || volume.coefficient < 0n ||
    compareDecimal(high, open) < 0 || compareDecimal(high, close) < 0 ||
    compareDecimal(high, low) < 0 || compareDecimal(low, open) > 0 ||
    compareDecimal(low, close) > 0
  ) {
    throw new TypeError("BarV2 exact OHLCV values are inconsistent");
  }

  const timestamps = [
    row.barStartUtc, row.barEndExclusiveUtc, row.availableAtUtc, row.sessionStartUtc,
    row.sessionEndExclusiveUtc, row.windowStartUtc, row.windowEndExclusiveUtc,
    row.sourceStartUtc, row.sourceEndExclusiveUtc,
  ];
  if (timestamps.some((value) => value === undefined || !validTimestampValue(value))) {
    throw new TypeError("BarV2 timestamp fields are required");
  }
  const [barStart, barEnd, availableAt, sessionStart, sessionEnd, windowStart, windowEnd, sourceStart, sourceEnd] =
    timestamps as NonNullable<(typeof timestamps)[number]>[];
  if (
    compareTimestamp(sessionStart, sessionEnd) >= 0 ||
    compareTimestamp(windowStart, windowEnd) >= 0 ||
    compareTimestamp(windowStart, sessionStart) < 0 ||
    compareTimestamp(windowEnd, sessionEnd) > 0 ||
    timestampNanoseconds(barEnd) - timestampNanoseconds(barStart) !== 60_000_000_000n ||
    compareTimestamp(barStart, windowStart) < 0 ||
    compareTimestamp(barEnd, windowEnd) > 0 ||
    compareTimestamp(barStart, sessionStart) < 0 ||
    compareTimestamp(barEnd, sessionEnd) > 0 ||
    compareTimestamp(availableAt, barEnd) < 0 ||
    compareTimestamp(sourceStart, barStart) < 0 ||
    compareTimestamp(sourceEnd, barEnd) > 0 ||
    compareTimestamp(sourceStart, sourceEnd) >= 0
  ) {
    throw new TypeError("BarV2 timestamps are inconsistent");
  }
}

function parseExactDecimal(value: string): { coefficient: bigint; scale: number } {
  if (value.length > 128 || !/^[\x00-\x7f]*$/.test(value)) {
    throw new TypeError("BarV2 decimal exceeds the exact-decimal input bound");
  }
  const match = EXACT_DECIMAL.exec(value);
  if (match === null) throw new TypeError("BarV2 decimal is not a JSON number");
  const [, sign, integer, fraction = "", exponentText = "0"] = match;
  const exponent = Number(exponentText);
  if (!Number.isInteger(exponent) || Math.abs(exponent) > 38) {
    throw new TypeError("BarV2 decimal exponent is outside the exact-decimal bound");
  }
  let digits = `${integer}${fraction}`.replace(/^0+/, "");
  if (digits.length === 0) return { coefficient: 0n, scale: 0 };
  let scale = fraction.length - exponent;
  if (scale < 0) {
    digits += "0".repeat(-scale);
    if (digits.length > 128) throw new TypeError("BarV2 decimal coefficient exceeds its bound");
    scale = 0;
  }
  while (scale > 0 && digits.endsWith("0")) {
    digits = digits.slice(0, -1);
    scale -= 1;
  }
  if (scale > 28) throw new TypeError("BarV2 decimal scale exceeds its bound");
  const magnitude = BigInt(digits);
  const signed = sign === "-" ? -magnitude : magnitude;
  if (signed < -(1n << 127n) || signed > (1n << 127n) - 1n) {
    throw new TypeError("BarV2 decimal coefficient overflows signed 128-bit");
  }
  return { coefficient: signed, scale };
}

function compareDecimal(left: { coefficient: bigint; scale: number }, right: { coefficient: bigint; scale: number }): number {
  const scale = Math.max(left.scale, right.scale);
  const leftValue = left.coefficient * 10n ** BigInt(scale - left.scale);
  const rightValue = right.coefficient * 10n ** BigInt(scale - right.scale);
  return leftValue === rightValue ? 0 : leftValue < rightValue ? -1 : 1;
}

function timestampNanoseconds(value: { readonly seconds: bigint; readonly nanos: number }): bigint {
  return value.seconds * 1_000_000_000n + BigInt(value.nanos);
}

function numericEncodingName(value: NumericEncodingV1): string {
  switch (value) {
    case NumericEncodingV1.NUMERIC_ENCODING_DECIMAL_TOKEN: return "decimal_token";
    case NumericEncodingV1.NUMERIC_ENCODING_INTEGER_TOKEN: return "integer_token";
    case NumericEncodingV1.NUMERIC_ENCODING_BINARY_FLOAT64_SHORTEST_DECIMAL: return "binary_float64_shortest_decimal";
    case NumericEncodingV1.NUMERIC_ENCODING_BINARY_FLOAT32_SHORTEST_DECIMAL: return "binary_float32_shortest_decimal";
    default: return "";
  }
}

function validCoreIdentity(value: string, maxBytes: number): boolean {
  return new TextEncoder().encode(value).byteLength <= maxBytes &&
    /^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(value) &&
    !value.includes("..") &&
    !value.split(/[._:-]/).some((part) => part.toLowerCase() === "latest" || part.toLowerCase() === "fallback");
}

function validBoundedText(value: string, maxBytes: number): boolean {
  return value.length > 0 && hasOnlyUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= maxBytes &&
    value.trim() === value && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
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

  const rawSchemaIdsForEncoding = rawFrameSchemaIds(source.numericEncoding);
  const rawSchemaIds = [
    "lqepoch.market_raw_frame.v1",
    "lqepoch.market_raw_frame.v2",
    "lqepoch.market_raw_json_frame.v1",
    "lqepoch.market_raw_json_frame.v2",
  ] as const;
  if (
    (rawSchemaIdsForEncoding === undefined && rawSchemaIds.some((id) => object.parquetSchemaSha256 === trustedParquetSchemaSha256(id))) ||
    (rawSchemaIdsForEncoding !== undefined && !rawSchemaIdsForEncoding.some((id) => object.parquetSchemaSha256 === trustedParquetSchemaSha256(id)))
  ) {
    throw new TypeError("raw byte-frame source must match its trusted schema fingerprint");
  }
  const hasTimeRange = message.timeRange !== undefined;
  if (rawSchemaIdsForEncoding !== undefined) {
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
  const receiptSha256 = finiteBatchSealReceiptSha256(value);
  if (
    !SHA256.test(value.sealReceiptSha256) ||
    value.sealReceiptSha256 !== receiptSha256 ||
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

function validateFiniteBatchReceiptProjectionFields(value: FiniteBatchCompletionV2): void {
  if (
    ![1, 2, 3, 4].includes(value.sourceKind) ||
    !validDatasetId(value.inputIdentity) ||
    !SHA256.test(value.inputSha256) ||
    value.inputSizeBytes === 0n ||
    value.inputRecordCount === 0n ||
    value.inputRecordCount !== value.consumedRecordCount ||
    !SHA256.test(value.reviewedPolicySha256) ||
    value.dataCutoffExclusive === undefined ||
    value.sealedAt === undefined ||
    value.completedAt === undefined ||
    compareTimestamp(value.dataCutoffExclusive, value.sealedAt) > 0 ||
    compareTimestamp(value.sealedAt, value.completedAt) > 0
  ) {
    throw new TypeError("finite batch receipt projection fields are incomplete or inconsistent");
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
      throw new TypeError("paged finite batch receipt projection is incomplete");
    }
  } else if (
    value.pageCount !== undefined ||
    value.pagesExhausted !== undefined ||
    value.pageSetSha256 !== undefined
  ) {
    throw new TypeError("non-paged finite batch receipt projection must omit page evidence");
  }
}

function compareTimestamp(
  left: { readonly seconds: bigint; readonly nanos: number } | undefined,
  right: { readonly seconds: bigint; readonly nanos: number } | undefined,
): number {
  if (!validTimestampValue(left) || !validTimestampValue(right)) {
    throw new TypeError("protobuf timestamps are outside the supported range");
  }
  if (left.seconds !== right.seconds) return left.seconds < right.seconds ? -1 : 1;
  return left.nanos === right.nanos ? 0 : left.nanos < right.nanos ? -1 : 1;
}

function validTimestampValue(
  value: { readonly seconds: bigint; readonly nanos: number } | undefined,
): value is { readonly seconds: bigint; readonly nanos: number } {
  return value !== undefined &&
    value.seconds >= PROTO_TIMESTAMP_MIN_SECONDS &&
    value.seconds <= PROTO_TIMESTAMP_MAX_SECONDS &&
    Number.isInteger(value.nanos) && value.nanos >= 0 && value.nanos <= 999_999_999;
}

function validDatasetId(value: string): boolean {
  if (!/^[A-Za-z0-9][A-Za-z0-9._:-]{0,255}$/.test(value) || value.includes("..")) return false;
  return !value.split(/[._:-]/).some((part) => part.toLowerCase() === "latest" || part.toLowerCase() === "fallback");
}

function validSourceIdentity(value: string): boolean {
  return value.length > 0 && hasOnlyUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= 128 &&
    value.trim() === value && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function validSortedSymbols(values: readonly string[]): boolean {
  return values.every((value) =>
    value.length > 0 && hasOnlyUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= 256 &&
    value.trim() === value && !/[\u0000-\u001f\u007f-\u009f]/u.test(value)
  ) && values.every((value, index) => index === 0 || compareUtf8(values[index - 1]!, value) < 0);
}

function validMarketSymbol(value: string): boolean {
  return value.length > 0 && hasOnlyUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= 256 &&
    value.trim() === value && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function validIsoDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const parsed = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(parsed.valueOf()) && parsed.toISOString().slice(0, 10) === value;
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
  if (value.length === 0 || !hasOnlyUnicodeScalars(value) ||
    new TextEncoder().encode(value).byteLength > 512 ||
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

function requireProtoJsonFields(
  value: unknown,
  camelFields: readonly string[],
  optionalCamelFields: readonly string[] = [],
): void {
  if (!isRecord(value)) throw new TypeError("ProtoJSON row must be an object");
  rejectDuplicateProtoFieldSpellings(value);
  for (const camel of camelFields) {
    if (!hasAliasedField(value, camel)) {
      throw new TypeError(`missing required ProtoJSON field: ${camel}`);
    }
  }
  const allowed = new Set([...camelFields, ...optionalCamelFields].flatMap((camel) => [
    camel,
    camel.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`),
  ]));
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) throw new TypeError(`unknown ProtoJSON row field: ${key}`);
  }
}

function hasAliasedField(value: Record<string, unknown>, camel: string): boolean {
  const snake = camel.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
  return camel in value || snake in value;
}

function getAliasedValue(value: Record<string, unknown>, camel: string, snake: string): unknown {
  return camel in value ? value[camel] : value[snake];
}

function validateDatasetManifestV2EnumNames(value: unknown): void {
  const source = getAliasedRecord(value, "source", "source");
  if (source !== undefined) {
    const encoding = getAliasedValue(source, "numericEncoding", "numeric_encoding");
    if (typeof encoding !== "string" || !NUMERIC_ENCODING_V2_NAMES.has(encoding)) {
      throw new TypeError("dataset v2 enum fields must use supported ProtoJSON names");
    }
  }

  const completion = getAliasedRecord(value, "completionEvidence", "completion_evidence");
  const finite = completion === undefined
    ? undefined
    : getAliasedRecord(completion, "finiteBatch", "finite_batch");
  if (finite !== undefined) {
    const sourceKind = getAliasedValue(finite, "sourceKind", "source_kind");
    if (typeof sourceKind !== "string" || !FINITE_SOURCE_KIND_V2_NAMES.has(sourceKind)) {
      throw new TypeError("dataset v2 enum fields must use supported ProtoJSON names");
    }
  }
}

function validateProtoTimestampV2Fields(value: unknown, depth = 0): void {
  if (depth > 64) throw new RangeError("dataset v2 JSON nesting exceeds its bound");
  if (Array.isArray(value)) {
    for (const nested of value) validateProtoTimestampV2Fields(nested, depth + 1);
    return;
  }
  if (!isRecord(value)) return;
  for (const [key, nested] of Object.entries(value)) {
    if (PROTO_TIMESTAMP_V2_FIELDS.has(key) && nested !== null) {
      if (typeof nested !== "string") {
        throw new TypeError("protobuf timestamp must be an RFC3339 string");
      }
      const match = PROTO_TIMESTAMP_V2.exec(nested);
      if (match === null) {
        throw new TypeError("protobuf timestamp must preserve at most 9 fractional digits");
      }
      const year = Number(match[1]);
      const month = Number(match[2]);
      const day = Number(match[3]);
      const hour = Number(match[4]);
      const minute = Number(match[5]);
      const second = Number(match[6]);
      const offsetHour = match[9] === undefined ? undefined : Number(match[9]);
      const offsetMinute = match[10] === undefined ? undefined : Number(match[10]);
      const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
      const daysInMonth = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
      if (
        year === 0 || month < 1 || month > 12 || day < 1 || day > daysInMonth[month - 1]! ||
        hour > 23 || minute > 59 || second > 59 ||
        (offsetHour !== undefined && (offsetHour > 23 || offsetMinute! > 59))
      ) {
        throw new TypeError("protobuf timestamp contains an out-of-range component");
      }
    } else {
      validateProtoTimestampV2Fields(nested, depth + 1);
    }
  }
}

function rejectDuplicateProtoFieldSpellings(value: unknown, depth = 0): void {
  if (depth > 64) throw new RangeError("ProtoJSON nesting exceeds its configured bound");
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

function assertBoundedProtoJsonText(text: string): void {
  // UTF-16 code-unit length is a cheap lower bound; reject before allocating the UTF-8 copy.
  if (text.length > MAX_PROTOJSON_TEXT_BYTES) {
    throw new RangeError("ProtoJSON text exceeds the configured byte limit");
  }
  if (new TextEncoder().encode(text).byteLength > MAX_PROTOJSON_TEXT_BYTES) {
    throw new RangeError("ProtoJSON text exceeds the configured byte limit");
  }
}

export function parsePredictionEnvelopeProtoJson(value: unknown): PredictionEnvelopeV1 {
  if (encodedJsonSize(value) > MAX_PROTOJSON_TEXT_BYTES) {
    throw new RangeError("prediction ProtoJSON exceeds the configured byte limit");
  }
  rejectDuplicateProtoFieldSpellings(value);
  validateProtoTimestampV2Fields(value);
  validatePredictionEnumNames(value);
  validateUint64JsonPaths(value, [["forecast", "sequence"]]);
  rejectDuplicateNumericEncodingAliases(value);
  const message = fromJson(PredictionEnvelopeV1Schema, value as JsonValue);
  if (rawFrameSchemaIds(message.source?.numericEncoding) !== undefined) {
    throw new TypeError("raw byte-frame encodings cannot identify a normalized prediction source");
  }
  return message;
}

function validatePredictionEnumNames(value: unknown): void {
  const source = getRecordField(value, "source");
  const quality = getRecordField(value, "quality");
  const horizon = getRecordField(value, "horizon");
  const forecast = getRecordField(value, "forecast");
  const forecastHorizon = forecast === undefined ? undefined : getAliasedRecord(forecast, "forecastHorizon", "forecast_horizon");
  if (source !== undefined && hasAliasedField(source, "numericEncoding")) {
    requireNamedEnum(source, "numericEncoding", "numeric_encoding", [
      "NUMERIC_ENCODING_DECIMAL_TOKEN",
      "NUMERIC_ENCODING_INTEGER_TOKEN",
      "NUMERIC_ENCODING_BINARY_FLOAT64_SHORTEST_DECIMAL",
      "NUMERIC_ENCODING_BINARY_FLOAT32_SHORTEST_DECIMAL",
    ]);
  }
  if (quality !== undefined && hasAliasedField(quality, "status")) {
    requireNamedEnum(quality, "status", "status", ["PASS", "UNVERIFIED", "BLOCKED_DATA", "FAILED"]);
  }
  if (horizon !== undefined && hasAliasedField(horizon, "unit")) {
    requireNamedEnum(horizon, "unit", "unit", ["ELAPSED_MINUTES", "SESSION_CLOSE", "TRADING_DAYS"]);
  }
  if (forecastHorizon !== undefined && hasAliasedField(forecastHorizon, "unit")) {
    requireNamedEnum(forecastHorizon, "unit", "unit", ["ELAPSED_MINUTES", "SESSION_CLOSE", "TRADING_DAYS"]);
  }
}

function requireNamedEnum(
  value: Record<string, unknown>,
  camel: string,
  snake: string,
  accepted: readonly string[],
): void {
  const item = value[camel] ?? value[snake];
  if (typeof item !== "string" || !accepted.includes(item)) {
    throw new TypeError("prediction enum fields require supported named ProtoJSON values");
  }
}

/**
 * Parse untrusted PredictionEnvelope ProtoJSON text without losing duplicate keys or field aliases.
 * The byte and nesting bounds are enforced before generated protobuf conversion.
 */
export function parsePredictionEnvelopeProtoJsonText(text: string): PredictionEnvelopeV1 {
  assertBoundedProtoJsonText(text);
  rejectDuplicateJsonObjectKeys(text);
  const value: unknown = JSON.parse(text);
  rejectDuplicateProtoFieldSpellings(value);
  return parsePredictionEnvelopeProtoJson(value);
}

const ENGINE_STATUS_FIELDS = [
  "api_version",
  "service",
  "service_readiness",
  "source_readiness",
  "mode",
  "projection_consistency",
  "execution_enabled",
  "mutation_routes_enabled",
  "schema_version",
  "pending_unknown_count",
  "pending_unknown_count_capped",
  "pending_unknown_consumed_risk_count",
  "pending_unknown_unverified_risk_count",
] as const;

const SYNTHETIC_PREVIEW_FIELDS = [
  "api_version",
  "preview_kind",
  "source_mode",
  "source_provenance",
  "projection_consistency",
  "execution_enabled",
  "order_mutations_enabled",
  "account_data_loaded",
  "market_data_connected",
  "source_schema_version",
  "pending_unknown_count",
  "pending_unknown_count_capped",
  "pending_unknown_consumed_risk_count",
  "pending_unknown_unverified_risk_count",
  "disposition",
] as const;

/**
 * Parse the exact snake_case status JSON emitted by the offline engine reader.
 * This checks shape and fail-closed diagnostic invariants; it grants no authority.
 */
function parseEngineStatusResponseV1ProtoJson(value: unknown): EngineStatusResponseV1 {
  const row = requireExactProtoJsonFields(value, ENGINE_STATUS_FIELDS);
  requireLiteral(row, "api_version", "v1");
  requireLiteral(row, "service", "offline-persist-preview");
  requireLiteral(row, "service_readiness", "read_only_ready");
  requireLiteral(row, "source_readiness", "unknown");
  requireLiteral(row, "mode", "synthetic_offline");
  requireLiteral(row, "projection_consistency", "best_effort_non_transactional");
  requireFalse(row, "execution_enabled");
  requireFalse(row, "mutation_routes_enabled");
  requireUInt32(row, "schema_version", 1);
  validateUnknownSample(row);
  return fromJson(EngineStatusResponseV1Schema, row as JsonValue);
}

/** Parse untrusted raw HTTP response text without losing duplicate keys or aliases. */
export function parseEngineStatusResponseV1ProtoJsonText(text: string): EngineStatusResponseV1 {
  assertBoundedProtoJsonText(text);
  rejectDuplicateJsonObjectKeys(text);
  const value: unknown = JSON.parse(text);
  rejectDuplicateProtoFieldSpellings(value);
  return parseEngineStatusResponseV1ProtoJson(value);
}

/**
 * Parse the exact snake_case synthetic preview emitted by the offline engine reader.
 * This is a best-effort diagnostic projection, not a state or execution authority.
 */
function parseSyntheticOfflinePreviewV1ProtoJson(
  value: unknown,
): SyntheticOfflinePreviewV1 {
  const row = requireExactProtoJsonFields(value, SYNTHETIC_PREVIEW_FIELDS);
  requireLiteral(row, "api_version", "v1");
  requireLiteral(row, "preview_kind", "synthetic_session_state");
  requireLiteral(row, "source_mode", "synthetic_offline");
  requireLiteral(row, "source_provenance", "synthetic_only");
  requireLiteral(row, "projection_consistency", "best_effort_non_transactional");
  requireFalse(row, "execution_enabled");
  requireFalse(row, "order_mutations_enabled");
  requireFalse(row, "account_data_loaded");
  requireFalse(row, "market_data_connected");
  requireUInt32(row, "source_schema_version", 1);
  const sample = validateUnknownSample(row);
  const expectedDisposition = sample.unverified > 0
    ? "unknown_reservation_state"
    : sample.count > 0
      ? "reconciliation_required"
      : "no_pending_unknown_in_sample";
  requireLiteral(row, "disposition", expectedDisposition);
  return fromJson(SyntheticOfflinePreviewV1Schema, row as JsonValue);
}

/** Parse untrusted raw HTTP response text without losing duplicate keys or aliases. */
export function parseSyntheticOfflinePreviewV1ProtoJsonText(
  text: string,
): SyntheticOfflinePreviewV1 {
  assertBoundedProtoJsonText(text);
  rejectDuplicateJsonObjectKeys(text);
  const value: unknown = JSON.parse(text);
  rejectDuplicateProtoFieldSpellings(value);
  return parseSyntheticOfflinePreviewV1ProtoJson(value);
}

function requireExactProtoJsonFields(
  value: unknown,
  fields: readonly string[],
): Record<string, unknown> {
  if (!isRecord(value)) throw new TypeError("engine response must be a JSON object");
  const allowed = new Set(fields);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) throw new TypeError(`unknown engine response field: ${key}`);
  }
  for (const field of fields) {
    if (!Object.hasOwn(value, field)) throw new TypeError(`missing engine response field: ${field}`);
  }
  return value;
}

function requireLiteral(
  row: Record<string, unknown>,
  field: string,
  expected: string,
): void {
  if (row[field] !== expected) throw new TypeError(`engine response field ${field} is unsupported`);
}

function requireFalse(row: Record<string, unknown>, field: string): void {
  if (row[field] !== false) throw new TypeError(`engine response field ${field} must be false`);
}

function requireUInt32(row: Record<string, unknown>, field: string, minimum: number): number {
  const value = row[field];
  if (
    typeof value !== "number" ||
    !Number.isInteger(value) ||
    value < minimum ||
    value > 0xffff_ffff
  ) {
    throw new TypeError(`engine response field ${field} is outside uint32 bounds`);
  }
  return value;
}

function validateUnknownSample(row: Record<string, unknown>): {
  count: number;
  unverified: number;
} {
  const count = requireUInt32(row, "pending_unknown_count", 0);
  const consumed = requireUInt32(row, "pending_unknown_consumed_risk_count", 0);
  const unverified = requireUInt32(row, "pending_unknown_unverified_risk_count", 0);
  if (count > MAX_ENGINE_UNKNOWN_SAMPLE || consumed > count || unverified > count ||
    consumed + unverified !== count) {
    throw new TypeError("engine response UNKNOWN sample counts are inconsistent");
  }
  if (row.pending_unknown_count_capped !== (count === MAX_ENGINE_UNKNOWN_SAMPLE)) {
    throw new TypeError("engine response UNKNOWN sample cap flag is inconsistent");
  }
  return { count, unverified };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getRecordField(value: unknown, key: string): Record<string, unknown> | undefined {
  if (!isRecord(value)) return undefined;
  const field = value[key];
  return isRecord(field) ? field : undefined;
}

function rawFrameSchemaIds(encoding: NumericEncodingV1 | undefined): readonly string[] | undefined {
  switch (encoding) {
    case NumericEncodingV1.NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES:
      return ["lqepoch.market_raw_frame.v1", "lqepoch.market_raw_frame.v2"];
    case NumericEncodingV1.NUMERIC_ENCODING_RAW_JSON_BYTES:
      return ["lqepoch.market_raw_json_frame.v1", "lqepoch.market_raw_json_frame.v2"];
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
