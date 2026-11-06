import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Calendar } from '@/pages/calendar'

const events = vi.hoisted(() => vi.fn())
vi.mock('@/wire/endpoints/calendar', () => ({ wireCalendarEvents: events }))

afterEach(() => {
  cleanup()
  events.mockReset()
})

function at(y: number, m: number, d: number, h: number): string {
  return new Date(y, m, d, h).toISOString()
}

function rows() {
  return [
    {
      dtend: at(2026, 8, 2, 10),
      dtstart: at(2026, 8, 2, 9),
      organizer: 'chair@partner.example',
      partstat: 'DECLINED' as const,
      source: 'mail:board-1',
      status: 'CONFIRMED',
      summary: 'Board meeting',
      thread_id: 't-board@partner.example',
      uid: 'board-1',
    },
    {
      dtend: null,
      dtstart: at(2026, 8, 3, 11),
      organizer: null,
      source: 'feed:team',
      status: null,
      summary: 'Standup',
      uid: 'standup',
    },
  ]
}

let navigated: string[] = []

/** Records where the page sent the reader. */
function Location() {
  const where = useLocation()
  if (where.pathname !== '/calendar') navigated.push(where.pathname)
  return null
}

function show(path: string) {
  navigated = []
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter initialEntries={[path]}>
        <Calendar />
        <Location />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

describe('Calendar', () => {
  it('asks for the week shown and strikes through a declined meeting', async () => {
    events.mockResolvedValue(rows())
    show('/calendar?date=2026-09-02')
    const declined = await screen.findByText('Board meeting')
    expect(declined.closest('button')?.className).toContain('line-through')
    expect(screen.getByText('Standup').closest('button')?.className).not.toContain('line-through')
    const [start, end] = events.mock.calls[0]
    expect(start).toEqual(new Date(2026, 7, 31))
    expect(end).toEqual(new Date(2026, 8, 7))
    expect(screen.getAllByRole('gridcell')).toHaveLength(7)
  })

  it('switches to the month and pages through months', async () => {
    events.mockResolvedValue([])
    show('/calendar?date=2026-09-02')
    fireEvent.click(screen.getByRole('tab', { name: 'Month' }))
    expect(await screen.findByText('No events in this period')).toBeDefined()
    expect(screen.getAllByRole('gridcell')).toHaveLength(42)
    expect(screen.getByTestId('calendar-heading').textContent).toContain('September')

    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    expect(await screen.findByText(/October/)).toBeDefined()
    const [start] = events.mock.calls[events.mock.calls.length - 1]
    expect(start).toEqual(new Date(2026, 8, 28))

    fireEvent.click(screen.getByRole('button', { name: 'Previous' }))
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }))
    expect(await screen.findByText(/August/)).toBeDefined()
  })

  it('opens an event to say where it came from and what was answered', async () => {
    events.mockResolvedValue(rows())
    show('/calendar?date=2026-09-02')
    fireEvent.click(await screen.findByText('Board meeting'))
    expect(await screen.findByText('Invitation in your mail')).toBeDefined()
    expect(screen.getByText('Declined')).toBeDefined()
    expect(screen.getByText('chair@partner.example')).toBeDefined()
    fireEvent.click(screen.getByRole('button', { name: 'Open the invitation' }))
    expect(navigated).toEqual(['/mail'])
  })

  it('offers to open the mail only when there is one', async () => {
    events.mockResolvedValue(rows())
    show('/calendar?date=2026-09-02')
    fireEvent.click(await screen.findByText('Standup'))
    expect(await screen.findByText('Subscribed calendar')).toBeDefined()
    expect(screen.queryByRole('button', { name: 'Open the invitation' })).toBeNull()
  })

  it('shows an all-day event on its days, as all day, with its dates', async () => {
    events.mockResolvedValue([
      {
        dtend: '2026-09-05',
        dtstart: '2026-09-03',
        organizer: null,
        source: 'feed:jp-holidays',
        status: null,
        summary: 'Trip',
        uid: 'trip',
      },
    ])
    show('/calendar?date=2026-09-02')
    const chips = await screen.findAllByText('Trip')
    expect(chips).toHaveLength(2)
    const cells = screen.getAllByRole('gridcell')
    expect(cells.map((c) => c.textContent?.includes('Trip'))).toEqual([
      false,
      false,
      false,
      true,
      true,
      false,
      false,
    ])
    expect(chips[0].closest('button')?.textContent).toContain('All day')
    fireEvent.click(chips[0])
    const detail = await screen.findByRole('region', { name: 'Trip' })
    expect(detail.textContent).toContain('All day')
    expect(detail.textContent).not.toContain('Start')
  })

  // the server sends a recurring event once per occurrence, all with its uid
  it('shows each occurrence of a recurring event and opens the one clicked', async () => {
    const occurrence = (d: number) => ({
      dtend: at(2026, 8, d, 10),
      dtstart: at(2026, 8, d, 9),
      organizer: null,
      recurrence_id: at(2026, 8, d, 9),
      source: 'mail:daily',
      status: null,
      summary: 'Daily sync',
      uid: 'daily',
    })
    events.mockResolvedValue([occurrence(1), occurrence(2), occurrence(3)])
    show('/calendar?date=2026-09-02')
    const chips = await screen.findAllByText('Daily sync')
    expect(chips).toHaveLength(3)
    fireEvent.click(chips[0])
    const first = (await screen.findByRole('region', { name: 'Daily sync' })).textContent
    fireEvent.click(chips[2])
    const third = (await screen.findByRole('region', { name: 'Daily sync' })).textContent
    expect(third).not.toEqual(first)
  })

  it('says the calendar could not be loaded rather than showing an empty week', async () => {
    events.mockRejectedValue(new Error('down'))
    show('/calendar?date=2026-09-02')
    expect(await screen.findByRole('alert')).toBeDefined()
    expect(screen.queryByText('No events in this period')).toBeNull()
  })
})
