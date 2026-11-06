// The sectioned inbox moved from this browser's local storage to the
// user's setting on the server. A choice the reader made here is written
// to the server once and then dropped; a reader who never chose gets the
// server's value, on by default; after the move the server's value wins.

import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/mail-settings', () => ({
  wireGetSectionedInbox: vi.fn(),
  wireSetSectionedInbox: vi.fn(),
}))

import { InboxSectionsCard } from '@/components/settings/inbox-sections-card'
import { useSectionedInbox } from '@/hooks/use-sectioned-inbox'
import { SECTIONED_LOCAL_KEY } from '@/lib/sectioned-inbox'
import { wireGetSectionedInbox, wireSetSectionedInbox } from '@/wire/endpoints/mail-settings'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

function freshClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

describe('the sectioned inbox setting', () => {
  it('writes a choice made in this browser to the server once, then drops it here', async () => {
    localStorage.setItem(SECTIONED_LOCAL_KEY, 'false')
    vi.mocked(wireSetSectionedInbox).mockResolvedValue({ enabled: false })
    vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: false })

    // off from the first render: the choice waiting to move is shown
    const first = renderHook(() => useSectionedInbox(), { wrapper: wrapper(freshClient()) })
    expect(first.result.current).toBe(false)
    await waitFor(() => expect(localStorage.getItem(SECTIONED_LOCAL_KEY)).toBeNull())
    expect(wireSetSectionedInbox).toHaveBeenCalledTimes(1)
    expect(wireSetSectionedInbox).toHaveBeenCalledWith({ enabled: false })
    expect(wireGetSectionedInbox).not.toHaveBeenCalled()
    expect(first.result.current).toBe(false)

    // the next load reads the server and writes nothing
    const next = renderHook(() => useSectionedInbox(), { wrapper: wrapper(freshClient()) })
    await waitFor(() => expect(wireGetSectionedInbox).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(next.result.current).toBe(false))
    expect(wireSetSectionedInbox).toHaveBeenCalledTimes(1)
  })

  it('writes nothing for a reader who never chose, and shows the sections', async () => {
    vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: true })
    const { result } = renderHook(() => useSectionedInbox(), { wrapper: wrapper(freshClient()) })
    expect(result.current).toBe(true)
    await waitFor(() => expect(wireGetSectionedInbox).toHaveBeenCalledTimes(1))
    expect(result.current).toBe(true)
    expect(wireSetSectionedInbox).not.toHaveBeenCalled()
  })

  it("follows the server's value once nothing is left to move", async () => {
    // turned off on a phone
    vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: false })
    const { result } = renderHook(() => useSectionedInbox(), { wrapper: wrapper(freshClient()) })
    await waitFor(() => expect(result.current).toBe(false))
    expect(wireSetSectionedInbox).not.toHaveBeenCalled()
  })

  it('keeps the choice here when the server did not take it, to move it next time', async () => {
    localStorage.setItem(SECTIONED_LOCAL_KEY, 'true')
    vi.mocked(wireSetSectionedInbox).mockRejectedValue(new Error('offline'))
    renderHook(() => useSectionedInbox(), { wrapper: wrapper(freshClient()) })
    await waitFor(() => expect(wireSetSectionedInbox).toHaveBeenCalledTimes(1))
    expect(localStorage.getItem(SECTIONED_LOCAL_KEY)).toBe('true')
  })

  it('the settings switch saves on the server and replaces a choice still kept here', async () => {
    vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: true })
    vi.mocked(wireSetSectionedInbox).mockResolvedValue({ enabled: false })
    render(<InboxSectionsCard />, { wrapper: wrapper(freshClient()) })
    await waitFor(() => expect(wireGetSectionedInbox).toHaveBeenCalledTimes(1))
    const toggle = screen.getByRole('switch')
    expect(toggle.getAttribute('aria-checked')).toBe('true')

    localStorage.setItem(SECTIONED_LOCAL_KEY, 'true')
    vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: false })
    fireEvent.click(toggle)
    await waitFor(() => expect(wireSetSectionedInbox).toHaveBeenCalledWith({ enabled: false }))
    await waitFor(() => expect(toggle.getAttribute('aria-checked')).toBe('false'))
    expect(localStorage.getItem(SECTIONED_LOCAL_KEY)).toBeNull()
  })
})
