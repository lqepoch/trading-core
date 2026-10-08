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
import { trustedParquetSchemaSha256 } from "./schema-fingerprint.js";

const UINT64_MAX = 18_446_744_073_709_551_615n;
const CANONICAL_UINT64 = /^(0|[1-9][0-9]*)$/;

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
