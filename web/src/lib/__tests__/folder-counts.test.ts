import { describe, expect, it } from 'vitest'

import { countFor } from '@/lib/folder-counts'

// two unread promotions and nothing unread in the Inbox: the case that
// showed "2" beside the Inbox
const unread = { count: 2, inbox: 0, np: 2 }

describe('each folder shows the unread it lists', () => {
  it('puts the promotions beside Notifications & Promotions, not the Inbox', () => {
    expect(countFor('inbox', unread)).toBe(0)
    expect(countFor('np', unread)).toBe(2)
  })

  it('gives the Unread list the whole count', () => {
    expect(countFor('unread', unread)).toBe(2)
  })

  it('gives every other folder none', () => {
    expect(countFor('starred', unread)).toBe(0)
    expect(countFor('junk', unread)).toBe(0)
  })
})
