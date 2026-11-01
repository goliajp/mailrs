// A reply to mail sent to one of the user's masked addresses goes out from
// that mask; a forward, or mail sent to the real address, does not.

import type { ComposerStart, ReplyContext } from '../start'

import { describe, expect, it } from 'vitest'

import { replyMaskOf } from '../start'

const MASK = 'amber.otter.000042@golia.jp'

function ctx(originalTo: string): ReplyContext {
  return {
    forwardAttachmentsUid: null,
    forwardMessageId: null,
    lastMessageId: 'm@shop.example',
    originalBody: '',
    originalDate: '',
    originalFrom: 'Shop <news@shop.example>',
    originalHtmlBody: null,
    originalTo,
    replyAllRecipients: '',
    replyRecipients: 'news@shop.example',
    subject: 'Your order',
    threadId: 't',
  }
}

function inline(originalTo: string, mode: 'forward' | 'reply' | 'reply-all'): ComposerStart {
  return { ctx: ctx(originalTo), kind: 'inline', mode }
}

describe('replyMaskOf', () => {
  it('picks the mask the message was sent to, whatever its case and display name', () => {
    expect(replyMaskOf(inline(`Me <${MASK.toUpperCase()}>, other@x.test`, 'reply'), [MASK])).toBe(
      MASK
    )
    expect(replyMaskOf(inline(MASK, 'reply-all'), [MASK])).toBe(MASK)
  })

  it('leaves a forward and mail to the real address alone', () => {
    expect(replyMaskOf(inline(MASK, 'forward'), [MASK])).toBe('')
    expect(replyMaskOf(inline('me@golia.jp', 'reply'), [MASK])).toBe('')
    expect(replyMaskOf({ kind: 'new' }, [MASK])).toBe('')
  })
})
