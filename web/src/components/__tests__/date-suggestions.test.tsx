import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { DateSuggestions } from '@/components/date-suggestions'

describe('dates offered from the body', () => {
  it('offers nothing when there is nothing to offer', () => {
    const { container } = render(<DateSuggestions suggestions={[]} />)
    expect(container).toBeEmptyDOMElement()
  })

  // The one that is easy to get wrong. "2pm" in a sentence means the
  // writer's own afternoon, and neither side knows which zone that is.
  // RFC 5545 3.3.5 has a form for exactly that — a floating time, no
  // zone and no Z — and stamping it UTC instead would move the
  // appointment by the reader's offset.
  it('writes the time floating, not as an instant', () => {
    render(
      <DateSuggestions
        suggestions={[
          { date: '2026-08-21', datetime: '2026-08-21T14:00:00', text: 'August 21 at 2pm' },
        ]}
      />
    )
    const href = screen.getByRole('link').getAttribute('href') ?? ''
    const ics = decodeURIComponent(href.replace(/^data:text\/calendar;charset=utf-8,/, ''))
    expect(ics).toContain('DTSTART:20260821T140000')
    expect(ics).not.toContain('Z\r\n')
    expect(ics).toContain('SUMMARY:August 21 at 2pm')
  })

  // A day with no hour is a day. Giving it midnight invents a meeting
  // time nobody wrote.
  it('keeps a date-only suggestion date-only', () => {
    render(
      <DateSuggestions suggestions={[{ date: '2026-08-21', datetime: null, text: '21 August' }]} />
    )
    const href = screen.getByRole('link').getAttribute('href') ?? ''
    const ics = decodeURIComponent(href.replace(/^data:text\/calendar;charset=utf-8,/, ''))
    expect(ics).toContain('DTSTART;VALUE=DATE:20260821')
    expect(ics).not.toContain('T00')
  })

  it('quotes back what was written, rather than reformatting it', () => {
    render(
      <DateSuggestions suggestions={[{ date: '2026-08-21', datetime: null, text: '8月21日' }]} />
    )
    expect(screen.getByText('8月21日')).toBeTruthy()
  })
})
