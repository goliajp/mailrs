import { describe, expect, it } from 'vitest'

import { type FromAddress, replyFromFor } from '../from-addresses'

const own: FromAddress = { accountId: '', address: 'me@golia.jp', label: 'me@golia.jp' }
const gmail: FromAddress = { accountId: 'ext_g', address: 'me@gmail.com', label: 'Work' }
const qq: FromAddress = { accountId: 'ext_q', address: 'me@qq.com', label: 'QQ' }
const all = [own, gmail, qq]

describe('which address a reply leaves by', () => {
  /**
   * The one that breaks threads. A reply to mail that arrived at a
   * connected Gmail has to go out through that Gmail — sent from
   * anywhere else it lands in the conversation as a stranger, and half
   * the time the recipient's provider refuses it outright.
   */
  it('follows the account the mail arrived at', () => {
    expect(replyFromFor('ext_g', all)).toBe('me@gmail.com')
    expect(replyFromFor('ext_q', all)).toBe('me@qq.com')
  })

  it('uses this server for mail that arrived here', () => {
    expect(replyFromFor('', all)).toBe('me@golia.jp')
    expect(replyFromFor(null, all)).toBe('me@golia.jp')
    expect(replyFromFor(undefined, all)).toBe('me@golia.jp')
  })

  /**
   * An account that was removed, or whose password stopped working and
   * so is not offered: replying from somewhere beats a compose window
   * that will not send.
   */
  it('falls back rather than leaving the sender blank', () => {
    expect(replyFromFor('ext_gone', all)).toBe('me@golia.jp')
    expect(replyFromFor('ext_g', [own])).toBe('me@golia.jp')
  })

  /** Nothing to send from is empty, not a crash. */
  it('survives having no addresses at all', () => {
    expect(replyFromFor('ext_g', [])).toBe('')
  })
})
