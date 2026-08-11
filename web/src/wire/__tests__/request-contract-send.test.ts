import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

/**
 * `sendMail` picks its transport by whether there are attachments; with
 * none it sends JSON, which is the path these fixtures describe. The
 * multipart path is a FormData body and is covered by the field-name
 * assertions in the Rust half plus `send-mail-schedule.test.ts`.
 */
describe('send request bodies match the shared contract', () => {
  it('send with a scheduled time', async () => {
    const { sendMail } = await import('@/lib/send-mail')
    const body = await bodyOf(() =>
      sendMail({
        bcc: [],
        body: 'hello',
        cc: [],
        from: 'lihao@golia.jp',
        htmlBody: '<p>hello</p>',
        scheduledAt: 1_785_542_400,
        subject: 'subject',
        to: ['someone@example.com'],
        token: 'test-token',
      })
    )
    expect(body).toEqual(fixture('send'))
  })

  it('send as a redraft, keeping two of the carried attachments', async () => {
    const { sendMail } = await import('@/lib/send-mail')
    const body = await bodyOf(() =>
      sendMail({
        bcc: [],
        body: 'fixed',
        cc: [],
        from: 'lihao@golia.jp',
        htmlBody: '<p>fixed</p>',
        redraftKeep: [0, 2],
        redraftOf: 'abc123@golia.jp',
        subject: 'subject',
        to: ['someone@example.com'],
        token: 'test-token',
      })
    )
    expect(body).toEqual(fixture('send-redraft'))
  })
})
