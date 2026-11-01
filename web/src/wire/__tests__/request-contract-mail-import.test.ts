// The IMAP import's request body, against the fixture the Rust side reads.

import { describe, expect, it } from 'vitest'

import { bodyOf, capturedRequest, fixture } from './request-contract-harness'

describe('mail import request bodies match the shared contract', () => {
  it('imap import start', async () => {
    const { wireStartImapImport } = await import('../endpoints/imap-import')
    const body = await bodyOf(() =>
      wireStartImapImport({
        host: 'imap.gmail.com',
        password: 'abcd efgh ijkl mnop',
        port: 993,
        username: 'me@gmail.com',
      })
    )
    expect(body).toEqual(fixture('imap-import-start'))
    expect(capturedRequest().path).toBe('/api/mail/import/imap')
  })
})
