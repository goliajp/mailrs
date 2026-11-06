import type { RecurrenceRule } from '@/lib/invite-recurrence'

import { describe, expect, it } from 'vitest'

import { type Lang, translate } from '@/i18n'
import { recurrenceText, shownSpan, weekdayOf } from '@/lib/invite-recurrence'
import { formatOrganiserClock } from '@/lib/invite-time'

const LOCALE: Record<Lang, string> = { en: 'en-US', ja: 'ja-JP', zh: 'zh-CN' }

function words(lang: Lang, rule: Partial<RecurrenceRule>, firstDay: null | string = null) {
  const full: RecurrenceRule = {
    by_day: [],
    by_month: [],
    by_month_day: [],
    count: null,
    freq: 'WEEKLY',
    interval: 1,
    plain: true,
    until: null,
    ...rule,
  }
  return recurrenceText(full, (k, v) => translate(lang, k, v), LOCALE[lang], firstDay)
}

describe('a rule in words', () => {
  it('reads the Teams series in each language', () => {
    const rule = { by_day: ['TH'], until: '2027-04-29' }
    expect(words('zh', rule)).toBe('每周四 · 到 2027年4月29日 为止')
    expect(words('en', rule)).toBe('Weekly on Thursday · until Apr 29, 2027')
    expect(words('ja', rule)).toBe('毎週木曜日 · 2027/04/29 まで')
  })

  it('names several days, an interval and a count', () => {
    expect(words('zh', { by_day: ['MO', 'WE'] })).toBe('每周一、三')
    expect(words('en', { by_day: ['MO', 'WE'], count: 10, interval: 2 })).toBe(
      'Every 2 weeks on Monday, Wednesday · 10 times'
    )
    expect(words('zh', { freq: 'DAILY' })).toBe('每天')
    expect(words('zh', { by_month_day: [15], freq: 'MONTHLY' })).toBe('每月 15 日')
  })

  it('takes the first day’s weekday when the rule names none', () => {
    expect(weekdayOf('2026-10-29T22:00:00')).toBe('TH')
    expect(words('zh', {}, '2026-10-29T22:00:00')).toBe('每周四')
  })

  it('says only that it repeats when the rule is beyond a few words', () => {
    expect(words('zh', { plain: false })).toBe('重复')
    expect(words('zh', { by_day: ['2TU'], freq: 'MONTHLY' })).toBe('每月')
    expect(words('zh', { by_day: ['-1FR'] })).toBe('重复')
  })
})

describe('the organiser’s clock', () => {
  it('uses the zone database for the date, not the TZID’s name', () => {
    const tokyo = 'Asia/Tokyo'
    expect(formatOrganiserClock('2026-10-30T05:00:00Z', 'America/Los_Angeles', tokyo)).toMatch(
      /22:00 PDT|10:00 PM PDT/
    )
    expect(formatOrganiserClock('2026-11-06T06:00:00Z', 'America/Los_Angeles', tokyo)).toMatch(
      /22:00 PST|10:00 PM PST/
    )
    expect(formatOrganiserClock('2026-11-06T06:00:00Z', tokyo, tokyo)).toBeNull()
  })
})

describe('the span the card shows', () => {
  it('is the next occurrence of a series, as an instant or a date', () => {
    const base = { method: 'REQUEST', sequence: 0, summary: 's', uid: 'u' }
    expect(
      shownSpan({
        ...base,
        dtstart_utc: '2026-10-30T05:00:00+00:00',
        next_occurrence: {
          end: '2026-11-06T07:00:00+00:00',
          organiser_local: null,
          organiser_offset_secs: null,
          start: '2026-11-06T06:00:00+00:00',
        },
      }).start
    ).toEqual({ Utc: '2026-11-06T06:00:00+00:00' })
    expect(
      shownSpan({
        ...base,
        next_occurrence: {
          end: '2026-11-04',
          organiser_local: null,
          organiser_offset_secs: null,
          start: '2026-11-03',
        },
      }).start
    ).toEqual({ Date: '2026-11-03' })
    expect(shownSpan({ ...base, dtstart_utc: '2026-10-30T05:00:00+00:00' }).start).toEqual({
      Utc: '2026-10-30T05:00:00+00:00',
    })
  })
})
