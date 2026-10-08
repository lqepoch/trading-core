import { createHash } from "node:crypto";
import { trustedParquetSchemaDescriptor } from "./schema-fingerprint.js";

export const MARKET_RAW_FRAME_V2_SCHEMA_ID = "lqepoch.market_raw_frame.v2";
export const MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID = "lqepoch.market_raw_json_frame.v2";
export const MARKET_EVENT_V3_SCHEMA_ID = "lqepoch.market_event.v3";
export const MAX_RAW_FRAME_BYTES = 1024 * 1024;
export const MAX_RAW_FRAME_EVENT_COUNT = 512;
export const MAX_RAW_CAPTURE_CHUNK_FRAMES = 1024;
export const MAX_RAW_CAPTURE_CHUNK_BYTES = 16 * 1024 * 1024;
export const MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES = 16 * 1024 * 1024;
export const MAX_RAW_FRAME_SYMBOLS_JSON_BYTES = MAX_RAW_FRAME_EVENT_COUNT * (2 * 256 + 3) + 1;

const UINT64_MAX = (1n << 64n) - 1n;
const INT64_MIN = -(1n << 63n);
const INT64_MAX = (1n << 63n) - 1n;
const CAPTURE_INSTANCE_ID = /^[0-9a-f]{12}4[0-9a-f]{3}[89ab][0-9a-f]{15}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const DECIMAL = /^(-?)(0|[1-9][0-9]*)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?$/;
const NUMERIC_ENCODINGS = new Set([
  "decimal_token",
  "integer_token",
  "binary_float64_shortest_decimal",
  "binary_float32_shortest_decimal",
]);
const ENTITLEMENTS = new Set(["unknown", "authorized", "unauthorized"]);
const DISPOSITIONS = new Set([
  "market_data",
  "control",
  "unknown_message",
  "malformed_message",
  "provider_error",
]);
const EVENT_KINDS = new Set(["stock_quote", "stock_trade", "option_quote", "option_trade"]);
const REFERENCE_FIELDS = [
  "raw_frame_capture_instance_id",
  "raw_frame_source_generation",
  "raw_frame_generation",
  "raw_frame_sequence",
  "raw_frame_event_ordinal",
  "raw_frame_event_count",
] as const;

export type ParquetRow = Readonly<Record<string, unknown>>;

/** Require the capture spool's exact 32-character lowercase UUIDv4/RFC-variant form. */
export function validateCaptureInstanceIdV2(value: unknown): string {
  if (typeof value !== "string" || !CAPTURE_INSTANCE_ID.test(value)) {
    throw new TypeError("capture instance ID must be a lowercase UUIDv4 with RFC variant");
  }
  return value;
}

/** Validate one exact MessagePack or JSON raw-frame Parquet row. */
export function validateRawFrameRowV2(row: ParquetRow, schemaId: string): void {
  validateRawFrameRowV2AndSymbols(row, schemaId);
}

function validateRawFrameRowV2AndSymbols(row: ParquetRow, schemaId: string): string[] {
  if (
    schemaId !== MARKET_RAW_FRAME_V2_SCHEMA_ID &&
    schemaId !== MARKET_RAW_JSON_FRAME_V2_SCHEMA_ID
  ) {
    throw new TypeError("raw-frame v2 validator requires a trusted raw-frame v2 schema ID");
  }
  requireExactRowFields(row, schemaId);
  if (!isUint32(row.schema_version) || row.schema_version !== 2) {
    throw new TypeError("raw-frame v2 row schema_version must equal 2");
  }
  validateSource(row.provider, row.feed, row.entitlement);
  if (row.source_numeric_encoding !== null && !NUMERIC_ENCODINGS.has(asString(row.source_numeric_encoding))) {
    throw new TypeError("raw-frame projection encoding must be a normalized numeric encoding");
  }
  validateCaptureInstanceIdV2(row.capture_instance_id);
  positiveUint64(row.source_generation, "source_generation");
  positiveUint64(row.source_frame_sequence, "source_frame_sequence");
  positiveUint64(row.canonical_generation, "canonical_generation");
  timestampNanoseconds(row.received_timestamp_utc);
  if (typeof row.frame_sha256 !== "string" || !SHA256.test(row.frame_sha256)) {
    throw new TypeError("raw-frame SHA-256 must be lowercase hexadecimal");
  }
  if (!(row.frame_bytes instanceof Uint8Array) || row.frame_bytes.byteLength > MAX_RAW_FRAME_BYTES) {
    throw new TypeError("raw-frame payload must fit the 1 MiB byte bound");
  }
  const actualHash = createHash("sha256").update(row.frame_bytes).digest("hex");
  if (actualHash !== row.frame_sha256) {
    throw new TypeError("raw-frame SHA-256 does not match the exact bytes");
  }
  if (!isUint32(row.event_count) || row.event_count > MAX_RAW_FRAME_EVENT_COUNT) {
    throw new TypeError("raw-frame event count is outside the per-frame bound");
  }
  const disposition = asString(row.disposition);
  if (!DISPOSITIONS.has(disposition)) throw new TypeError("unknown raw-frame disposition");
  const symbols = canonicalSymbols(row.symbols_json);
  if (disposition === "market_data" && (row.event_count === 0 || symbols.length === 0)) {
    throw new TypeError("market-data raw frames require expected events and symbols");
  }
  if (disposition === "control" && (row.event_count !== 0 || symbols.length !== 0)) {
    throw new TypeError("control raw frames cannot claim normalized market events");
  }
  if (row.event_count > 0 && (symbols.length === 0 || symbols.length > row.event_count)) {
    throw new TypeError("expected events require a bounded non-empty symbol set");
  }
  return symbols;
}

