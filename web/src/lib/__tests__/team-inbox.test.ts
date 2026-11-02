import { describe, expect, it } from 'vitest'

import { parseTeamConv } from '@/wire/endpoints/shared'

describe('a team inbox row', () => {
  it('reads its team conversation back, and nothing else as one', () => {
    expect(parseTeamConv('3:41')).toEqual({ conv: 41, gid: 3 })
    expect(parseTeamConv('')).toBeNull()
    expect(parseTeamConv(undefined)).toBeNull()
    expect(parseTeamConv('3')).toBeNull()
    expect(parseTeamConv('a:1')).toBeNull()
  })
})
