import { describe, expect, it } from 'vitest'

import { verifiedSender } from '@/lib/sender-identity'

describe('verifiedSender', () => {
  it('names the address only when authentication vouched for it', () => {
    expect(verifiedSender({ sender: 'Shop <news@shop.example>', sender_trust: 'verified' })).toBe(
      'news@shop.example'
    )
    expect(verifiedSender({ sender: 'news@shop.example', sender_trust: 'verified' })).toBe(
      'news@shop.example'
    )
    expect(
      verifiedSender({ sender: 'news@shop.example', sender_trust: 'unverified' })
    ).toBeUndefined()
    expect(
      verifiedSender({ sender: 'news@shop.example', sender_trust: 'suspicious' })
    ).toBeUndefined()
    expect(verifiedSender({ sender: 'Shop', sender_trust: 'verified' })).toBeUndefined()
  })
})