/** Validate V3 Parquet row shape and capture columns; Rust remains semantic admission authority. */
export function validateMarketEventRowV3(row: ParquetRow): void {
  requireExactRowFields(row, MARKET_EVENT_V3_SCHEMA_ID);
  if (!isUint32(row.schema_version) || row.schema_version !== 1) {
    throw new TypeError("event v3 storage rows retain wire schema_version 1");
  }
  validateSource(row.provider, row.feed, row.entitlement);
  if (typeof row.numeric_encoding !== "string" || !NUMERIC_ENCODINGS.has(row.numeric_encoding)) {
    throw new TypeError("event row requires a normalized numeric encoding");
  }
  if (row.source_record_id !== null && !validSourceIdentity(row.source_record_id)) {
    throw new TypeError("invalid source record identity");
  }
  if (row.raw_frame_sha256 !== null &&
      (typeof row.raw_frame_sha256 !== "string" || !SHA256.test(row.raw_frame_sha256))) {
    throw new TypeError("invalid raw-frame SHA-256 reference");
  }
  if (
    (row.numeric_encoding.startsWith("binary_float") && row.raw_frame_sha256 === null) ||
    ((row.provider === "synthetic" || row.feed === "synthetic") &&
      row.numeric_encoding !== "decimal_token")
  ) {
    throw new TypeError("event source encoding requires a binary-float digest and decimal synthetic values");
  }
  const generation = positiveUint64(row.generation, "generation");
  positiveUint64(row.sequence, "sequence");
  if (row.source_timestamp !== null) timestampNanoseconds(row.source_timestamp);
  timestampNanoseconds(row.received_timestamp);
  if (typeof row.event_kind !== "string" || !EVENT_KINDS.has(row.event_kind)) {
    throw new TypeError("unknown normalized event kind");
  }
  if (typeof row.symbol !== "string" || !validMarketSymbol(row.symbol)) {
    throw new TypeError("invalid normalized event symbol");
  }
  validateEventValues(row, row.event_kind);

  const present = REFERENCE_FIELDS.map((name) => row[name] !== null);
  if (present.some(Boolean) && !present.every(Boolean)) {
    throw new TypeError("event v3 capture reference columns must be all present or all absent");
  }
  if (present.every(Boolean)) {
    validateCaptureInstanceIdV2(row.raw_frame_capture_instance_id);
    positiveUint64(row.raw_frame_source_generation, "raw_frame_source_generation");
    const canonicalGeneration = positiveUint64(row.raw_frame_generation, "raw_frame_generation");
    positiveUint64(row.raw_frame_sequence, "raw_frame_sequence");
    if (
      !isUint32(row.raw_frame_event_ordinal) ||
      !isUint32(row.raw_frame_event_count) ||
      row.raw_frame_event_count === 0 ||
      row.raw_frame_event_count > MAX_RAW_FRAME_EVENT_COUNT ||
      row.raw_frame_event_ordinal === 0 ||
      row.raw_frame_event_ordinal > row.raw_frame_event_count ||
      canonicalGeneration !== generation ||
      row.raw_frame_sha256 === null
    ) {
      throw new TypeError("invalid event v3 capture reference");
    }
  }
}

/** Cross-check row pairing; this helper does not replace Rust market-domain validation. */
export function validateEventAgainstRawFrameRowV2(
  event: ParquetRow,
  rawFrame: ParquetRow,
  schemaId: string,
): void {
  const symbols = validateRawFrameRowV2AndSymbols(rawFrame, schemaId);
  validateMarketEventRowV3(event);
  validateEventFramePair(event, rawFrame, symbols);
}

