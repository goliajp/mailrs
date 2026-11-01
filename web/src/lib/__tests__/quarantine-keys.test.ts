/**
 * The two fraud axes must key differently.
 *
 * Not a shape test. Both of these are cases where the reader is served
 * the wrong rows out of cache and nothing errors — the failure mode
 * this repository keeps finding, where every layer is individually
 * consistent and the answer is quietly wrong:
 *
 * - a reader who turns "hide suspected fraud" off and gets the same
 *   list back has been told their setting does nothing;
 * - the Review tab and the Inbox are two different questions, and a
 *   shared key would answer the second with the first.
 */

import { describe, expect, it } from 'vitest'

import { conversationKeys } from '@/store/query-keys-v21'

describe('the fraud axes participate in the cache key', () => {
  it('showing held conversations is a different list from hiding them', () => {
    const hidden = conversationKeys.infinite({ folder: 'INBOX' })
    const shown = conversationKeys.infinite({ folder: 'INBOX', includeQuarantined: true })
    expect(shown).not.toEqual(hidden)
  })

  it('the review list is a different list from the inbox', () => {
    const inbox = conversationKeys.infinite({ folder: 'INBOX' })
    const review = conversationKeys.infinite({ quarantined: true })
    expect(review).not.toEqual(inbox)
  })

  it('and the two fraud axes are not each other', () => {
    const including = conversationKeys.infinite({ includeQuarantined: true })
    const only = conversationKeys.infinite({ quarantined: true })
    expect(including).not.toEqual(only)
  })

  it('still keys the same for two callers asking the same thing', () => {
    expect(conversationKeys.infinite({ folder: 'INBOX', includeQuarantined: true })).toEqual(
      conversationKeys.infinite({ folder: 'INBOX', includeQuarantined: true })
    )
  })
})
