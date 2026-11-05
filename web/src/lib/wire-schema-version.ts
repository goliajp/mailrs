/**
 * Which wire shapes the persisted query cache was written with.
 *
 * The cache in localStorage holds parsed responses. One written before a
 * schema gained a field comes back without it, and a reader that assumes
 * the field is there breaks; a query that never goes stale keeps the old
 * shape until the cache is dropped. So the cache is dropped when the
 * shapes change, and only then, not on every deploy.
 *
 * `WIRE_SCHEMAS_HASH` is the hash of `src/wire/schemas/*.ts`; a test
 * recomputes it and fails when a schema changed without this version
 * moving. Bump the version, then copy the hash the test prints.
 */
export const WIRE_SCHEMA_VERSION = 10

export const WIRE_SCHEMAS_HASH = '7a57e8da'

/** FNV-1a over the files' names and contents, in name order. */
export function hashSchemas(files: Record<string, string>): string {
  let h = 0x811c9dc5
  for (const name of Object.keys(files).sort()) {
    for (const ch of `${name}\n${files[name]}\n`) {
      h ^= ch.codePointAt(0) ?? 0
      h = Math.imul(h, 0x01000193) >>> 0
    }
  }
  return h.toString(16).padStart(8, '0')
}
