import { describe, expect, it } from 'vitest'

import { device } from '@/lib/user-agent'

describe('device', () => {
  it('names the browser and the system', () => {
    expect(
      device('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Version/18.1 Safari/605.1.15')
    ).toBe('Safari on macOS')
    expect(device('Mozilla/5.0 (Windows NT 10.0) Chrome/130.0 Safari/537.36 Edg/130.0')).toBe(
      'Edge on Windows'
    )
    expect(device('Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0')).toBe(
      'Firefox on Linux'
    )
  })

  it('shows what it cannot name, and says when there is nothing', () => {
    expect(device('curl/8.4.0')).toBe('curl/8.4.0')
    expect(device('')).toBe('Unknown device')
  })
})