/** Validate one bounded UUID/source-generation chunk and require complete event ordinals. */
export function validateRawEventChunkV2(
  rawFrames: readonly ParquetRow[],
  events: readonly ParquetRow[],
  schemaId: string,
): void {
  if (!Array.isArray(rawFrames) || rawFrames.length === 0) {
    throw new TypeError("capture chunk must contain at least one raw frame");
  }
  if (rawFrames.length > MAX_RAW_CAPTURE_CHUNK_FRAMES) {
    throw new RangeError("capture chunk exceeds the 1024-frame bound");
  }
  if (!Array.isArray(events)) throw new TypeError("capture events must be a bounded array");
  if (events.length > MAX_RAW_CAPTURE_CHUNK_FRAMES * MAX_RAW_FRAME_EVENT_COUNT) {
    throw new RangeError("capture chunk expected event count exceeds its structural bound");
  }
  assertDenseArray(rawFrames, "raw frames");
  assertDenseArray(events, "events");

  let captureId: string | undefined;
  let sourceGeneration: bigint | undefined;
  let expectedSequence: bigint | undefined;
  let payloadBytes = 0;
  let metadataBytes = 0;
  let expectedEventCount = 0;
  const frameBySequence = new Map<string, number>();
  const groupedEvents: ParquetRow[][] = rawFrames.map(() => []);
  const symbolsByFrame: string[][] = [];

  for (let index = 0; index < rawFrames.length; index += 1) {
    const frame = rawFrames[index]!;
    const symbols = validateRawFrameRowV2AndSymbols(frame, schemaId);
    const currentCaptureId = validateCaptureInstanceIdV2(frame.capture_instance_id);
    const currentSourceGeneration = positiveUint64(frame.source_generation, "source_generation");
    const sequence = positiveUint64(frame.source_frame_sequence, "source_frame_sequence");
    if (captureId === undefined) {
      captureId = currentCaptureId;
      sourceGeneration = currentSourceGeneration;
    } else if (currentCaptureId !== captureId || currentSourceGeneration !== sourceGeneration) {
      throw new TypeError("capture chunk cannot mix capture UUIDs or source generations");
    }
    if (expectedSequence !== undefined && sequence !== expectedSequence) {
      throw new TypeError("capture chunk frame sequences must be contiguous");
    }
    if (frameBySequence.has(sequence.toString())) {
      throw new TypeError("capture chunk contains a duplicate source frame sequence");
    }
    frameBySequence.set(sequence.toString(), index);
    expectedSequence = sequence + 1n;
    payloadBytes += (frame.frame_bytes as Uint8Array).byteLength;
    metadataBytes += new TextEncoder().encode(frame.symbols_json as string).byteLength;
    if (payloadBytes > MAX_RAW_CAPTURE_CHUNK_BYTES) {
      throw new RangeError("capture chunk exceeds the 16 MiB payload bound");
    }
    if (metadataBytes > MAX_RAW_CAPTURE_CHUNK_METADATA_BYTES) {
      throw new RangeError("capture chunk exceeds the 16 MiB symbols metadata bound");
    }
    expectedEventCount += frame.event_count as number;
    symbolsByFrame.push(symbols);
  }

  if (events.length !== expectedEventCount) {
    throw new TypeError("capture chunk does not contain every expected normalized event");
  }
  for (const event of events) {
    validateMarketEventRowV3(event);
    if (
      event.raw_frame_capture_instance_id !== captureId ||
      positiveUint64(event.raw_frame_source_generation, "raw_frame_source_generation") !== sourceGeneration
    ) {
      throw new TypeError("event capture identity does not match this chunk");
    }
    const sequence = positiveUint64(event.raw_frame_sequence, "raw_frame_sequence");
    const index = frameBySequence.get(sequence.toString());
    if (index === undefined) throw new TypeError("event references a frame outside this chunk");
    groupedEvents[index]!.push(event);
  }

  for (let index = 0; index < rawFrames.length; index += 1) {
    const frame = rawFrames[index]!;
    const frameEvents = groupedEvents[index]!;
    if (frameEvents.length !== frame.event_count) {
      throw new TypeError("capture chunk is missing a frame projection");
    }
    const ordinals = new Set<number>();
    for (const event of frameEvents) {
      validateEventFramePair(event, frame, symbolsByFrame[index]!);
      const ordinal = event.raw_frame_event_ordinal as number;
      if (ordinals.has(ordinal)) throw new TypeError("capture chunk contains duplicate event ordinals");
      ordinals.add(ordinal);
    }
    if (ordinals.size !== frame.event_count) {
      throw new TypeError("capture chunk event ordinals are incomplete");
    }
  }
}

