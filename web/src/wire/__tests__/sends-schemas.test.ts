import { describe, expect, it } from 'vitest'

import { redraftSchema, sendsSchema } from '../schemas/sends'

/**
 * Captured verbatim from prod 2.18.14 on 2026-07-30:
 *
 *   curl -H 'Authorization: Bearer …' 'localhost:3103/api/mail/sends?limit=3'
 *
 * Not edited to match the schema. Nine schemas drifted from their handlers
 * on 2026-07-08 because their fixtures were written from the schema, so
 * fixture and schema agreed while the backend sent something else and the
 * UI rendered an empty list with no error
 * (`.claude/rules/frontend/wire-schema-verification.md`).
 */
const CAPTURED = [
  {
    can_resend: true,
    created_at: 1785369273,
    recipients: [
      {
        code: 250,
        delivered: true,
        message: '',
        pending: false,
        recipient: 'GOLIA <goliaaccess@gmail.com>',
      },
    ],
    resent_from: null,
    send_id: '4974a5fd975d0dab@golia.jp',
    status: 'delivered',
    subject: 'send test 2',
    thread_id: '4974a5fd975d0dab@golia.jp',
    to: ['GOLIA <goliaaccess@gmail.com>'],
  },
  {
    can_resend: true,
    created_at: 1785369247,
    recipients: [
      {
        code: 250,
        delivered: true,
        message: '',
        pending: false,
        recipient: 'GOLIA <goliaaccess@gmail.com>',
      },
    ],
    resent_from: null,
    send_id: '3d6af3cd9ece8e18@golia.jp',
    status: 'delivered',
    subject: 'send test 1',
    thread_id: '3d6af3cd9ece8e18@golia.jp',
    to: ['GOLIA <goliaaccess@gmail.com>'],
  },
]

describe('sendsSchema', () => {
  it('parses the response prod actually returns', () => {
    const parsed = sendsSchema.parse(CAPTURED)
    expect(parsed).toHaveLength(2)
    expect(parsed[0].status).toBe('delivered')
    expect(parsed[0].recipients[0].code).toBe(250)
    // Display name included — a UI that assumes a bare address here shows
    // the raw header to the user.
    expect(parsed[0].to[0]).toBe('GOLIA <goliaaccess@gmail.com>')
  })

  it('keeps resent_from absent rather than turning null into a string', () => {
    const parsed = sendsSchema.parse(CAPTURED)
    expect(parsed[0].resent_from).toBeNull()
  })

  /// An unknown status must fail loudly. Widening it to a fallback would
  /// paint a failed send with whatever the default badge is, which is the
  /// one thing this view exists to make visible.
  it('refuses a status it does not know', () => {
    const bad = [{ ...CAPTURED[0], status: 'quantum' }]
    expect(() => sendsSchema.parse(bad)).toThrow()
  })

  it('parses a failed send with a real SMTP refusal', () => {
    const failed = [
      {
        ...CAPTURED[0],
        recipients: [
          {
            code: 550,
            delivered: false,
            message: '5.1.1 The email account that you tried to reach does not exist.',
            pending: false,
            recipient: 'nope@golia.jp',
          },
        ],
        status: 'failed',
      },
    ]
    const parsed = sendsSchema.parse(failed)
    expect(parsed[0].status).toBe('failed')
    expect(parsed[0].recipients[0].code).toBe(550)
  })

  /// `can_resend: false` is how the backend says the envelope bytes are
  /// not on disk. It must survive parsing, because the UI keys the resend
  /// and re-edit buttons off it.
  it('carries can_resend false through', () => {
    const parsed = sendsSchema.parse([{ ...CAPTURED[0], can_resend: false }])
    expect(parsed[0].can_resend).toBe(false)
  })
})

describe('redraftSchema', () => {
  /**
   * No prod capture: `:redraft` ships in the same commit as the schema.
   * Read off the `RedraftResponse` struct in
   * crates/webapi/src/handlers/sends.rs.
   */
  it('parses the RedraftResponse shape', () => {
    const parsed = redraftSchema.parse({
      attachments: [
        { content_type: 'image/png', filename: 'a.png', index: 0, size: 1024 },
        { content_type: 'image/png', filename: 'a.png', index: 1, size: 2048 },
      ],
      bcc: ['blind@x.com'],
      body: 'text',
      cc: [],
      html_body: '<p>text</p>',
      in_reply_to: 'thread@golia.jp',
      redraft_of: 'orig@golia.jp',
      subject: 'fix me',
      to: ['GOLIA <goliaaccess@gmail.com>'],
    })
    expect(parsed.attachments).toHaveLength(2)
    // Two parts, one filename, distinct indices — the reason the wire
    // carries indices at all.
    expect(parsed.attachments[0].filename).toBe(parsed.attachments[1].filename)
    expect(parsed.attachments[0].index).not.toBe(parsed.attachments[1].index)
    expect(parsed.bcc).toEqual(['blind@x.com'])
  })
})
