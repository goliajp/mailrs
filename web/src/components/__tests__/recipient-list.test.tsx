import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { MessageRecipients, RecipientList } from '@/components/recipient-list'
import { splitAddresses } from '@/lib/recipients'

afterEach(cleanup)

/**
 * The `to` line used to be display names with the addresses thrown
 * away, so a header reading `to 29841300, lihao` gave no way to find
 * out who `29841300` is — the question a reader has exactly when a
 * name looks odd, in a client where a display name is the part an
 * impersonator chooses.
 */
describe('splitting a recipients header', () => {
  it('keeps both halves of each addressee', () => {
    const out = splitAddresses('29841300 <ops@golia.jp>, lihao <lihao@golia.jp>')
    expect(out).toEqual([
      { address: 'ops@golia.jp', name: '29841300' },
      { address: 'lihao@golia.jp', name: 'lihao' },
    ])
  })

  /**
   * The comma inside a quoted display name. `"Lastname, Firstname"
   * <x@y>` is ordinary in corporate mail, and a naive split turns one
   * person into two — the second of whom has no address at all.
   */
  it('does not split inside a quoted name', () => {
    const out = splitAddresses('"Kumar, Rahul" <krahu@qti.qualcomm.com>, lihao@golia.jp')
    expect(out).toHaveLength(2)
    expect(out[0].address).toBe('krahu@qti.qualcomm.com')
    expect(out[1].address).toBe('lihao@golia.jp')
  })

  it('survives a bare address and an empty field', () => {
    expect(splitAddresses('noreply@golia.jp')[0].address).toBe('noreply@golia.jp')
    expect(splitAddresses('')).toEqual([])
    expect(splitAddresses('  ,  ')).toEqual([])
  })
})

describe('the recipient line', () => {
  it('shows a name and reveals its address when pressed', () => {
    render(<RecipientList label="to" value="29841300 <ops@golia.jp>" />)
    // The address is not on screen until asked for.
    expect(screen.queryByText('ops@golia.jp')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: '29841300' }))
    expect(screen.getByText('ops@golia.jp')).toBeDefined()

    // And pressing again puts it away.
    fireEvent.click(screen.getByRole('button', { name: '29841300' }))
    expect(screen.queryByText('ops@golia.jp')).toBeNull()
  })

  it('closes on Escape, not only on a click elsewhere', () => {
    render(<RecipientList label="to" value="29841300 <ops@golia.jp>" />)
    fireEvent.click(screen.getByRole('button', { name: '29841300' }))
    expect(screen.getByText('ops@golia.jp')).toBeDefined()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByText('ops@golia.jp')).toBeNull()
  })

  it('renders every addressee of a short list, not the first two and a count', () => {
    render(<RecipientList label="to" value="a <a@x.com>, b <b@x.com>, c <c@x.com>, d <d@x.com>" />)
    for (const name of ['a', 'b', 'c', 'd']) {
      expect(screen.getByRole('button', { name })).toBeDefined()
    }
  })
})

/**
 * A promotional mail listed several hundred addresses in `To:` and the
 * header grew to fill the screen, pushing the body and the verdict panel
 * out of sight. A long list starts as one line: the reader, a few names
 * and a count.
 */
describe('a message header with hundreds of addressees', () => {
  const contacts = (n: number, from = 0) =>
    Array.from({ length: n }, (_, i) => `contact${i + from} <c${i + from}@promo.example>`)
  const summaryLine = (container: HTMLElement) =>
    container.querySelector('[data-recipient-summary]')

  it('collapses to the reader, three names and a count, and expands to all of them', () => {
    const to = [...contacts(150), 'Lihao <LiHao@golia.jp>', ...contacts(149, 150)].join(', ')
    const { container } = render(<MessageRecipients cc="" myEmail="lihao@golia.jp" to={to} />)
    expect(summaryLine(container)?.textContent).toBe(
      'toMe, contact0, contact1, contact2and 296 others'
    )
    // the rest of the list is not rendered until asked for
    expect(screen.queryByRole('button', { name: 'contact3' })).toBeNull()
    expect(screen.getAllByRole('button')).toHaveLength(1)

    fireEvent.click(screen.getByRole('button', { name: 'Show all 300 recipients' }))
    expect(summaryLine(container)).toBeNull()
    expect(screen.getAllByRole('button')).toHaveLength(300)
    expect(screen.getByRole('button', { name: 'contact298' })).toBeDefined()
    expect(screen.getByRole('button', { name: 'Lihao' })).toBeDefined()
  })

  it('keeps to and cc apart once expanded', () => {
    render(
      <MessageRecipients
        cc={contacts(5, 100).join(', ')}
        myEmail="me@golia.jp"
        to={contacts(5).join(', ')}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: 'Show all 10 recipients' }))
    expect(screen.getByText('to')).toBeDefined()
    expect(screen.getByText('cc')).toBeDefined()
    expect(screen.getAllByRole('button')).toHaveLength(10)
  })

  it('does not claim the reader when they were only Bcc', () => {
    const { container } = render(
      <MessageRecipients cc="" myEmail="lihao@golia.jp" to={contacts(300).join(', ')} />
    )
    expect(summaryLine(container)?.textContent).toBe('tocontact0, contact1, contact2and 297 others')
  })

  it('leaves a list of eight as it was', () => {
    const { container } = render(
      <MessageRecipients
        cc={contacts(3, 10).join(', ')}
        myEmail="c0@promo.example"
        to={contacts(5).join(', ')}
      />
    )
    expect(summaryLine(container)).toBeNull()
    expect(screen.getAllByRole('button')).toHaveLength(8)
  })
})
