import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Calendar } from '@/pages/calendar'

// East of UTC an all-day date read as an instant becomes 09:00 on its day,
// running into the next: the bug only shows here, whatever zone the
// machine running the tests is in.
vi.hoisted(() => {
  process.env.TZ = 'Asia/Tokyo'
})

const events = vi.hoisted(() => vi.fn())
vi.mock('@/wire/endpoints/calendar', () => ({ wireCalendarEvents: events }))

afterEach(() => {
  cleanup()
  events.mockReset()
})

function allDay(uid: string, dtstart: string, dtend: null | string) {
  return {
    dtend,
    dtstart,
    organizer: null,
    source: 'feed:jp-holidays',
    status: null,
    summary: uid,
    uid,
  }
}

/** Which grid cells name `summary`, by position. */
function cellsWith(summary: string): number[] {
  const found: number[] = []
  screen.getAllByRole('gridcell').forEach((cell, i) => {
    if (cell.textContent?.includes(summary)) found.push(i)
  })
  return found
}

function show(path: string) {
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter initialEntries={[path]}>
        <Calendar />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

describe('all-day events in Tokyo', () => {
  it('runs in Tokyo', () => {
    expect(new Date(2026, 8, 1).getTimezoneOffset()).toBe(-540)
  })

  it('the week shows each all-day event on its own dates only, as all day', async () => {
    events.mockResolvedValue([
      allDay('Holiday', '2026-09-01', null),
      allDay('Trip', '2026-09-03', '2026-09-05'),
    ])
    show('/calendar?date=2026-09-02')
    await screen.findAllByText('Trip')
    // the week of Monday 31 August
    expect(cellsWith('Holiday')).toEqual([1])
    expect(cellsWith('Trip')).toEqual([3, 4])
    for (const chip of screen.getAllByText('Holiday')) {
      expect(chip.closest('button')?.textContent).toContain('All day')
      expect(chip.closest('button')?.textContent).not.toContain('09:00')
    }
  })

  it('the month shows each all-day event on its own dates only', async () => {
    events.mockResolvedValue([
      allDay('Holiday', '2026-09-01', null),
      allDay('Trip', '2026-09-03', '2026-09-05'),
    ])
    show('/calendar?date=2026-09-02')
    fireEvent.click(screen.getByRole('tab', { name: 'Month' }))
    await screen.findAllByText('Trip')
    expect(screen.getAllByRole('gridcell')).toHaveLength(42)
    // the month grid starts on Monday 31 August
    expect(cellsWith('Holiday')).toEqual([1])
    expect(cellsWith('Trip')).toEqual([3, 4])
  })
})
