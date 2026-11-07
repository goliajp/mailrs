import { QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import { getDefaultStore } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

import history from '@/components/admin-monitor/__tests__/fixtures/daemon-history.json'
import daemon from '@/components/admin-monitor/__tests__/fixtures/daemon-state.json'
import { chosenLangAtom } from '@/i18n'
import { queryClient } from '@/lib/query-client'

import { AdminMonitor } from '../admin-monitor'

// the daemon answers were captured from a real `mailrs-ops` (its /v1/state
// and /v1/history); webapi wraps the state with whether it reached it
const STATE = '/api/admin/ops/state'
const HISTORY = '/api/admin/ops/history'

function reached(change: (d: typeof daemon) => object = (d) => d, stale = false) {
  return { reach: 'ok', silent_after_secs: 600, stale, state: change(structuredClone(daemon)) }
}

function stub(state: unknown) {
  const calls: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      calls.push(path)
      let body: unknown = {}
      if (path.startsWith(STATE)) body = state
      if (path.startsWith(HISTORY)) body = history
      return new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
  queryClient.clear()
  getDefaultStore().set(chosenLangAtom, null)
})

function renderPage() {
  render(
    <QueryClientProvider client={queryClient}>
      <AdminMonitor />
    </QueryClientProvider>
  )
}

describe('admin monitoring page', () => {
  it('lists the alerts that are not ok, with the first step', async () => {
    stub(reached())
    renderPage()
    const table = await screen.findByRole('table', { name: 'Current alerts' })
    const row = within(table).getByText('Nothing outside watches the ops daemon').closest('tr')
    expect(row).not.toBeNull()
    const cells = within(row as HTMLElement)
    expect(cells.getByText('Firing')).toBeTruthy()
    expect(cells.getByText('Configure an external "alert when silent" check URL')).toBeTruthy()
    // an ok rule is not listed
    expect(within(table).queryByText('Mail was quarantined')).toBeNull()
    expect(screen.getByText(/Last completed check:/)).toBeTruthy()
    // the history: the last day's changes and the silence the capture set
    await screen.findByText('Recent changes, last day')
    expect(screen.getByText(/set by cli:capture: known file/)).toBeTruthy()
  })

  it('gives the first step in the interface language', async () => {
    getDefaultStore().set(chosenLangAtom, 'ja')
    stub(reached())
    renderPage()
    await screen.findByRole('table', { name: '現在のアラート' })
    expect(
      screen.getByText('外部の「一定時間届かなければ通知する」チェックの URL を設定します')
    ).toBeTruthy()
  })

  it('says there are no alerts when every rule is ok', async () => {
    stub(reached((d) => ({ ...d, alerts: d.alerts.map((a) => ({ ...a, phase: 'ok' })) })))
    renderPage()
    await screen.findByText('No alerts')
    expect(screen.queryByRole('table', { name: 'Current alerts' })).toBeNull()
  })

  it('says plainly when webapi cannot reach the daemon, with no list', async () => {
    const calls = stub({
      detail: 'error sending request: connection refused',
      reach: 'unreachable',
    })
    renderPage()
    await screen.findByText('webapi cannot reach the ops daemon')
    expect(screen.getByText(/connection refused/)).toBeTruthy()
    expect(screen.queryByText('No alerts')).toBeNull()
    expect(screen.queryByText('Silences')).toBeNull()
    expect(calls.some((c) => c.startsWith(HISTORY))).toBe(false)
  })

  it('says so when the daemon is not configured', async () => {
    stub({ reach: 'not_configured' })
    renderPage()
    await screen.findByText('The ops daemon is not configured')
    expect(screen.queryByText('No alerts')).toBeNull()
  })

  it('warns when the last completed check is older than the silent limit', async () => {
    stub(reached((d) => d, true))
    renderPage()
    await screen.findByText(/The ops daemon's last completed check was at/)
    expect(screen.getByText(/MAILRS_OPS_SILENT_SECS \(600 seconds\)/)).toBeTruthy()
  })

  it('shows no stale banner while the daemon keeps checking', async () => {
    stub(reached())
    renderPage()
    await screen.findByRole('table', { name: 'Current alerts' })
    expect(screen.queryByText(/The ops daemon's last completed check was at/)).toBeNull()
  })

  it('says what is lost without an external channel or outside watch', async () => {
    stub(reached())
    renderPage()
    await screen.findByText('No external notification channel is configured')
    expect(
      screen.getByText('If mailrs itself goes down, nobody will receive an alert')
    ).toBeTruthy()
    expect(screen.getByText('If the whole machine fails, nobody will notice')).toBeTruthy()
    expect(screen.getByText(/MAILRS_OPS_HEARTBEAT_URL is not set/)).toBeTruthy()
  })

  it('drops those warnings once a channel and an outside watch are configured', async () => {
    stub(reached((d) => ({ ...d, external_channel: true, unmonitored: false })))
    renderPage()
    await screen.findByRole('table', { name: 'Current alerts' })
    expect(screen.queryByText('No external notification channel is configured')).toBeNull()
    expect(screen.queryByText(/MAILRS_OPS_HEARTBEAT_URL is not set/)).toBeNull()
  })

  it('says CSP counting has not started instead of showing zeros', async () => {
    stub(reached())
    renderPage()
    await screen.findByText(/Counting has not started/)
    expect(screen.queryByRole('table', { name: 'CSP report counts' })).toBeNull()
  })

  it('shows the CSP counts of the last 7 days and since counting began', async () => {
    const entry = {
      blocked: 'https://cdn.example.com',
      client: 'android-webview',
      count: 3,
      directive: 'img-src',
      first_at: daemon.now - 3600,
      last_at: daemon.now - 60,
      page: 'mail',
      violated_directive: '',
    }
    const totals = (reports: number, distinct: (typeof entry)[], pages: number) => ({
      by_directive: { 'img-src': reports },
      distinct,
      distinct_dropped: 0,
      from: daemon.now - 7 * 86_400,
      pages_served: { desktop: pages },
      reports,
      to: daemon.now,
    })
    const csp = {
      counting_since: daemon.now - 3 * 86_400,
      last_7_days: totals(3, [entry], 40),
      since_counting: totals(5, [entry], 90),
    }
    stub(reached((d) => ({ ...d, csp })))
    renderPage()
    const counts = await screen.findByRole('table', { name: 'CSP report counts' })
    const row = within(counts).getByText('All CSP reports').closest('tr') as HTMLElement
    expect(within(row).getByText('3')).toBeTruthy()
    expect(within(row).getByText('5')).toBeTruthy()
    const pages = within(counts).getByText('Pages served to Desktop browser').closest('tr')
    expect(within(pages as HTMLElement).getByText('90')).toBeTruthy()
    // under a week of counting, the page says the week is not full
    expect(screen.getByText(/less than 7 full days/)).toBeTruthy()
    const distinct = screen.getByRole('table', { name: 'Distinct reports, last 7 days' })
    expect(within(distinct).getByText('https://cdn.example.com')).toBeTruthy()
    expect(within(distinct).getByText('/mail')).toBeTruthy()
    expect(within(distinct).getByText('Android in-app web view')).toBeTruthy()
    // the browser sent no violated directive: unknown, not blank
    expect(within(distinct).getByText('—')).toBeTruthy()
  })
})
