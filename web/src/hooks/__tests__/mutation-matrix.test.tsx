/**
 * Cross-screen mutation matrix — v2.7.3 §Phase 12 §12.8.
 *
 * Every mail mutation runs an optimistic-patch + rollback dance over
 * EVERY conversations cache line (legacy `mailKeys.conversations()`
 * prefix + v2.1 `conversationKeys.infinites()` prefix). A mutation
 * fired from any screen must patch the caches that every other
 * screen reads — that is the cross-screen consistency contract this
 * matrix locks down.
 *
 * Screens simulated as cache lines:
 *   - inbox infinite list   (mail-list screen)
 *   - junk infinite list    (junk-view screen)
 *   - legacy conversations  (dashboard / not-yet-migrated callers)
 *
 * For each mutation the matrix asserts:
 *   1. optimistic patch lands on ALL cache lines (field change or drop)
 *   2. rollback on wire error restores every line byte-identical
 *      (except mark-read, which deliberately keeps the patch — see
 *      the Gmail-style-continuity comment in use-mail-mutations.ts)
 */

import type { ConversationSummary } from '@/lib/types'

import { QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'

import {
  useArchiveMutation,
  useDeleteMutation,
  useMarkJunkMutation,
  useMarkNotJunkMutation,
  useMarkReadMutation,
  useMarkUnreadMutation,
  usePinMutation,
  useSnoozeMutation,
  useStarMutation,
  useUnarchiveMutation,
  useUnpinMutation,
  useUnstarMutation,
} from '../use-mail-mutations'

// Wire adapter is mocked per-test: resolve for the patch assertions,
// reject for the rollback assertions.
vi.mock('@/wire/endpoints/mutations', () => ({
  wireArchiveThread: vi.fn(),
  wireBatchMutation: vi.fn(),
  wireDeleteThread: vi.fn(),
  wireMarkJunk: vi.fn(),
  wireMarkNotJunk: vi.fn(),
  wireMarkThreadRead: vi.fn(),
  wireMarkThreadUnread: vi.fn(),
  wirePinThread: vi.fn(),
  wireStarThread: vi.fn(),
  wireUnarchiveThread: vi.fn(),
  wireUnpinThread: vi.fn(),
  wireUnstarThread: vi.fn(),
}))
vi.mock('@/lib/api', () => ({
  snoozeConversation: vi.fn(),
  unsnoozeConversation: vi.fn(),
}))

import { snoozeConversation } from '@/lib/api'
import * as wire from '@/wire/endpoints/mutations'

function makeConvo(id: string, over: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    archived: false,
    category: 'inbox',
    flagged: false,
    importance_level: 'normal',
    importance_score: 0,
    last_date: 100,
    last_sender: 'a@x',
    message_count: 1,
    participants: [],
    pinned: false,
    received_count: 1,
    requires_action: false,
    sent_count: 0,
    snippet: '',
    subject: 'hi',
    thread_id: id,
    unread_count: 2,
    ...over,
  } as ConversationSummary
}

// The three "screens" — distinct cache lines the matrix checks.
const SCREEN_KEYS = [
  conversationKeys.infinite({}), // mail-list (inbox)
  conversationKeys.infinite({ folder: 'junk' } as never), // junk-view
  mailKeys.conversations(), // legacy dashboard callers
] as const

type Pages = { pageParams: (number | undefined)[]; pages: ConversationSummary[][] }

function findRow(key: readonly unknown[], threadId: string): ConversationSummary | undefined {
  const data = queryClient.getQueryData<Pages>(key)
  return data?.pages.flat().find((c) => c.thread_id === threadId)
}

function seedAllScreens(threadId: string) {
  for (const key of SCREEN_KEYS) {
    queryClient.setQueryData<Pages>(key, {
      pageParams: [undefined],
      pages: [[makeConvo(threadId), makeConvo('other-thread')]],
    })
  }
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
}

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
})

afterEach(() => {
  queryClient.clear()
})

// ── matrix: field-patch mutations ─────────────────────────────────
//
// Each row: [name, hook, wire fn to mock, assertion over the patched row]

