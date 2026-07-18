import { describe, expect, it } from 'vitest'

import { agentKeyListSchema, createdAgentKeySchema } from '../schemas/settings'

describe('agentKeyListSchema', () => {
  it('parses the actual backend row shape (scopes is an array)', () => {
    // Fixture captured 2026-07-18 from prod network kevy
    // `agent:keys:lihao@golia.jp` — the exact record whose array-typed
    // `scopes` broke the old z.string() declaration and blanked the
    // API Keys section.
    const raw = {
      items: [
        {
          created_at: 1784335208,
          id: 2,
          name: 'admin.golia.jp',
          prefix: 'mk_b04cf',
          scopes: [],
        },
        {
          created_at: 1784335185,
          id: 1,
          name: 'admin.golia.jp',
          prefix: 'mk_d21ab',
          scopes: [],
        },
      ],
    }
    const parsed = agentKeyListSchema.parse(raw)
    expect(parsed.items).toHaveLength(2)
    expect(parsed.items[0]).toMatchObject({
      created_at: '1784335208',
      id: '2',
      name: 'admin.golia.jp',
      prefix: 'mk_b04cf',
    })
  })

  it('accepts a bare array response', () => {
    const parsed = agentKeyListSchema.parse([
      { created_at: 1784335208, id: 2, name: 'k', prefix: 'mk_b04cf', scopes: ['mail.read'] },
    ])
    expect(parsed.items).toHaveLength(1)
  })
})

describe('createdAgentKeySchema', () => {
  it('maps backend {id, secret} to the UI {id, key, prefix} shape', () => {
    const parsed = createdAgentKeySchema.parse({
      id: 2,
      secret: 'mk_b04cf846421391acbc0b397a2a5292f6aee2f0bc8abc26d4',
    })
    expect(parsed.key).toBe('mk_b04cf846421391acbc0b397a2a5292f6aee2f0bc8abc26d4')
    expect(parsed.prefix).toBe('mk_b04cf')
    expect(parsed.id).toBe('2')
  })
})
