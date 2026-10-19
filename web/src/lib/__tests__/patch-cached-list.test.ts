import { QueryClient } from '@tanstack/react-query'
import { describe, expect, it } from 'vitest'

import { patchCachedList } from '@/lib/patch-cached-list'

const KEY = ['conversation', 'infinite', { folder: 'NP' }]

function cached(qc: QueryClient) {
  const q = qc.getQueryCache().find({ exact: true, queryKey: KEY })
  if (!q) throw new Error('not cached')
  return q.state
}

describe('patchCachedList', () => {
  it('keeps a list invalidated by new mail invalidated', async () => {
    const qc = new QueryClient()
    qc.setQueryData(KEY, ['old'], { updatedAt: 1_000 })
    await qc.invalidateQueries({ exact: true, queryKey: KEY, refetchType: 'none' })
    patchCachedList(qc, KEY, ['old, read'])
    expect(cached(qc).data).toEqual(['old, read'])
    expect(cached(qc).isInvalidated).toBe(true)
    expect(cached(qc).dataUpdatedAt).toBe(1_000)
  })

  it('does not make a list look fetched when it was only edited', () => {
    const qc = new QueryClient()
    qc.setQueryData(KEY, ['old'], { updatedAt: 1_000 })
    patchCachedList(qc, KEY, ['old, read'])
    expect(cached(qc).isInvalidated).toBe(false)
    expect(cached(qc).dataUpdatedAt).toBe(1_000)
  })
})
