import { describe, expect, it, vi } from 'vitest'

import { fetchList, unwrapList } from '../api'

/**
 * The whole point of `fetchList` is that no matter what the backend
 * decides is a list, the caller gets a real `T[]`. Every shape the
 * webapi has ever returned (and every shape it might return without
 * telling anyone) must resolve to an array — never throw, never leak a
 * non-array.
 */
describe('unwrapList', () => {
  it('passes through a bare array', () => {
    expect(unwrapList([1, 2, 3])).toEqual([1, 2, 3])
  })

  it('extracts .items — the canonical `wire::*ListResponse` shape', () => {
    expect(unwrapList({ items: ['a', 'b'] })).toEqual(['a', 'b'])
  })

  it('extracts .results / .data / .list / .rows aliases', () => {
    expect(unwrapList({ results: [1] })).toEqual([1])
    expect(unwrapList({ data: [1] })).toEqual([1])
    expect(unwrapList({ list: [1] })).toEqual([1])
    expect(unwrapList({ rows: [1] })).toEqual([1])
  })

  it('discovers a single array-valued property when the field is renamed', () => {
    // Guard against a future wire type that names its list field after
    // the resource, e.g. `WebhookListResponse` → `{ webhooks: [] }`.
    expect(unwrapList({ webhooks: [{ id: 1 }] })).toEqual([{ id: 1 }])
    // Only kicks in when there's exactly one array property — no
    // ambiguity, no wrong guess.
    expect(unwrapList({ a: [1], b: [2] })).toEqual([])
  })

  it('returns [] for null / undefined / primitives / error envelopes', () => {
    expect(unwrapList(null)).toEqual([])
    expect(unwrapList(undefined)).toEqual([])
    expect(unwrapList(0)).toEqual([])
    expect(unwrapList('')).toEqual([])
    expect(unwrapList({ error: 'unauthorized' })).toEqual([])
    expect(unwrapList({ ok: false })).toEqual([])
  })
})

describe('fetchList', () => {
  it('resolves to [] on a 204 No Content (endpoints like /api/icon signal empty this way)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(null, { status: 204 }))
    )
    const list = await fetchList<number>('/some/list')
    expect(list).toEqual([])
  })

  it('unwraps a wire::*ListResponse-shaped 200 response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ items: [{ id: 1 }, { id: 2 }] }), {
            headers: { 'content-type': 'application/json' },
            status: 200,
          })
      )
    )
    const list = await fetchList<{ id: number }>('/admin/domains')
    expect(list).toEqual([{ id: 1 }, { id: 2 }])
  })

  it('accepts a bare array too (legacy monolith / mixed lanes)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify(['a', 'b']), {
            headers: { 'content-type': 'application/json' },
            status: 200,
          })
      )
    )
    const list = await fetchList<string>('/admin/permissions')
    expect(list).toEqual(['a', 'b'])
  })
})
