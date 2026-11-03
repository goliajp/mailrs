import { describe, expect, it } from 'vitest'

import { dateFormat } from '../format'

const WHEN = [
  new Date(2026, 10, 3, 9, 5),
  new Date(2024, 0, 31, 23, 59),
  new Date(2026, 5, 1, 0, 0),
]
const LOCALES = ['en-US', 'ja-JP', 'zh-CN']

// every option set format.ts asks for
const OPTIONS: Intl.DateTimeFormatOptions[] = [
  { weekday: 'long' },
  { day: 'numeric', month: 'short' },
  { day: 'numeric', month: 'short', year: 'numeric' },
  { day: 'numeric', month: 'short', year: '2-digit' },
  { hour: '2-digit', minute: '2-digit' },
  { weekday: 'short' },
  {
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    month: 'short',
    weekday: 'short',
    year: 'numeric',
  },
]

describe('dateFormat', () => {
  it('formats as the toLocale methods format.ts used', () => {
    for (const locale of LOCALES) {
      for (const options of OPTIONS) {
        for (const d of WHEN) {
          const got = dateFormat(locale, options).format(d)
          expect(got).toBe(d.toLocaleString(locale, options))
          // the date and time variants add the fields of their kind when
          // none is given, so each is the same only for options of its kind
          if (!('hour' in options)) expect(got).toBe(d.toLocaleDateString(locale, options))
          if (!('day' in options || 'weekday' in options)) {
            expect(got).toBe(d.toLocaleTimeString(locale, options))
          }
        }
      }
    }
  })

  it('builds one formatter per locale and options', () => {
    const a = dateFormat('ja-JP', { day: 'numeric', month: 'short' })
    expect(dateFormat('ja-JP', { day: 'numeric', month: 'short' })).toBe(a)
    expect(dateFormat('en-US', { day: 'numeric', month: 'short' })).not.toBe(a)
    expect(dateFormat('ja-JP', { day: 'numeric', month: 'long' })).not.toBe(a)
  })
})
