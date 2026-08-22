import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AttachmentPreview } from '../attachment-preview'
import { DateSuggestions } from '../date-suggestions'

/// One rule for separation in the reading pane: the column draws the
/// lines between its sections, and no section draws its own.
///
/// 2026-08-22, from a screenshot: two hairlines 12px apart above the
/// "Add to calendar" row, because the header drew a `border-b` and the
/// suggestions drew a `border-t`; and the same doubling again where the
/// body's `border-b` met the attachments' `border-t`, with a third rule
/// beside the ATTACHMENTS label for good measure. Where two sections
/// each drew none — the AI panel against the body — there was no line
/// at all. Both faults are the same missing rule.
///
/// Asserted on the sections themselves rather than through the whole
/// pane, because that is where the defect lives: a section that draws
/// its own separator is wrong wherever it is placed.
describe('a section does not draw its own separator', () => {
  const edges = (el: Element): string[] =>
    [...el.querySelectorAll('*'), el]
      .flatMap((n) => [...n.classList])
      .filter((c) => /^border-[tb]$/.test(c) || /^border-[tb]-/.test(c))

  it('the date suggestions row draws none', () => {
    const { container } = render(
      <DateSuggestions suggestions={[{ date: '2026-08-25', datetime: null, text: 'the 25th' }]} />
    )
    expect(edges(container.firstElementChild!)).toEqual([])
  })

  it('the attachments section draws none', () => {
    const { container } = render(
      <AttachmentPreview
        attachments={[{ content_type: 'application/pdf', filename: 'a.pdf', size: 10 }]}
        html={null}
        uid={1}
      />
    )
    expect(edges(container.firstElementChild!)).toEqual([])
  })

  it('the suggestions row carries its own section padding', () => {
    // It is a direct child of the divided column now, so nothing else
    // will indent it.
    const { container } = render(
      <DateSuggestions suggestions={[{ date: '2026-08-25', datetime: null, text: 'the 25th' }]} />
    )
    const cls = [...container.firstElementChild!.classList]
    expect(cls.some((c) => c.startsWith('px-'))).toBe(true)
    expect(cls.some((c) => c.startsWith('py-'))).toBe(true)
  })

  it('an empty suggestion list renders nothing, so no divider is drawn for it', () => {
    const { container } = render(<DateSuggestions suggestions={[]} />)
    expect(container.firstElementChild).toBeNull()
  })
})
