import { hasOnlyUnicodeScalars } from "./raw-frame.js";

type StringFormat = { type: "string"; validate: (value: string) => boolean };
type FormatRegistry = { addFormat: (name: string, format: StringFormat) => unknown };

/** Register assertion formats for the UTF-8 byte limits in the shared OpenAPI schema.
 * Generic OpenAPI tools may treat custom formats as annotations; these assertions
 * do not replace domain, source, receipt or provider-authority validation.
 */
export function registerOpenApiUtf8ByteFormats(registry: FormatRegistry): void {
  for (const maximum of [128, 256, 512]) {
    registry.addFormat(`utf8-bytes-${maximum}`, {
      type: "string",
      validate: (value: string) => value.length <= maximum &&
        hasOnlyUnicodeScalars(value) && new TextEncoder().encode(value).byteLength <= maximum,
    });
  }
}
