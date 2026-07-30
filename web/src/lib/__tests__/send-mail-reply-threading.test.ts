import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { sendMail } from '../send-mail'

/**
 * A reply **with an attachment** arrived unthreaded on 2026-07-30: the
 * envelope had no `In-Reply-To`, so the recipient saw a new conversation and
 * our own sent copy became a one-message thread of its own.
 *
 * Two replies sent the same day without attachments were threaded correctly
 * (`multipart/alternative`, one `In-Reply-To` line each), and the broken one
 * was `multipart/mixed` — so the difference is the transport `sendMail`
 * picks, not the reply logic above it.
 */
let captured: { body?: unknown } = {}

beforeEach(() => {
  captured = {}
  vi.stubGlobal(
    'fetch',
    vi.fn((_input: string, init?: RequestInit) => {
      captured = { body: init?.body }
      return Promise.resolve(
        new Response(JSON.stringify({ message_id: 'new@golia.jp', success: true }), {
          headers: { 'content-type': 'application/json' },
          status: 200,
        })
      )
    })
  )
})

afterEach(() => vi.unstubAllGlobals())

function form(): FormData {
  expect(captured.body, 'no request body was sent').toBeInstanceOf(FormData)
  return captured.body as FormData
}

const REPLY = {
  bcc: [],
  body: 'text',
  cc: [],
  from: 'lihao@golia.jp',
  htmlBody: '<p>text</p>',
  inReplyTo: '2d31fbc5-f7f1-4958-9e81-99819ab73d61@nagatax.tokyo.jp',
  subject: 'Re: 決算について',
  to: ['nagata@nagatax.tokyo.jp'],
  token: 't',
}

describe('a reply keeps its threading on both transports', () => {
  it('sends in_reply_to as JSON when there are no attachments', async () => {
    await sendMail(REPLY)
    expect(captured.body).toBeTypeOf('string')
    expect(JSON.parse(captured.body as string).in_reply_to).toBe(REPLY.inReplyTo)
  })

  /// The case that broke. `multipart/mixed` on the wire means this path ran.
  it('sends in_reply_to as multipart when there are attachments', async () => {
    await sendMail({
      ...REPLY,
      attachments: [new File(['x'], 'a.png', { type: 'image/png' })],
    })
    expect(form().get('in_reply_to')).toBe(REPLY.inReplyTo)
  })

  /// The thread hint goes on both transports. It is what makes threading
  /// survive a client that has lost the parent message id — the draft
  /// round-trip, for one, which stores `reply_to_thread_id` and until
  /// 2026-07-30 never read it back.
  it('sends reply_to_thread_id as JSON', async () => {
    await sendMail({ ...REPLY, replyToThreadId: 'thread@nagatax.tokyo.jp' })
    expect(JSON.parse(captured.body as string).reply_to_thread_id).toBe('thread@nagatax.tokyo.jp')
  })

  it('sends reply_to_thread_id as multipart', async () => {
    await sendMail({
      ...REPLY,
      attachments: [new File(['x'], 'a.png', { type: 'image/png' })],
      replyToThreadId: 'thread@nagatax.tokyo.jp',
    })
    expect(form().get('reply_to_thread_id')).toBe('thread@nagatax.tokyo.jp')
  })

  /// The draft case end to end: no parent message id at all, thread only.
  /// The server has to be able to thread from this alone.
  it('can carry the thread with no parent message id', async () => {
    await sendMail({
      ...REPLY,
      attachments: [new File(['x'], 'a.png', { type: 'image/png' })],
      inReplyTo: undefined,
      replyToThreadId: 'thread@nagatax.tokyo.jp',
    })
    const fd = form()
    expect(fd.get('in_reply_to')).toBeNull()
    expect(fd.get('reply_to_thread_id')).toBe('thread@nagatax.tokyo.jp')
  })

  /// Everything else the reply needs must survive the same path.
  it('keeps subject, recipients and bodies on the multipart path', async () => {
    await sendMail({
      ...REPLY,
      attachments: [new File(['x'], 'a.png', { type: 'image/png' })],
    })
    const fd = form()
    expect(fd.get('subject')).toBe('Re: 決算について')
    expect(fd.getAll('to')).toEqual(['nagata@nagatax.tokyo.jp'])
    expect(fd.get('html_body')).toBe('<p>text</p>')
    expect(fd.getAll('attachments')).toHaveLength(1)
  })
})
