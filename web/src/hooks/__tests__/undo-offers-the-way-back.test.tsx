/**
 * Every one-gesture action offers a way back.
 *
 * A right swipe on a phone archives in one movement, and the only
 * route back was to go find the Archived list and unarchive by hand.
 * The toast component has taken an `action` since it was adopted —
 * 109 call sites, not one of them using it — while the inverse
 * mutation for each verb sits in the same hook that fires the verb.
 */

import { QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const toastCalls: Array<{ opts?: { action?: { label: string; onClick: () => void } } }> = []
vi.mock('@goliapkg/gds', async () => {
  const real = await vi.importActual<Record<string, unknown>>('@goliapkg/gds')
  return {
    ...real,
    toast: {
      error: vi.fn(),
      success: (_msg: string, opts?: { action?: { label: string; onClick: () => void } }) => {
        toastCalls.push({ opts })
      },
    },
  }
})

vi.mock('@/wire/endpoints/mutations', () => ({
  wireArchiveThread: vi.fn(() => Promise.resolve()),
  wireBatchMutation: vi.fn(() => Promise.resolve()),
  wireDeleteThread: vi.fn(() => Promise.resolve()),
  wireMarkJunk: vi.fn(() => Promise.resolve()),
  wireMarkNotification: vi.fn(() => Promise.resolve()),
  wireMarkNotJunk: vi.fn(() => Promise.resolve()),
  wireMarkPromotion: vi.fn(() => Promise.resolve()),
  wireMarkThreadRead: vi.fn(() => Promise.resolve()),
  wireMarkThreadUnread: vi.fn(() => Promise.resolve()),
  wireMoveToInbox: vi.fn(() => Promise.resolve()),
  wirePinThread: vi.fn(() => Promise.resolve()),
  wireStarThread: vi.fn(() => Promise.resolve()),
  wireUnarchiveThread: vi.fn(() => Promise.resolve()),
  wireUnpinThread: vi.fn(() => Promise.resolve()),
  wireUnstarThread: vi.fn(() => Promise.resolve()),
}))
vi.mock('@/lib/api', () => ({
  postJson: vi.fn(() => Promise.resolve({})),
  snoozeConversation: vi.fn(() => Promise.resolve()),
  unsnoozeConversation: vi.fn(() => Promise.resolve()),
}))

const { wireArchiveThread, wireMarkJunk, wireMarkNotJunk, wireUnarchiveThread } =
  await import('@/wire/endpoints/mutations')
const { queryClient } = await import('@/lib/query-client')
const { useConversationActions } = await import('@/hooks/use-conversation-actions')

function wrapper({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
}

beforeEach(() => {
  queryClient.clear()
  toastCalls.length = 0
  vi.clearAllMocks()
})

describe('undo', () => {
  it('archiving offers it, and taking it unarchives', async () => {
    const { result } = renderHook(() => useConversationActions(), { wrapper })
    await result.current.act('t1', 'archive')
    await waitFor(() => expect(wireArchiveThread).toHaveBeenCalledWith('t1'))
    await waitFor(() => expect(toastCalls.length).toBeGreaterThan(0))

    const action = toastCalls[toastCalls.length - 1].opts?.action
    expect(action?.label, 'archiving offered no way back').toBe('Undo')
    action?.onClick()
    await waitFor(() => expect(wireUnarchiveThread).toHaveBeenCalledWith('t1'))
  })

  it('junking offers it, and taking it puts the thread back', async () => {
    const { result } = renderHook(() => useConversationActions(), { wrapper })
    await result.current.act('t2', 'mark-junk')
    await waitFor(() => expect(wireMarkJunk).toHaveBeenCalledWith('t2'))
    await waitFor(() => expect(toastCalls.length).toBeGreaterThan(0))

    const action = toastCalls[toastCalls.length - 1].opts?.action
    expect(action?.label, 'junking offered no way back').toBe('Undo')
    action?.onClick()
    await waitFor(() => expect(wireMarkNotJunk).toHaveBeenCalledWith('t2'))
  })
})
