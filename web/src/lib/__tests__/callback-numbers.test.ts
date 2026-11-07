import { describe, expect, it } from 'vitest'

import { newCallbackNumbers, phoneNumbers } from '@/lib/callback-numbers'

// shared with iOS and Android, which test their copies of the rule against
// the same file (scripts/check-callback-numbers.sh keeps the bytes equal)
import shared from '../callback-numbers.json'

const msg = (
  text: string,
  sender = 'Billing <billing@shop.example>',
  message_id = 'm1@shop.example'
) => ({
  html_body: null,
  message_id,
  sender,
  text_body: text,
})

/** A number the sender gave this reader before, as `/api/senders/{address}/callback-numbers` names it. */
const given = (number: string, first_message_id: string) => ({
  first_message_id,
  first_seen: 1_790_000_000,
  number,
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
      message_id: 'po@vendor.example',
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
      message_id: 'h@b.example',
      sender: 'a@b.example',
      text_body: null,
    }
    expect(newCallbackNumbers(html, [])).toEqual(['+1 888 555 0142'])
  })

  it('does not point out a number the sender gave in another conversation', () => {
    const history = [given('08001234567', 'older@shop.example')]
    expect(newCallbackNumbers(msg('Call 0800-123-4567 today'), [], history)).toEqual([])
  })

  it('points out a number whose first message is the one being read', () => {
    const history = [given('08001234567', 'm1@shop.example')]
    expect(newCallbackNumbers(msg('Call 0800-123-4567 today'), [], history)).toEqual([
      '0800-123-4567',
    ])
  })

  it('compares digits, however the number was written before', () => {
    const history = [given('81312345678', 'older@shop.example')]
    expect(newCallbackNumbers(msg('Call +81-3-1234-5678 or 0570-000-999'), [], history)).toEqual([
      '0570-000-999',
    ])
  })
})
