import { describe, expect, it } from 'vitest'

import { hashSchemas, WIRE_SCHEMA_VERSION, WIRE_SCHEMAS_HASH } from '@/lib/wire-schema-version'

const schemas = import.meta.glob<string>('../../wire/schemas/*.ts', {
  eager: true,
  import: 'default',
  query: '?raw',
})

describe('the persisted cache version', () => {
  it('moves whenever a wire schema changes', () => {
    expect(Object.keys(schemas).length).toBeGreaterThan(10)
    const hash = hashSchemas(schemas)
    expect(
      hash,
      `a schema in src/wire/schemas changed: bump WIRE_SCHEMA_VERSION (now ${WIRE_SCHEMA_VERSION}) and set WIRE_SCHEMAS_HASH to '${hash}'`
    ).toBe(WIRE_SCHEMAS_HASH)
  })

  it('reads a change in any one file', () => {
    const a = hashSchemas({ 'a.ts': 'x', 'b.ts': 'y' })
    expect(hashSchemas({ 'a.ts': 'x', 'b.ts': 'z' })).not.toBe(a)
    expect(hashSchemas({ 'a.ts': 'x', 'b.ts': 'y' })).toBe(a)
  })
})
