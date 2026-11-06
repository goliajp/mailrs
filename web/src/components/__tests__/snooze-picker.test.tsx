// Snoozing asks when, with the four choices iOS offers, instead of always
// putting the conversation away until a fixed hour tomorrow.

import { QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from '@testing-library/react'
import { getDefaultStore } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const toastCalls: string[] = []
vi.mock('@goliapkg/gds', async () => {
  const real = await vi.importActual<Record<string, unknown>>('@goliapkg/gds')
  return { ...real, toast: { error: vi.fn(), success: (m: string) => toastCalls.push(m) } }
})
vi.mock('@/lib/api', () => ({
  postJson: vi.fn(() => Promise.resolve({})),
  snoozeConversation: vi.fn(() => Promise.resolve()),
  unsnoozeConversation: vi.fn(() => Promise.resolve()),
}))

const { snoozeConversation } = await import('@/lib/api')
const { queryClient } = await import('@/lib/query-client')
const { useConversationActions } = await import('@/hooks/use-conversation-actions')
const { SnoozePicker } = await import('@/components/snooze-picker')
const { snoozePickerAtom } = await import('@/store/ui')
const { snoozeSeconds } = await import('@/lib/snooze-choice')

function wrapper({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
}

beforeEach(() => {
  queryClient.clear()
  toastCalls.length = 0
  vi.clearAllMocks()
  getDefaultStore().set(snoozePickerAtom, null)
})
afterEach(cleanup)

describe('snooze', () => {
  it('asks when before snoozing anything', async () => {
    const { result } = renderHook(() => useConversationActions(), { wrapper })
    await act(() => result.current.act('t1', 'snooze'))
    expect(getDefaultStore().get(snoozePickerAtom)).toEqual({ threadId: 't1' })
    expect(snoozeConversation).not.toHaveBeenCalled()
  })

  it('offers the four times and snoozes until the one picked', async () => {
    getDefaultStore().set(snoozePickerAtom, { threadId: 't1' })
    render(<SnoozePicker />, { wrapper })
    const choices = screen.getAllByRole('button').map((b) => b.textContent)
    expect(choices).toEqual(['Later today', 'Tomorrow morning', 'This weekend', 'Next week'])

    const expected = snoozeSeconds('nextWeek', new Date())
    fireEvent.click(screen.getByRole('button', { name: 'Next week' }))
    await waitFor(() => expect(snoozeConversation).toHaveBeenCalledWith('t1', expected))
    expect(getDefaultStore().get(snoozePickerAtom)).toBeNull()
    await waitFor(() => expect(toastCalls[0]).toMatch(/^Snoozed until /))
  })
})
