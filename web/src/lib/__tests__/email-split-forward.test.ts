import { describe, expect, it } from 'vitest'

import { splitEmail } from '../email-split'

/**
 * The shape a Gmail forward actually has, decoded from the message that
 * surfaced this on 2026-07-30
 * (`/data/maildir/golia.jp/lihao/new/1785384840.M921632P1Q5…`, base64
 * text/html part). Everything is inside the quote container; the visible
 * body is two line breaks.
 */
const GMAIL_FORWARD = [
  '<div dir="ltr"><br><br>',
  '<div class="gmail_quote gmail_quote_container">',
  '<div dir="ltr" class="gmail_attr">---------- Forwarded message ---------<br>',
  '发件人： <strong class="gmail_sendername" dir="auto">＜PayPay銀行＞</strong> ',
  '<span dir="auto">&lt;customer1@cc.paypay-bank.co.jp&gt;</span><br>',
  'Date: 2026年7月30日周四 12:56<br>',
  'Subject: 口座開設のお手続きが完了しました<br></div><br><br>',
  'ゴリア（カ様<br><br>',
  'PayPay銀行の法人口座をお申し込みいただきありがとうございました。<br>',
  '口座開設のお手続きが完了しました。<br>',
  '</div></div>',
].join('')

/** A reply, where hiding the quoted history is the point. */
const GMAIL_REPLY = [
  '<div dir="ltr">Sounds good, thanks.</div>',
  '<div class="gmail_quote">',
  '<div dir="ltr" class="gmail_attr">On Wed, Jul 29, 2026 at 4:12 PM Someone wrote:<br></div>',
  '<blockquote class="gmail_quote">the original question</blockquote>',
  '</div>',
].join('')

describe('splitEmail on a forward', () => {
  /// The reported bug: a forwarded mail rendered as a blank band the height
  /// of two `<br>`s. MessageBubble renders only `parts.body` and never
  /// `parts.quoted`, so extracting the quote did not collapse the content —
  /// it discarded it.
  it('keeps the body when the quote block is the whole message', () => {
    const { parts } = splitEmail(null, GMAIL_FORWARD)
    expect(parts.quoted).toBeNull()
    expect(parts.body).toContain('口座開設のお手続きが完了しました')
    expect(parts.body).toContain('PayPay銀行の法人口座')
  })

  /// Not detected by looking for "Forwarded message": that string is
  /// localized, and this very message mixes English and Chinese in one
  /// header block. The rule is what is left after the split.
  it('does not depend on the forward header wording', () => {
    const localized = GMAIL_FORWARD.replace(
      '---------- Forwarded message ---------',
      '---------- 転送されたメッセージ ----------'
    )
    const { parts } = splitEmail(null, localized)
    expect(parts.body).toContain('PayPay銀行の法人口座')
  })
})

describe('splitEmail on a reply', () => {
  /// The behaviour that must not change: a reply with real text of its own
  /// keeps hiding the quoted history.
  it('still splits when there is something left to read', () => {
    const { parts } = splitEmail(null, GMAIL_REPLY)
    expect(parts.body).toContain('Sounds good')
    expect(parts.quoted).toContain('the original question')
    expect(parts.body).not.toContain('the original question')
  })

  it('splits a plain-text reply and keeps the new text', () => {
    const text = 'Yes, agreed.\n\nOn Wed, Jul 29, 2026 Someone wrote:\n> the original\n> question\n'
    const { parts } = splitEmail(text, null)
    expect(parts.body.trim()).toBe('Yes, agreed.')
    expect(parts.quoted).toContain('the original')
  })

  /// A text forward has no new text either — same rule, same outcome.
  it('keeps a plain-text body that is only quoted lines', () => {
    const text = '\n\n> forwarded line one\n> forwarded line two\n'
    const { parts } = splitEmail(text, null)
    expect(parts.body).toContain('forwarded line one')
    expect(parts.quoted).toBeNull()
  })
})

describe('splitEmail emptiness rule', () => {
  /// An image-only body has no text and is not empty. Treating it as empty
  /// would restore a quote nobody needed to see.
  it('does not call an image-only body empty', () => {
    const html = [
      '<div dir="ltr"><img src="cid:logo"></div>',
      '<div class="gmail_quote">quoted history</div>',
    ].join('')
    const { parts } = splitEmail(null, html)
    expect(parts.quoted).toBe('quoted history')
    expect(parts.body).toContain('<img')
  })

  /// `&nbsp;` is not content. Gmail and Outlook both emit it in otherwise
  /// empty wrappers.
  it('treats a body of only whitespace entities as empty', () => {
    const html = '<div>&nbsp;<br></div><div class="gmail_quote">the whole message</div>'
    const { parts } = splitEmail(null, html)
    expect(parts.quoted).toBeNull()
    expect(parts.body).toContain('the whole message')
  })
})
