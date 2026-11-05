import { describe, expect, it } from 'vitest'

import { draftListSchema } from '../schemas/mail'

// Backend: crates/webapi/src/handlers/prefs.rs — fn list_drafts, a bare
// array of `DraftWire` (crates/core-api/src/method/admin/userdata.rs). The
// first row is what an account's restore writes for a send whose time
// passed while the account was deleted
// (crates/core-sidestate/src/families/outbound/withheld_draft.rs); the
// second is an ordinary draft, which omits the field.
const LISTED = [
  {
    bcc: 'gus@outside.example',
    body: 'see you on monday\r\n',
    cc: 'fay@outside.example',
    created_at: 1_790_000_100,
    from: '',
    id: 7,
    subject: 'お知らせ',
    to: 'Erin <erin@outside.example>',
    updated_at: 1_790_000_100,
    withheld_send_at: 1_790_000_000,
  },
  {
    bcc: '',
    body: 'half written',
    cc: '',
    created_at: 1_790_000_200,
    from: '',
    id: 8,
    subject: 'later',
    to: 'erin@outside.example',
    updated_at: 1_790_000_200,
  },
]

describe('a draft a restore returned', () => {
  it('carries the time it was set to go, and other drafts carry none', () => {
    const { items } = draftListSchema.parse(LISTED)
    expect(items[0].withheld_send_at).toBe(1_790_000_000)
    // returned unsent: nothing schedules it again
    expect(items[0].scheduled_at).toBeNull()
    expect(items[1].withheld_send_at).toBeNull()
  })
})
