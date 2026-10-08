import { createHash } from "node:crypto";
import { TRUSTED_PARQUET_SCHEMA_REGISTRY } from "./parquet-schema-registry.js";

export const PARQUET_SCHEMA_FINGERPRINT_PREFIX = "LQEpoch-Parquet-Schema-v1\n";
export const PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY = "lqepoch.schema_descriptor.v1";
export const PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY = "lqepoch.schema_fingerprint_sha256";
export const PARQUET_LOGICAL_TYPES = new Set([
	"binary",
	"bool",
  "date_iso8601",
  "decimal_string",
  "sha256_hex",
  "timestamp_ns_utc",
  "uint32",
  "uint64",
  "utf8",
]);

export type SchemaField = { name: string; nullable: boolean; type: string };
export type SchemaDescriptor = {
  schema_version: number;
  schema_id: string;
  fields: SchemaField[];
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Object.keys(value);
  return keys.length === expected.length && expected.every((key) => Object.hasOwn(value, key));
}

function isAsciiAlphaNumeric(code: number): boolean {
  return (
    (code >= 0x30 && code <= 0x39) ||
    (code >= 0x41 && code <= 0x5a) ||
    (code >= 0x61 && code <= 0x7a)
  );
}

function validSchemaId(value: string): boolean {
  if (value.length === 0 || value.length > 128 || !isAsciiAlphaNumeric(value.charCodeAt(0))) {
    return false;
  }
  for (let index = 1; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (!isAsciiAlphaNumeric(code) && ![0x5f, 0x2e, 0x3a, 0x2d].includes(code)) return false;
  }
  return true;
}

function validFieldName(value: string): boolean {
  if (value.length === 0 || value.length > 128) return false;
  const first = value.charCodeAt(0);
  if (!((first >= 0x41 && first <= 0x5a) || (first >= 0x61 && first <= 0x7a) || first === 0x5f)) {
    return false;
  }
  for (let index = 1; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (!isAsciiAlphaNumeric(code) && code !== 0x5f) return false;
  }
  return true;
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
        .map(([key, nested]) => [key, canonicalValue(nested)]),
    );
  }
  return value;
}

export function canonicalSchemaJson(value: unknown): string {
  if (!isRecord(value) || !hasExactKeys(value, ["schema_version", "schema_id", "fields"])) {
    throw new TypeError("schema descriptor must have exactly the defined keys");
  }
  const schemaVersion = value.schema_version;
  const schemaId = value.schema_id;
  const fields = value.fields;
  if (
    typeof schemaVersion !== "number" ||
    !Number.isSafeInteger(schemaVersion) ||
    schemaVersion !== 1 ||
    typeof schemaId !== "string" ||
    !validSchemaId(schemaId) ||
    !Array.isArray(fields) ||
    fields.length === 0
  ) {
    throw new TypeError("invalid schema descriptor header");
  }
  const names = new Set<string>();
  for (const field of fields) {
    if (
      !isRecord(field) ||
      !hasExactKeys(field, ["name", "nullable", "type"]) ||
      typeof field.name !== "string" ||
      !validFieldName(field.name) ||
      names.has(field.name) ||
      typeof field.nullable !== "boolean" ||
      typeof field.type !== "string" ||
      !PARQUET_LOGICAL_TYPES.has(field.type)
    ) {
      throw new TypeError("invalid schema field descriptor");
    }
    names.add(field.name);
  }
  return JSON.stringify(canonicalValue(value));
}

export function fingerprintSchemaSha256(descriptor: unknown): string {
  return createHash("sha256")
    .update(PARQUET_SCHEMA_FINGERPRINT_PREFIX, "utf8")
    .update(canonicalSchemaJson(descriptor), "utf8")
    .digest("hex");
}

/** Return the exact registry descriptor generated from the root schema fixture. */
export function trustedParquetSchemaDescriptor(schemaId: string): SchemaDescriptor {
  const entry = TRUSTED_PARQUET_SCHEMA_REGISTRY.schemas.find(
    (candidate) => candidate.descriptor.schema_id === schemaId,
  );
  if (entry === undefined) throw new TypeError(`unregistered Parquet schema ID: ${schemaId}`);
  return JSON.parse(JSON.stringify(entry.descriptor)) as SchemaDescriptor;
}

/** Return the golden fingerprint for a descriptor registered by trading-core. */
export function trustedParquetSchemaSha256(schemaId: string): string {
  const entry = TRUSTED_PARQUET_SCHEMA_REGISTRY.schemas.find(
    (candidate) => candidate.descriptor.schema_id === schemaId,
  );
  if (entry === undefined) throw new TypeError(`unregistered Parquet schema ID: ${schemaId}`);
  return entry.sha256;
}

/** Return the exact optional Arrow/Parquet metadata pair for a trusted schema. */
export function trustedParquetSchemaMetadata(schemaId: string): Record<string, string> {
  const descriptor = trustedParquetSchemaDescriptor(schemaId);
  return {
    [PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY]: canonicalSchemaJson(descriptor),
    [PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY]: fingerprintSchemaSha256(descriptor),
  };
}

/** Accept absent registry metadata for old files; if present, require both exact values. */
export function validateOptionalParquetSchemaMetadata(
  schemaId: string,
  metadata: unknown,
): void {
  const trusted = trustedParquetSchemaMetadata(schemaId);
  if (metadata === undefined || metadata === null) return;
  if (
    !isRecord(metadata) ||
    Object.values(metadata).some((value) => typeof value !== "string")
  ) {
    throw new TypeError("schema metadata must be a string map");
  }
  const hasDescriptor = Object.hasOwn(metadata, PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY);
  const hasFingerprint = Object.hasOwn(metadata, PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY);
  if (!hasDescriptor && !hasFingerprint) return;
  if (!hasDescriptor || !hasFingerprint) {
    throw new TypeError("schema metadata must include both registered descriptor keys");
  }
  if (
    metadata[PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY] !==
      trusted[PARQUET_SCHEMA_DESCRIPTOR_METADATA_KEY] ||
    metadata[PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY] !==
      trusted[PARQUET_SCHEMA_FINGERPRINT_METADATA_KEY]
  ) {
    throw new TypeError("schema metadata differs from the registered descriptor or fingerprint");
  }
}
