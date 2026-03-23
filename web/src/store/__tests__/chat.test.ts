import { describe, expect, it } from 'vitest'
import { createStore } from 'jotai/vanilla'

import type { ConversationSummary } from '@/lib/types'
import {
  batchModeAtom,
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
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
  unreadCountAtom,
} from '../chat'

function makeConversation(overrides: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    thread_id: 'thread-1',
    subject: 'Test subject',
    participants: ['alice@example.com'],
    message_count: 1,
    unread_count: 0,
    last_date: Date.now(),
    category: 'inbox',
    flagged: false,
    snippet: '',
    pinned: false,
    archived: false,
    importance_level: 'normal',
    importance_score: 0.3,
    requires_action: false,
    ...overrides,
  }
}

describe('unreadCountAtom', () => {
  it('returns 0 when conversations list is empty', () => {
    const store = createStore()
    expect(store.get(unreadCountAtom)).toBe(0)
  })

  it('sums unread_count across all conversations', () => {
    const store = createStore()
    store.set(conversationsAtom, [
      makeConversation({ thread_id: 't1', unread_count: 3 }),
      makeConversation({ thread_id: 't2', unread_count: 5 }),
      makeConversation({ thread_id: 't3', unread_count: 0 }),
    ])
    expect(store.get(unreadCountAtom)).toBe(8)
  })

  it('updates reactively when conversations change', () => {
    const store = createStore()
    store.set(conversationsAtom, [makeConversation({ thread_id: 't1', unread_count: 2 })])
    expect(store.get(unreadCountAtom)).toBe(2)

    store.set(conversationsAtom, [
      makeConversation({ thread_id: 't1', unread_count: 2 }),
      makeConversation({ thread_id: 't2', unread_count: 7 }),
    ])
    expect(store.get(unreadCountAtom)).toBe(9)
  })

  it('returns 0 when all conversations have 0 unread', () => {
    const store = createStore()
    store.set(conversationsAtom, [
      makeConversation({ thread_id: 't1', unread_count: 0 }),
      makeConversation({ thread_id: 't2', unread_count: 0 }),
    ])
    expect(store.get(unreadCountAtom)).toBe(0)
  })

  it('handles single conversation correctly', () => {
    const store = createStore()
    store.set(conversationsAtom, [makeConversation({ thread_id: 't1', unread_count: 12 })])
    expect(store.get(unreadCountAtom)).toBe(12)
  })
})

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
