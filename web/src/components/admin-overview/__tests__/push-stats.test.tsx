import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { PushStats } from '../push-stats'

/** `GET /api/admin/push-stats` as `PushStats` serializes (handlers/admin_push_stats.rs). */
const ZERO = { failed: 0, sent: 0, suppressed_policy: 0, suppressed_quiet: 0 }
const STATS = {
  accounts: 12,
  days: [
    {
      apns: ZERO,
      day: '20261107',
      unifiedpush: ZERO,
      web: { failed: 1, sent: 2, suppressed_policy: 0, suppressed_quiet: 0 },
    },
    {
      apns: { ...ZERO, suppressed_policy: 3 },
      day: '20261106',
      unifiedpush: { failed: 0, sent: 3, suppressed_policy: 1, suppressed_quiet: 4 },
      web: { ...ZERO, sent: 5 },
    },
  ],
}

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <PushStats />
    </QueryClientProvider>
  )
}

function stubFetch(status: number, body: unknown): ReturnType<typeof vi.fn> {
  const fn = vi.fn(
    async () =>
      new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status,
      })
  )
  vi.stubGlobal('fetch', fn)
  return fn
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<PushStats />', () => {
  it('shows each channel per day with its two-week total, zeros as zeros', async () => {
    const fetch = stubFetch(200, STATS)
    renderIt()
    const web = await screen.findByRole('table', { name: 'Browsers (Web Push)' })
    const rows = within(web).getAllByRole('row')
    // header, two days, total
    expect(rows).toHaveLength(4)
    expect(rows[1]).toHaveTextContent('11-07')
    expect(
      within(rows[1])
        .getAllByRole('cell')
        .map((c) => c.textContent)
    ).toEqual(['2', '1', '0', '0'])
    expect(
      within(rows[3])
        .getAllByRole('cell')
        .map((c) => c.textContent)
    ).toEqual(['7', '1', '0', '0'])
    const up = screen.getByRole('table', { name: 'Android (UnifiedPush)' })
    const upTotal = within(up).getAllByRole('row')[3]
    expect(
      within(upTotal)
        .getAllByRole('cell')
        .map((c) => c.textContent)
    ).toEqual(['3', '0', '1', '4'])
    expect(screen.getByRole('table', { name: 'iPhone (APNs)' })).toBeInTheDocument()
    expect(screen.getByText('Summed over all 12 accounts. Days are UTC days')).toBeInTheDocument()
    expect(String(fetch.mock.calls[0][0])).toContain('/api/admin/push-stats?days=14')
  })

  it('says so when the counts cannot be read, rather than showing zeros', async () => {
    stubFetch(500, { error: 'kevy unreachable' })
    renderIt()
    expect(await screen.findByText(/The push counts could not be read/)).toBeInTheDocument()
    expect(screen.queryByRole('table')).toBeNull()
  })
})
