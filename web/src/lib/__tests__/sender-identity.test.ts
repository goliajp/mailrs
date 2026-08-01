import { describe, expect, it } from 'vitest'

import { isOwnMessage, isSpoofSuspected } from '../sender-identity'

/**
 * `Netflix <takagi@golia.jp>` arrived on 2026-08-01 from a Google Cloud host
 * announcing itself as `mail.golia.ai`. SPF softfail, no DKIM, DMARC fail
 * against a published `p=quarantine`. The pipeline marked it
 * `sender_trust: suspicious` and `category: spam`; the reading pane drew it
 * with the recipient's own avatar anyway, because the From header said so.
 */
describe('a forged From cannot claim to be you', () => {
  it('is not your message when authentication failed', () => {
    expect(isOwnMessage('takagi@golia.jp', 'takagi@golia.jp', 'suspicious')).toBe(false)
  })

  it('is your message when the address matches and nothing contradicts it', () => {
    expect(isOwnMessage('takagi@golia.jp', 'takagi@golia.jp', 'verified')).toBe(true)
    // A locally written copy of your own sent mail carries no verdict.
    expect(isOwnMessage('takagi@golia.jp', 'takagi@golia.jp', '')).toBe(true)
    expect(isOwnMessage('takagi@golia.jp', 'takagi@golia.jp', undefined)).toBe(true)
  })

  it('is never your message when the address does not match', () => {
    expect(isOwnMessage('someone@else.com', 'takagi@golia.jp', 'verified')).toBe(false)
  })

  it('compares addresses without case', () => {
    expect(isOwnMessage('Takagi@Golia.JP', 'takagi@golia.jp', 'verified')).toBe(true)
  })

  it('an empty address is nobody', () => {
    expect(isOwnMessage('', 'takagi@golia.jp', 'verified')).toBe(false)
    expect(isOwnMessage('takagi@golia.jp', '', 'verified')).toBe(false)
  })

  /**
   * `unverified` is the ordinary middle — most legitimate mail from small
   * senders lands there. Warning on it would train people past the one
   * verdict that means something.
   */
  it('only a failed verdict is a suspected spoof', () => {
    expect(isSpoofSuspected('suspicious')).toBe(true)
    expect(isSpoofSuspected('unverified')).toBe(false)
    expect(isSpoofSuspected('verified')).toBe(false)
    expect(isSpoofSuspected('')).toBe(false)
    expect(isSpoofSuspected(null)).toBe(false)
  })
})
