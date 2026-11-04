// Scam reports waiting for an administrator: the reported message, how
// many inboxes hold it, why it waits and some of the copies, then a
// decision that goes to the server.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/report', () => ({
  fetchPendingReports: vi.fn(),
  wireConfirmReport: vi.fn(),
  wireDismissReport: vi.fn(),
}))

import { AdminReports } from '@/pages/admin-reports'
import { fetchPendingReports, wireConfirmReport, wireDismissReport } from '@/wire/endpoints/report'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AdminReports />
    </QueryClientProvider>
  )
}

const outcome = {
  at: 1_793_700_000,
  decided_at: 0,
  grounds: [],
  inboxes: 2,
  message_id: 's1@parcel-desk.example',
  no_fingerprint: false,
  own_held: 0,
  report_id: 'rp1',
  state: 'waiting' as const,
  taken_back: 0,
  thread_id: 's1@parcel-desk.example',
  waits: ['no_ground'],
}

describe('scam reports', () => {
  it('shows what waits and takes the copies back on confirming', async () => {
    vi.mocked(fetchPendingReports).mockResolvedValue([
      {
        from: 'Parcel Desk <notice@parcel-desk.example>',
        outcome,
        reporter: 'alice@example.test',
        samples: [
          { from: 'Parcel Desk', subject: 'Your parcel is held', user: 'bob@example.test' },
        ],
        sender_domain: 'parcel-desk.example',
        subject: 'Your parcel is held',
      },
    ])
    vi.mocked(wireConfirmReport).mockResolvedValue({
      ...outcome,
      state: 'taken_back',
      taken_back: 2,
    })
    show()
    expect(await screen.findByText(/bob@example.test/)).toBeDefined()
    expect(screen.getByText(/parcel-desk.example ·/)).toBeDefined()
    expect(screen.getByText('One person reported it')).toBeDefined()
    fireEvent.click(screen.getByText('Take them back'))
    await waitFor(() => expect(wireConfirmReport).toHaveBeenCalledWith('rp1'))
    expect(wireDismissReport).not.toHaveBeenCalled()
  })

  it('says so when nothing waits', async () => {
    vi.mocked(fetchPendingReports).mockResolvedValue([])
    show()
    expect(await screen.findByText('No report waits')).toBeDefined()
  })
})
