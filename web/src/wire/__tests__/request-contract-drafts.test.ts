import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

describe('draft request bodies match the shared contract', () => {
  /** A draft with files names each by the id its upload answered. */
  it('draft save with attachments', async () => {
    const { wireSaveDraft } = await import('../endpoints/mail')
    const body = await bodyOf(() =>
      wireSaveDraft({
        attachments: [
          {
            blob_id: 'U3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7',
            content_type: 'application/pdf',
            name: 'quote.pdf',
            size: 48213,
          },
        ],
        bcc: '',
        body: 'The quote is attached.',
        cc: '',
        from: '',
        id: 42,
        subject: 'Quote',
        to: 'nagata@nagatax.tokyo.jp',
      })
    )
    expect(body).toEqual(fixture('draft-save-attachments'))
  })
})
