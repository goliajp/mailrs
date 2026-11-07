import { describe, expect, it } from 'vitest'

import { bodyOf, capturedRequest, fixture } from './request-contract-harness'

describe('sending a draft with files matches the shared contract', () => {
  /** The draft's files are attached by the server, from the draft it names. */
  it('send naming the draft', async () => {
    const { sendMail } = await import('@/lib/send-mail')
    const body = await bodyOf(() =>
      sendMail({
        bcc: [],
        body: 'The quote is attached.',
        cc: [],
        draftId: 42,
        from: 'lihao@golia.jp',
        htmlBody: '<p>The quote is attached.</p>',
        subject: 'Quote',
        to: ['nagata@nagatax.tokyo.jp'],
      })
    )
    expect(body).toEqual(fixture('send-draft'))
  })

  /** Without the flag the server leaves drafts with files out of the list. */
  it('lists drafts with their files', async () => {
    const { wireListDrafts } = await import('../endpoints/mail')
    await wireListDrafts()
    expect(capturedRequest().path).toContain('/mail/drafts?with_attachments=true')
  })
})
