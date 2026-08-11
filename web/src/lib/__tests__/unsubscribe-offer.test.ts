import { describe, expect, it } from 'vitest'

import { unsubscribeOffer } from '../unsubscribe-offer'

describe('unsubscribeOffer', () => {
  it('nothing advertised is nothing offered', () => {
    expect(unsubscribeOffer(null)).toEqual({ kind: 'none' })
    expect(unsubscribeOffer(undefined)).toEqual({ kind: 'none' })
    expect(unsubscribeOffer({ http: [], mailto: [] })).toEqual({ kind: 'none' })
  })

  /** The only one that costs the reader nothing wins outright. */
  it('one-click beats everything else on offer', () => {
    expect(
      unsubscribeOffer({
        http: ['https://list.example/leave'],
        mailto: ['mailto:leave@example'],
        one_click: true,
      })
    ).toEqual({ kind: 'one-click' })
  })

  it('a page before an address', () => {
    expect(
      unsubscribeOffer({ http: ['https://list.example/leave'], mailto: ['mailto:x@y'] })
    ).toEqual({ kind: 'page', url: 'https://list.example/leave' })
  })

  it('an address when that is all there is', () => {
    expect(unsubscribeOffer({ mailto: ['mailto:leave@example?subject=unsubscribe'] })).toEqual({
      kind: 'mailto',
      url: 'mailto:leave@example?subject=unsubscribe',
    })
  })

  /** A header can carry anything; only the schemes we can act on count. */
  it('an unusable value is not an offer', () => {
    expect(unsubscribeOffer({ http: ['javascript:alert(1)'], mailto: ['not-an-address'] })).toEqual(
      { kind: 'none' }
    )
  })
})
