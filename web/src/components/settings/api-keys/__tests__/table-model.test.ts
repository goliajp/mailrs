import type { AgentKey } from '../../_shared'

import { describe, expect, it } from 'vitest'

import {
  clampPage,
  filterKeys,
  pageCount,
  pageSlice,
  parsePageSize,
  parseSortDir,
  parseSortField,
  sortKeys,
} from '../table-model'

function key(over: Partial<AgentKey> & { id: string }): AgentKey {
  return {
    created_at: '1784335208',
    name: 'k',
    prefix: 'mk_00000',
    scopes: [],
    ...over,
  }
}

// Fixture shape captured from the prod `agent:keys:<user>` hash
// (crates/webapi/src/handlers/complete.rs:1335).
const ROWS: AgentKey[] = [
  key({ created_at: '1784335208', id: '2', name: 'admin.golia.jp', prefix: 'mk_b04cf' }),
  key({ created_at: '1784335185', id: '1', name: 'admin.golia.jp', prefix: 'mk_d21ab' }),
  key({ created_at: '1784600000', id: '3', name: 'devops', prefix: 'mk_13b99' }),
  key({
    created_at: '1784700000',
    id: '4',
    name: 'sms',
    prefix: 'mk_9bfe8',
    scopes: ['mail:read'],
  }),
]

describe('filterKeys', () => {
  it('matches name, prefix, scope and id, case-insensitively', () => {
    expect(filterKeys(ROWS, 'DEVOPS').map((r) => r.id)).toEqual(['3'])
    expect(filterKeys(ROWS, 'mk_d21').map((r) => r.id)).toEqual(['1'])
    expect(filterKeys(ROWS, 'mail:read').map((r) => r.id)).toEqual(['4'])
    expect(filterKeys(ROWS, '  ').map((r) => r.id)).toEqual(['2', '1', '3', '4'])
  })
})

describe('sortKeys', () => {
  it('sorts by created descending by default direction', () => {
    expect(sortKeys(ROWS, 'created', 'desc').map((r) => r.id)).toEqual(['4', '3', '2', '1'])
    expect(sortKeys(ROWS, 'created', 'asc').map((r) => r.id)).toEqual(['1', '2', '3', '4'])
  })

  it('breaks same-second ties on the id so paging is stable', () => {
    const collide = [
      key({ created_at: '1784335208', id: '9', name: 'b' }),
      key({ created_at: '1784335208', id: '7', name: 'a' }),
      key({ created_at: '1784335208', id: '8', name: 'c' }),
    ]
    expect(sortKeys(collide, 'created', 'desc').map((r) => r.id)).toEqual(['7', '8', '9'])
    expect(sortKeys(collide, 'created', 'asc').map((r) => r.id)).toEqual(['7', '8', '9'])
  })

  it('sorts by name and leaves the input untouched', () => {
    const before = ROWS.map((r) => r.id)
    expect(sortKeys(ROWS, 'name', 'asc').map((r) => r.name)).toEqual([
      'admin.golia.jp',
      'admin.golia.jp',
      'devops',
      'sms',
    ])
    expect(ROWS.map((r) => r.id)).toEqual(before)
  })

  it('treats an unparseable created_at as epoch zero rather than NaN', () => {
    const rows = [key({ created_at: '', id: '1' }), key({ created_at: '1784335208', id: '2' })]
    expect(sortKeys(rows, 'created', 'desc').map((r) => r.id)).toEqual(['2', '1'])
  })
})

describe('paging', () => {
  it('counts pages and slices them', () => {
    expect(pageCount(4, 2)).toBe(2)
    expect(pageCount(0, 25)).toBe(1)
    expect(pageSlice(ROWS, 2, 2).map((r) => r.id)).toEqual(['3', '4'])
  })

  it('clamps out-of-range pages into the valid window', () => {
    expect(clampPage(99, 4, 2)).toBe(2)
    expect(clampPage(0, 4, 2)).toBe(1)
    expect(clampPage(Number.NaN, 4, 2)).toBe(1)
  })
})

describe('url param parsing', () => {
  it('falls back to defaults for absent or bogus values', () => {
    expect(parseSortField(null)).toBe('created')
    expect(parseSortField('bogus')).toBe('created')
    expect(parseSortField('name')).toBe('name')
    expect(parseSortDir(null)).toBe('desc')
    expect(parseSortDir('asc')).toBe('asc')
    expect(parsePageSize('7')).toBe(25)
    expect(parsePageSize('50')).toBe(50)
  })
})
