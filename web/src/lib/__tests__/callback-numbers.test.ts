import { describe, expect, it } from 'vitest'

import { newCallbackNumbers, phoneNumbers } from '@/lib/callback-numbers'

// shared with iOS and Android, which test their copies of the rule against
// the same file (scripts/check-callback-numbers.sh keeps the bytes equal)
import shared from '../callback-numbers.json'

const msg = (text: string, sender = 'Billing <billing@shop.example>') => ({
  html_body: null,
  sender,
  text_body: text,
})

describe('phoneNumbers', () => {
  it('reads every shared sample the way the phones do', () => {
    expect(shared.samples.length).toBeGreaterThanOrEqual(10)
    const wrong = shared.samples
      .filter((s) => JSON.stringify(phoneNumbers(s.text)) !== JSON.stringify(s.numbers))
      .map((s) => `${s.name}: ${JSON.stringify(phoneNumbers(s.text))}`)
    expect(wrong).toEqual([])
  })

  it('does not join a purchase order number to the year of the invoice after it', () => {
    const html = {
      html_body:
        '<table><tr><td>Invoice for Phase 1: Under PO 4300078149</td>\n<td>2026-INV-003001-2</td></tr></table>',
      sender: 'billing@vendor.example',
      text_body: null,
    }
    expect(newCallbackNumbers(html, [])).toEqual([])
  })

  it('finds numbers written the ways people write them', () => {
    expect(
      phoneNumbers('Call +1 (888) 555-0142 or 0120-123-456, or 03-1234-5678 in Tokyo')
    ).toEqual(['+1 (888) 555-0142', '0120-123-456', '03-1234-5678'])
  })

  it('leaves out order numbers, dates and short codes', () => {
    expect(phoneNumbers('Order 4012888888881881 on 2026-11-01 12:30, code 123-456')).toEqual([])
  })

  it('lists one number once', () => {
    expect(phoneNumbers('+81 3 1234 5678 / +81-3-1234-5678')).toEqual(['+81 3 1234 5678'])
  })
})

describe('newCallbackNumbers', () => {
  it('points out a number the sender has not written before', () => {
    const before = [msg('Regards\n03-1234-5678')]
    expect(newCallbackNumbers(msg('Call 0800-123-4567 today\n03-1234-5678'), before)).toEqual([
      '0800-123-4567',
    ])
  })

  it('counts only earlier mail from the same address', () => {
    const before = [msg('0800-123-4567', 'Other <x@other.example>')]
    expect(newCallbackNumbers(msg('0800-123-4567'), before)).toEqual(['0800-123-4567'])
  })

  it('reads the html body when there is no text part', () => {
    const html = {
      html_body: '<p>Call <b>+1 888 555 0142</b></p>',
      sender: 'a@b.example',
      text_body: null,
    }
    expect(newCallbackNumbers(html, [])).toEqual(['+1 888 555 0142'])
  })
})
