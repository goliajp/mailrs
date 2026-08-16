import { describe, expect, it } from 'vitest'

import { claimedDomain, contradictedDomain } from '../sender-claim'

describe('contradictedDomain', () => {
  it('names the sending domain when the display name claims another', () => {
    expect(contradictedDomain('Amazon.co.jp', 'no-reply@mail07.jqjintaiyang.com')).toBe(
      'mail07.jqjintaiyang.com'
    )
    expect(contradictedDomain('golia.jp | HR', 'hr@halfwaylexus.cam')).toBe('halfwaylexus.cam')
  })

  /**
   * The padding that the character check deliberately lets through.
   *
   * `U+FEFF`, `U+200C` and `U+200D` have real typographic jobs, so
   * `mailrs_textguard` must not flag them — and three production
   * messages use exactly those to break up `Amazon` inside a display
   * name. This is the signal that catches them, which is why both
   * exist.
   */
  it('sees through padding made of legitimate invisibles', () => {
    // The mechanism, pinned: an invisible is not a label character, so
    // it splits the token and the tail is what matches. Without that,
    // `am\ufeffazon.co.jp` stays one token and this reads differently.
    expect(claimedDomain('Am\ufeffazon.co.jp')).toBe('azon.co.jp')

    expect(contradictedDomain('Am﻿azon.co.jp 配信システム', 'x@funny.lcrwa.com')).toBe(
      'funny.lcrwa.com'
    )
    expect(contradictedDomain('Ama‌zon.co.jp (自動送信メール)', 'x@drink.example.com')).toBe(
      'drink.example.com'
    )
    expect(contradictedDomain('Amazo‍n.co.jp デリバリー', 'x@bridge.877fnavi.net')).toBe(
      'bridge.877fnavi.net'
    )
  })

  it('says nothing when the name and the sender agree', () => {
    expect(contradictedDomain('Amazon.co.jp', 'no-reply@amazon.co.jp')).toBeNull()
    expect(contradictedDomain('Amazon.co.jp', 'no-reply@email.amazon.co.jp')).toBeNull()
    expect(contradictedDomain('email.amazon.co.jp', 'no-reply@amazon.co.jp')).toBeNull()
  })

  it('says nothing when the name claims no domain at all', () => {
    expect(contradictedDomain('MyJCB', 'a@wokjx.crabfishhh.com')).toBeNull()
    expect(contradictedDomain('Ann O’Brien', 'ann@example.com')).toBeNull()
    expect(contradictedDomain('', 'a@b.example')).toBeNull()
  })

  /** A bare public suffix is a suffix, not a claim. */
  it('does not read a bare suffix as a claim', () => {
    expect(contradictedDomain('co.jp', 'a@b.example')).toBeNull()
  })

  it('tolerates addresses it cannot read', () => {
    expect(contradictedDomain('Amazon.co.jp', '')).toBeNull()
    expect(contradictedDomain('Amazon.co.jp', 'not-an-address')).toBeNull()
    expect(contradictedDomain('Amazon.co.jp', 'a@localhost')).toBeNull()
  })

  /** Full `From` headers, the shape the UI actually holds. */
  it('reads a display name out of a full From header', () => {
    expect(contradictedDomain('Amazon.co.jp <x@mail07.jqjintaiyang.com>', '')).toBe(
      'mail07.jqjintaiyang.com'
    )
    expect(contradictedDomain('"Amazon.co.jp" <x@amazon.co.jp>', '')).toBeNull()
  })
})
