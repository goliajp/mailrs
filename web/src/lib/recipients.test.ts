import { describe, expect, it } from 'vitest'

import { formatRecipients } from '@/components/thread-view-helpers'

import { splitAddressList } from './recipients'

describe('splitAddressList', () => {
  it('keeps a quoted display name with a comma in one entry', () => {
    expect(splitAddressList('"Anthropic, PBC" <invoice@mail.anthropic.com>, c@d.com')).toEqual([
      '"Anthropic, PBC" <invoice@mail.anthropic.com>',
      'c@d.com',
    ])
  })

  it('drops empty entries', () => {
    expect(splitAddressList(' a@b.com ,, ,c@d.com,')).toEqual(['a@b.com', 'c@d.com'])
    expect(splitAddressList('')).toEqual([])
  })
})

describe('formatRecipients', () => {
  it('names a "Lastname, Firstname" addressee once', () => {
    expect(formatRecipients('"Doe, Jane" <jane@x.com>, bob@y.com')).toBe('Doe, Jane, bob')
  })
})
