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
      if (typeof current !== "object" || current === null || !(part in current)) {
        throw new TypeError(`missing uint64 JSON field: ${path.join(".")}`);
      }
      current = (current as Record<string, unknown>)[part];
    }
    parseCanonicalUint64Json(current);
  }
}

export function parseMarketEventProtoJson(value: unknown): MarketEventEnvelopeV1 {
  validateUint64JsonPaths(value, [["generation"], ["sequence"]]);
  if (hasRawMessagePackEncoding(value)) {
    throw new TypeError("raw_messagepack_bytes is not a normalized market-event encoding");
  }
  return fromJson(MarketEventEnvelopeV1Schema, value as JsonValue);
}

export function parseDatasetManifestProtoJson(value: unknown): DatasetManifestV1 {
  validateUint64JsonPaths(value, [
    ["sourceTimestampMissingRows"],
    ["rowCount"],
    ["object", "sizeBytes"],
    ["object", "parquetFooterRows"],
  ]);
  const rawEncoding = hasRawMessagePackEncoding(value);
  const object = getRecordField(value, "object");
  const schemaSha256 = object?.parquetSchemaSha256;
  const rawSchemaSha256 = trustedParquetSchemaSha256("lqepoch.market_raw_frame.v1");
  const root = isRecord(value) ? value : undefined;
  if (rawEncoding !== (schemaSha256 === rawSchemaSha256)) {
    throw new TypeError(
      "raw_messagepack_bytes must be bound to the registered market_raw_frame.v1 schema",
    );
  }
  if (
    rawEncoding &&
    (root === undefined ||
      "timeRange" in root ||
      "time_range" in root ||
      root.rowCount === "0" ||
      root.sourceTimestampMissingRows !== root.rowCount)
  ) {
    throw new TypeError(
      "raw-frame manifests require rows, no source-time range, and every frame timestamp missing",
    );
  }
  return fromJson(DatasetManifestV1Schema, value as JsonValue);
}

export function parsePredictionEnvelopeProtoJson(value: unknown): PredictionEnvelopeV1 {
  validateUint64JsonPaths(value, [["forecast", "sequence"]]);
  if (hasRawMessagePackEncoding(value)) {
    throw new TypeError("raw_messagepack_bytes cannot identify a normalized prediction source");
  }
  return fromJson(PredictionEnvelopeV1Schema, value as JsonValue);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getRecordField(value: unknown, key: string): Record<string, unknown> | undefined {
  if (!isRecord(value)) return undefined;
  const field = value[key];
  return isRecord(field) ? field : undefined;
}

function hasRawMessagePackEncoding(value: unknown): boolean {
  const source = getRecordField(value, "source");
  return (
    source?.numericEncoding === NumericEncodingV1.NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES ||
    source?.numericEncoding === "NUMERIC_ENCODING_RAW_MESSAGEPACK_BYTES"
  );
}