const FIELD_MATRIX = [
  {
    hook: useArchiveMutation,
    name: 'archive',
    assert: (c: ConversationSummary | undefined) => expect(c?.archived).toBe(true),
    wireFn: () => wire.wireArchiveThread,
  },
  {
    hook: useUnarchiveMutation,
    name: 'unarchive',
    assert: (c: ConversationSummary | undefined) => expect(c?.archived).toBe(false),
    wireFn: () => wire.wireUnarchiveThread,
  },
  {
    hook: useStarMutation,
    name: 'star',
    assert: (c: ConversationSummary | undefined) => expect(c?.flagged).toBe(true),
    wireFn: () => wire.wireStarThread,
  },
  {
    hook: useUnstarMutation,
    name: 'unstar',
    assert: (c: ConversationSummary | undefined) => expect(c?.flagged).toBe(false),
    wireFn: () => wire.wireUnstarThread,
  },
  {
    hook: usePinMutation,
    name: 'pin',
    assert: (c: ConversationSummary | undefined) => expect(c?.pinned).toBe(true),
    wireFn: () => wire.wirePinThread,
  },
  {
    hook: useUnpinMutation,
    name: 'unpin',
    assert: (c: ConversationSummary | undefined) => expect(c?.pinned).toBe(false),
    wireFn: () => wire.wireUnpinThread,
  },
  {
    hook: useMarkReadMutation,
    name: 'mark-read',
    assert: (c: ConversationSummary | undefined) => expect(c?.unread_count).toBe(0),
    wireFn: () => wire.wireMarkThreadRead,
  },
  {
    hook: useMarkUnreadMutation,
    name: 'mark-unread',
    assert: (c: ConversationSummary | undefined) =>
      expect(c?.unread_count).toBeGreaterThanOrEqual(1),
    wireFn: () => wire.wireMarkThreadUnread,
  },
] as const

// ── matrix: drop mutations (row removed from every list) ──────────

const DROP_MATRIX = [
  { hook: useDeleteMutation, name: 'delete', wireFn: () => wire.wireDeleteThread },
  { hook: useMarkJunkMutation, name: 'mark-junk', wireFn: () => wire.wireMarkJunk },
  { hook: useMarkNotJunkMutation, name: 'mark-not-junk', wireFn: () => wire.wireMarkNotJunk },
] as const

describe('mutation matrix — optimistic patch reaches every screen', () => {
  for (const row of FIELD_MATRIX) {
    it(`${row.name}: patches all three screen caches`, async () => {
      seedAllScreens('t-target')
      vi.mocked(row.wireFn()).mockResolvedValue(undefined as never)
      const { result } = renderHook(() => row.hook(), { wrapper })
      result.current.mutate({ threadId: 't-target' } as never)
      await waitFor(() => expect(result.current.isSuccess).toBe(true))
      for (const key of SCREEN_KEYS) {
        row.assert(findRow(key, 't-target'))
        // Bystander row untouched.
        expect(findRow(key, 'other-thread')?.unread_count).toBe(2)
      }
    })
  }

  for (const row of DROP_MATRIX) {
    it(`${row.name}: drops the row from all three screen caches`, async () => {
      seedAllScreens('t-target')
      vi.mocked(row.wireFn()).mockResolvedValue(undefined as never)
      const { result } = renderHook(() => row.hook(), { wrapper })
      result.current.mutate({ threadId: 't-target' })
      await waitFor(() => expect(result.current.isSuccess).toBe(true))
      for (const key of SCREEN_KEYS) {
        expect(findRow(key, 't-target')).toBeUndefined()
        expect(findRow(key, 'other-thread')).toBeDefined()
      }
    })
  }

  it('snooze: drops the row from all three screen caches', async () => {
    seedAllScreens('t-target')
    vi.mocked(snoozeConversation).mockResolvedValue(undefined as never)
    const { result } = renderHook(() => useSnoozeMutation(), { wrapper })
    result.current.mutate({ threadId: 't-target', until: '2026-08-01T00:00:00Z' })
    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    for (const key of SCREEN_KEYS) {
      expect(findRow(key, 't-target')).toBeUndefined()
    }
  })
})

describe('mutation matrix — rollback on wire error restores every screen', () => {
  const ROLLBACK_ROWS = [
    ...FIELD_MATRIX.filter((r) => r.name !== 'mark-read'),
    ...DROP_MATRIX,
  ] as const

  for (const row of ROLLBACK_ROWS) {
    it(`${row.name}: error rolls every cache line back`, async () => {
      seedAllScreens('t-target')
      const before = SCREEN_KEYS.map((key) => queryClient.getQueryData<Pages>(key))
      vi.mocked(row.wireFn()).mockRejectedValue(new Error('wire down'))
      const { result } = renderHook(() => row.hook(), { wrapper })
      result.current.mutate({ threadId: 't-target' } as never)
      await waitFor(() => expect(result.current.isError).toBe(true))
      SCREEN_KEYS.forEach((key, i) => {
        expect(queryClient.getQueryData<Pages>(key)).toEqual(before[i])
      })
    })
  }

  it('mark-read: error deliberately KEEPS the optimistic patch (Gmail-style continuity)', async () => {
    seedAllScreens('t-target')
    vi.mocked(wire.wireMarkThreadRead).mockRejectedValue(new Error('wire down'))
    const { result } = renderHook(() => useMarkReadMutation(), { wrapper })
    result.current.mutate({ threadId: 't-target' })
    await waitFor(() => expect(result.current.isError).toBe(true))
    for (const key of SCREEN_KEYS) {
      // No rollback: unread_count stays 0 so the auto-mark effect
      // doesn't re-fire in a loop against a down network.
      expect(findRow(key, 't-target')?.unread_count).toBe(0)
    }
  })
})