function validateEventFramePair(
  event: ParquetRow,
  rawFrame: ParquetRow,
  symbols: readonly string[],
): void {
  if (
    event.raw_frame_capture_instance_id !== rawFrame.capture_instance_id ||
    positiveUint64(event.raw_frame_source_generation, "raw_frame_source_generation") !==
      positiveUint64(rawFrame.source_generation, "source_generation") ||
    positiveUint64(event.raw_frame_generation, "raw_frame_generation") !==
      positiveUint64(rawFrame.canonical_generation, "canonical_generation") ||
    positiveUint64(event.generation, "generation") !==
      positiveUint64(rawFrame.canonical_generation, "canonical_generation") ||
    positiveUint64(event.raw_frame_sequence, "raw_frame_sequence") !==
      positiveUint64(rawFrame.source_frame_sequence, "source_frame_sequence") ||
    event.raw_frame_event_count !== rawFrame.event_count ||
    event.raw_frame_sha256 !== rawFrame.frame_sha256 ||
    event.provider !== rawFrame.provider ||
    event.feed !== rawFrame.feed ||
    event.entitlement !== rawFrame.entitlement ||
    timestampNanoseconds(event.received_timestamp) !== timestampNanoseconds(rawFrame.received_timestamp_utc) ||
    (rawFrame.source_numeric_encoding !== null &&
      event.numeric_encoding !== rawFrame.source_numeric_encoding) ||
    !symbols.includes(asString(event.symbol))
  ) {
    throw new TypeError("event row does not match the exact raw-frame capture key");
  }
}

function validateSource(provider: unknown, feed: unknown, entitlement: unknown): void {
  if (
    !validSourceIdentity(provider) ||
    !validSourceIdentity(feed) ||
    typeof entitlement !== "string" ||
    !ENTITLEMENTS.has(entitlement)
  ) {
    throw new TypeError("invalid source identity or entitlement spelling");
  }
  const mentionsSynthetic = provider.toLowerCase() === "synthetic" || feed.toLowerCase() === "synthetic";
  if (mentionsSynthetic && (provider !== "synthetic" || feed !== "synthetic")) {
    throw new TypeError("synthetic provider and feed identities must be paired");
  }
}

function requireExactRowFields(row: ParquetRow, schemaId: string): void {
  const expected = trustedParquetSchemaDescriptor(schemaId).fields.map((field) => field.name);
  const actual = Object.keys(row);
  if (actual.length !== expected.length || expected.some((field) => !Object.hasOwn(row, field))) {
    throw new TypeError("Parquet row fields differ from the trusted schema descriptor");
  }
}

function assertDenseArray(values: readonly unknown[], label: string): void {
  for (let index = 0; index < values.length; index += 1) {
    if (!Object.hasOwn(values, index)) throw new TypeError(`capture ${label} cannot contain sparse holes`);
  }
}

function positiveUint64(value: unknown, fieldName: string): bigint {
  let parsed: bigint;
  if (typeof value === "bigint") {
    parsed = value;
  } else if (typeof value === "string" && value.length <= 20) {
    const match = /^(0|[1-9][0-9]*)/.exec(value);
    if (match?.[0] !== value) {
      throw new TypeError(`${fieldName} must be an exact uint64 value`);
    }
    parsed = BigInt(value);
  } else {
    throw new TypeError(`${fieldName} must be an exact uint64 value`);
  }
  if (parsed <= 0n || parsed > UINT64_MAX) throw new RangeError(`${fieldName} must be positive uint64`);
  return parsed;
}

function isUint32(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 0xffff_ffff;
}

function canonicalSymbols(value: unknown): string[] {
  if (typeof value !== "string") {
    throw new TypeError("symbols_json exceeds its bounded UTF-8 size");
  }
  // UTF-8 uses at least as many bytes as UTF-16 code units for valid scalar
  // text. Reject this cheap lower bound before allocating an encoded copy.
  if (value.length > MAX_RAW_FRAME_SYMBOLS_JSON_BYTES) {
    throw new TypeError("symbols_json exceeds its bounded UTF-8 size");
  }
  if (!hasOnlyUnicodeScalars(value)) {
    throw new TypeError("symbols_json must contain valid Unicode scalar text");
  }
  if (new TextEncoder().encode(value).byteLength > MAX_RAW_FRAME_SYMBOLS_JSON_BYTES) {
    throw new TypeError("symbols_json exceeds its bounded UTF-8 size");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(value) as unknown;
  } catch {
    throw new TypeError("symbols_json is malformed");
  }
  if (
    !Array.isArray(parsed) ||
    parsed.length > MAX_RAW_FRAME_EVENT_COUNT ||
    parsed.some((symbol) => !validMarketSymbol(symbol)) ||
    parsed.some((symbol, index) => index > 0 && compareUtf8(parsed[index - 1] as string, symbol as string) >= 0) ||
    JSON.stringify(parsed) !== value
  ) {
    throw new TypeError("symbols_json must be a canonical sorted unique symbol array");
  }
  return parsed as string[];
}

