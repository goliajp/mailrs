import { describe, expect, it } from 'vitest'

import {
  formatDateTime,
  formatEpochOrIso,
  formatOrganiserTime,
  isDateOnly,
  toLocalDate,
  zoneNameOf,
} from '@/lib/invite-time'

describe('invite times', () => {
  // The defect this file exists for. A `Zoned` wall-clock has no offset,
  // and reading it as UTC is what put a 16:00 meeting in Santa Clara at
  // 01:00 the next morning in Tokyo — seven hours out, silently. The
  // instant now arrives resolved from the server.
  it('reads the resolved instant, not the wall-clock string', () => {
    const resolved = toLocalDate({ Utc: '2026-08-20T23:00:00Z' })
    expect(resolved?.toISOString()).toBe('2026-08-20T23:00:00.000Z')

    // The same event's wall-clock, which carries no offset of its own
    // and is read as the reader's local time.
    //
    // Asserted against a locally-built Date, **not** as "is not 16:00
    // UTC": on a machine in UTC those two are the same string, so that
    // assertion passed only because the laptops here are in Tokyo. CI
    // is UTC and it failed there the first time it ran — 2026-08-26,
    // six days after this file was written and one web release later.
    const wall = toLocalDate({
      Zoned: { local: '2026-08-20T16:00:00', tz_name: 'Pacific Standard Time' },
    })
    expect(wall?.getTime()).toBe(new Date(2026, 7, 20, 16, 0, 0).getTime())
  })

  // An all-day event has no offset. Giving it one is how it lands on the
  // wrong day for a reader west of the organiser.
  it('never gives an all-day event a time', () => {
    const dt = { Date: '2026-08-20' }
    expect(isDateOnly(dt)).toBe(true)
    expect(formatDateTime(dt)).not.toMatch(/\d:\d\d/)
  })

  // Gmail shows both zones when they differ, and it is right to: "08:00"
  // alone is a different claim from "08:00 here, 16:00 where it was
  // scheduled".
  it('names the organiser zone only when it is not the reader own', () => {
    const pacific = { Zoned: { local: '2026-08-20T16:00:00', tz_name: 'Pacific Standard Time' } }
    const reader = Intl.DateTimeFormat().resolvedOptions().timeZone
    const shown = formatOrganiserTime(pacific, zoneNameOf(pacific))
    if (reader === 'America/Los_Angeles') {
      expect(shown).toBeNull()
    } else {
      expect(shown).toBe('16:00 Pacific Standard Time')
    }

    // A UTC time carries no zone name, so there is nothing to add.
    expect(formatOrganiserTime({ Utc: '2026-08-20T23:00:00Z' }, null)).toBeNull()
  })

  it('reads a floating time as the reader own local time', () => {
    // No zone at all means "local to whoever is reading" (RFC 5545
    // 3.3.5), so it must not be shifted.
    const d = toLocalDate({ Floating: '2026-08-20T16:00:00' })
    expect(d?.getHours()).toBe(16)
  })

  // What the card printed on production the first time anybody answered
  // an invitation: "You accepted 1787223869". `rsvp_at` is Unix seconds
  // in a string — the server writes `now_secs().to_string()` — and the
  // ISO formatter handed the digits straight back.
  it('reads a stored answer time, which is seconds and not ISO', () => {
    const shown = formatEpochOrIso('1787223869')
    expect(shown).not.toBe('1787223869')
    expect(shown).toMatch(/2026/)

    // And still reads an ISO string, which is what everything else sends.
    expect(formatEpochOrIso('2026-08-20T23:00:00Z')).toMatch(/2026/)
    expect(formatEpochOrIso(null)).toBe('')
  })
})
