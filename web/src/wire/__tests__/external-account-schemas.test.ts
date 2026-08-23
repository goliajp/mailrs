import { describe, expect, it } from 'vitest'

import { externalAccountSchema } from '../schemas/external-accounts'

// The backend serialises the whole row, so every field it writes is on
// the wire whether or not anybody asked for it. A Zod object drops what
// it does not declare — silently, with no error and no empty value to
// notice — so a field the server writes and the schema omits reads
// exactly like a field the server never wrote.
//
// `progress` was that field: the sync worker writes it during a full
// re-read, and for one build no client declared it.
describe('an external account row', () => {
  const endpoint = {
    host: 'imap.gmail.com',
    port: 993,
    protocol: 'imap',
    tls: 'implicit',
  }
  const row = {
    auth: 'oauth2',
    colour: '#4285f4',
    display_name: 'Work',
    email: 'someone@gmail.com',
    id: 'acc_1',
    incoming: endpoint,
    last_error: null,
    outgoing: { ...endpoint, host: 'smtp.gmail.com', port: 465, protocol: 'smtp' },
    progress: 'reading Inbox again from the start (1 of 4 folders)',
    provider: 'gmail',
    state: 'ok',
  }

  it('keeps what the account is doing right now', () => {
    const parsed = externalAccountSchema.parse(row)
    expect(parsed.progress).toBe(row.progress)
  })

  it('keeps why it stopped', () => {
    const parsed = externalAccountSchema.parse({
      ...row,
      last_error: 'the server refused the password',
      progress: null,
      state: 'error',
    })
    expect(parsed.last_error).toBe('the server refused the password')
    expect(parsed.progress ?? null).toBeNull()
  })
})