function timestampNanoseconds(value: unknown): bigint {
  if (typeof value === "bigint") {
    if (value < INT64_MIN || value > INT64_MAX) throw new RangeError("timestamp exceeds signed Arrow range");
    return value;
  }
  if (typeof value !== "string") {
    throw new TypeError("timestamps must be exact RFC3339 UTC text or signed epoch nanoseconds");
  }
  const match = /^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]{1,9}))?(?:Z|([+-])([0-9]{2}):([0-9]{2}))$/.exec(value);
  if (match === null) throw new TypeError("timestamp is not exact RFC3339 UTC");
  const [, y, mo, d, h, mi, s, fraction, sign, offsetHour, offsetMinute] = match;
  const year = Number(y);
  const month = Number(mo);
  const day = Number(d);
  const hour = Number(h);
  const minute = Number(mi);
  const second = Number(s);
  if (
    year < 1677 || year > 2262 ||
    (sign !== undefined && (offsetHour !== "00" || offsetMinute !== "00")) ||
    second > 59 || hour > 23 || minute > 59
  ) {
    throw new RangeError("timestamp is outside supported UTC nanosecond range");
  }
  const milliseconds = Date.UTC(year, month - 1, day, hour, minute, second);
  const check = new Date(milliseconds);
  if (
    Number.isNaN(milliseconds) ||
    check.getUTCFullYear() !== year || check.getUTCMonth() !== month - 1 || check.getUTCDate() !== day ||
    check.getUTCHours() !== hour || check.getUTCMinutes() !== minute || check.getUTCSeconds() !== second
  ) {
    throw new TypeError("timestamp contains an invalid Gregorian date");
  }
  const nanos = BigInt((fraction ?? "").padEnd(9, "0"));
  const result = BigInt(Math.trunc(milliseconds / 1000)) * 1_000_000_000n + nanos;
  if (result < INT64_MIN || result > INT64_MAX) throw new RangeError("timestamp exceeds signed Arrow range");
  return result;
}

function validSourceIdentity(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 &&
    value.length <= 128 &&
    hasOnlyUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= 128 && value.trim() === value &&
    !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function validMarketSymbol(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 &&
    value.length <= 256 &&
    hasOnlyUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= 256 && value.trim() === value &&
    !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function hasOnlyUnicodeScalars(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
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

function validateEventValues(row: ParquetRow, eventKind: string): void {
  const fields = ["price", "size", "bid", "ask", "bid_size", "ask_size"] as const;
  for (const field of fields) {
    const value = row[field];
    if (value === null) continue;
    if (typeof value !== "string" || value.length > 128 || !DECIMAL.test(value)) {
      throw new TypeError(`event row ${field} must be a bounded exact decimal string`);
    }
  }
  if (eventKind === "stock_trade" || eventKind === "option_trade") {
    if (
      typeof row.price !== "string" || typeof row.size !== "string" ||
      decimalIsNegative(row.price) || decimalIsZero(row.price) ||
      decimalIsNegative(row.size) || decimalIsZero(row.size) ||
      fields.slice(2).some((field) => row[field] !== null)
    ) {
      throw new TypeError("trade row requires positive price/size and no quote fields");
    }
  } else if (
    row.price !== null || row.size !== null ||
    fields.slice(2).every((field) => row[field] === null) ||
    fields.slice(2).some((field) => typeof row[field] === "string" && decimalIsNegative(row[field] as string))
  ) {
    throw new TypeError("quote row requires non-negative quote fields and no trade fields");
  }
}

function decimalIsNegative(value: string): boolean {
  return value.startsWith("-") && !decimalIsZero(value);
}

function decimalIsZero(value: string): boolean {
  const match = DECIMAL.exec(value);
  return match !== null && `${match[2]}${match[3] ?? ""}`.split("").every((digit) => digit === "0");
}

function asString(value: unknown): string {
  if (typeof value !== "string") throw new TypeError("expected a string field");
  return value;
}
