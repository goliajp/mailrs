import { createStore } from 'jotai/vanilla'
import { describe, expect, it } from 'vitest'

import {
  batchModeAtom,
  categoryFilterAtom,
  composingNewAtom,
  hasMoreAtom,
  initialLoadingAtom,
  loadingMoreAtom,
  mobileViewAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  selectedThreadIdsAtom,
  shortcutsDialogOpenAtom,
  sortOrderAtom,
  threadMessagesAtom,
} from '../chat'

// v2.1 phase-5d: `unreadCountAtom` deleted. The equivalent logic lives
// in `hooks/use-current-mail-filters::useCurrentUnreadCount`, which
// derives from the RQ-native `useFlatConversations` hook. The
// derivation semantics haven't changed; only the source of truth did.

describe('sortOrderAtom', () => {
  it('defaults to "newest"', () => {
    const store = createStore()
    expect(store.get(sortOrderAtom)).toBe('newest')
  })

  it('can be set to "oldest"', () => {
    const store = createStore()
    store.set(sortOrderAtom, 'oldest')
    expect(store.get(sortOrderAtom)).toBe('oldest')
  })

  it('can be set to "unread"', () => {
    const store = createStore()
    store.set(sortOrderAtom, 'unread')
    expect(store.get(sortOrderAtom)).toBe('unread')
  })
})

describe('primitive atoms — initial values', () => {
  it('selectedThreadIdAtom defaults to null', () => {
    const store = createStore()
    expect(store.get(selectedThreadIdAtom)).toBeNull()
  })

  it('threadMessagesAtom defaults to empty array', () => {
    const store = createStore()
    expect(store.get(threadMessagesAtom)).toEqual([])
  })

  it('composingNewAtom defaults to false', () => {
    const store = createStore()
    expect(store.get(composingNewAtom)).toBe(false)
  })

  it('searchQueryAtom defaults to empty string', () => {
    const store = createStore()
    expect(store.get(searchQueryAtom)).toBe('')
  })

  it('hasMoreAtom defaults to true', () => {
    const store = createStore()
    expect(store.get(hasMoreAtom)).toBe(true)
  })

  it('loadingMoreAtom defaults to false', () => {
    const store = createStore()
    expect(store.get(loadingMoreAtom)).toBe(false)
  })

  it('categoryFilterAtom defaults to null', () => {
    const store = createStore()
    expect(store.get(categoryFilterAtom)).toBeNull()
  })

  it('selectedDomainsAtom defaults to empty array', () => {
    const store = createStore()
    expect(store.get(selectedDomainsAtom)).toEqual([])
  })

  it('initialLoadingAtom defaults to true', () => {
    const store = createStore()
    expect(store.get(initialLoadingAtom)).toBe(true)
  })

  it('mobileViewAtom defaults to "list"', () => {
    const store = createStore()
    expect(store.get(mobileViewAtom)).toBe('list')
  })

  it('batchModeAtom defaults to false', () => {
    const store = createStore()
    expect(store.get(batchModeAtom)).toBe(false)
  })

  it('selectedThreadIdsAtom defaults to empty Set', () => {
    const store = createStore()
    expect(store.get(selectedThreadIdsAtom)).toEqual(new Set())
  })

  it('shortcutsDialogOpenAtom defaults to false', () => {
    const store = createStore()
    expect(store.get(shortcutsDialogOpenAtom)).toBe(false)
  })
})

describe('primitive atoms — writability', () => {
  it('selectedThreadIdAtom can be set to a thread id', () => {
    const store = createStore()
    store.set(selectedThreadIdAtom, 'thread-xyz')
    expect(store.get(selectedThreadIdAtom)).toBe('thread-xyz')
  })

  it('searchQueryAtom can be updated', () => {
    const store = createStore()
    store.set(searchQueryAtom, 'invoice')
    expect(store.get(searchQueryAtom)).toBe('invoice')
  })

  it('categoryFilterAtom can be set to a category string', () => {
    const store = createStore()
    store.set(categoryFilterAtom, 'newsletter')
    expect(store.get(categoryFilterAtom)).toBe('newsletter')
  })

  it('mobileViewAtom can switch to "thread"', () => {
    const store = createStore()
    store.set(mobileViewAtom, 'thread')
    expect(store.get(mobileViewAtom)).toBe('thread')
  })

  it('selectedThreadIdsAtom can hold multiple ids', () => {
    const store = createStore()
    const ids = new Set(['t1', 't2', 't3'])
    store.set(selectedThreadIdsAtom, ids)
    expect(store.get(selectedThreadIdsAtom)).toEqual(ids)
  })

  it('each store instance is isolated', () => {
    const storeA = createStore()
    const storeB = createStore()
    storeA.set(sortOrderAtom, 'oldest')
    expect(storeB.get(sortOrderAtom)).toBe('newest')
  })
})
