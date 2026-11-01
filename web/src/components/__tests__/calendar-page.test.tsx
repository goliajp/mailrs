import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
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
  })

  it('says the calendar could not be loaded rather than showing an empty week', async () => {
    events.mockRejectedValue(new Error('down'))
    show('/calendar?date=2026-09-02')
    expect(await screen.findByRole('alert')).toBeDefined()
    expect(screen.queryByText('No events in this period')).toBeNull()
  })
})
