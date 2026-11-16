import { describe, expect, it } from 'vitest'

import { inInbox, rowCategory } from '@/lib/row-category'

const NOW = 1_800_000_000

// a conversation with one reply from the other side: in the Inbox
const replied = {
  archived: false,
  category: 'inbox',
  message_count: 2,
  quarantined: false,
  sent_count: 1,
  snoozed_until: 0,
  trashed_at: 0,
}

describe('rowCategory', () => {
  it('shows Inbox on a conversation that is in the Inbox', () => {
    expect(rowCategory(replied, NOW)).toBe('inbox')
  })

  it('does not show Inbox on mail the reader only sent', () => {
    const sent = { ...replied, message_count: 1, sent_count: 1 }
    expect(inInbox(sent, NOW)).toBe(false)
    expect(rowCategory(sent, NOW)).toBeNull()
  })

  it('does not show Inbox where the conversation was put away', () => {
    for (const away of [
      { archived: true },
      { quarantined: true },
      { trashed_at: NOW - 60 },
      { snoozed_until: NOW + 3600 },
    ]) {
      expect(rowCategory({ ...replied, ...away }, NOW)).toBeNull()
    }
    expect(rowCategory({ ...replied, snoozed_until: NOW - 1 }, NOW)).toBe('inbox')
  })

  it('shows what kind of mail it is wherever it lies', () => {
    const sentNotice = { ...replied, category: 'notification', message_count: 1, sent_count: 1 }
    expect(rowCategory(sentNotice, NOW)).toBe('notification')
    expect(rowCategory({ ...replied, archived: true, category: 'promotion' }, NOW)).toBe(
      'promotion'
    )
  })

  it('shows nothing for no category or the general one', () => {
    expect(rowCategory({ ...replied, category: '' }, NOW)).toBeNull()
    expect(rowCategory({ ...replied, category: 'general' }, NOW)).toBeNull()
  })

  it('counts a conversation without counts as received mail', () => {
    expect(rowCategory({ ...replied, message_count: 0, sent_count: 0 }, NOW)).toBe('inbox')
  })
})
