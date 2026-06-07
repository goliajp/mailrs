// Lightweight runtime shape assertion for hot-path API responses.
//
// We deliberately do not use Zod here — at ~50-100μs per object × 200
// items × every conversation-list refresh, the cost adds up to single-
// digit milliseconds we'd rather spend on rendering. Instead we check the
// drift signals that actually matter:
//
//   1. The response is the expected container kind (array vs object).
//   2. If a non-empty array, the first item has every required top-level
//      key. (Backends usually homogenise array elements; one sample is
//      enough to catch a renamed field like `valkey` → `kevy`.)
//   3. If an object, it has every required top-level key.
//
// New fields are silently allowed (forward-compat). Renamed / removed
// required fields throw a structured ApiShapeError naming the path + the
// missing key — visible in the route-level ErrorBoundary or via the
// network log instead of a downstream `undefined.foo` deeper in the
// render tree.

export class ApiShapeError extends Error {
  readonly missing: string[]
  readonly path: string
  constructor(path: string, missing: string[]) {
    super(`API response shape drift at ${path}: missing required field(s): ${missing.join(', ')}`)
    this.name = 'ApiShapeError'
    this.path = path
    this.missing = missing
  }
}

export function assertArrayShape<T>(
  path: string,
  value: unknown,
  required: readonly (keyof T & string)[]
): T[] {
  if (!Array.isArray(value)) {
    throw new ApiShapeError(path, ['<not an array>'])
  }
  if (value.length === 0) return value as T[]
  // sample the first element only — backends emit homogeneous arrays
  const missing = objectMissing(value[0], required)
  if (missing.length > 0) throw new ApiShapeError(`${path}[0]`, missing)
  return value as T[]
}

export function assertObjectShape<T>(
  path: string,
  value: unknown,
  required: readonly (keyof T & string)[]
): T {
  const missing = objectMissing(value, required)
  if (missing.length > 0) throw new ApiShapeError(path, missing)
  return value as T
}

function objectMissing(value: unknown, required: readonly string[]): string[] {
  if (value === null || typeof value !== 'object') return [...required]
  return required.filter((k) => !(k in value))
}
