import { describe, expect, it } from 'vitest'

import { answerWanted, fileableWithoutAnswer, inviteBadge } from '@/lib/invite-badge'

describe('what kind of invitation this is', () => {
  // The case that matters, because it is the common one: Exchange does
  // not send METHOD:UPDATE. It re-sends the whole invitation as a
  // REQUEST with a higher SEQUENCE, so a meeting moved nine times
  // arrives as SEQUENCE:9 — and calling that "New invite" tells the
  // reader the opposite of what happened.
  it('calls a re-sent REQUEST an update', () => {
    expect(inviteBadge('REQUEST', 9).label).toBe('Updated invite')
    expect(inviteBadge('REQUEST', 0).label).toBe('New invite')
    expect(inviteBadge('UPDATE', 0).label).toBe('Updated invite')
  })

  it('names a cancellation as one', () => {
    expect(inviteBadge('CANCEL', 3).label).toBe('Cancelled')
  })

  // Offering Yes/No against a PUBLISH or somebody else's REPLY sends an
  // iTIP message to a party who never asked for one.
  it('asks for an answer only where one was asked for', () => {
    expect(answerWanted('REQUEST')).toBe(true)
    expect(answerWanted('PUBLISH')).toBe(false)
    expect(answerWanted('REPLY')).toBe(false)
    expect(answerWanted('CANCEL')).toBe(false)
  })

  it('offers to file the ones that want nothing', () => {
    expect(fileableWithoutAnswer('PUBLISH')).toBe(true)
    expect(fileableWithoutAnswer('REQUEST')).toBe(false)
  })
})
