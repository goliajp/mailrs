// The Teams invitation from the bug report, as `GET /api/mail/messages/{uid}`
// serves it after `calendar_events::series::annotate`: weekly on Thursday
// 22:00 Pacific, effective 2026-10-29 until 2027-04-29. The shapes follow
// crates/core-sidestate/tests/recurring_rows.rs, which pins what the
// server writes for this series.
import type { InvitePayload } from '@/lib/invite-types'

/// The next occurrence the server reports at a given read: the Thursday
/// before the US clocks go back (05:00 UTC) or after (06:00 UTC).
export function recurringDetail(next: 'after-change' | 'before-change' | 'ended') {
  return {
    invite_method: 'REQUEST',
    invite_payload: recurringPayload(next),
    rsvp_status: null,
    uid: 9,
  }
}

function nextOccurrence(next: 'after-change' | 'before-change' | 'ended') {
  if (next === 'ended') return null
  if (next === 'before-change') {
    return {
      end: '2026-10-30T06:00:00+00:00',
      organiser_local: '2026-10-29T22:00:00',
      organiser_offset_secs: -7 * 3600,
      start: '2026-10-30T05:00:00+00:00',
    }
  }
  return {
    end: '2026-11-06T07:00:00+00:00',
    organiser_local: '2026-11-05T22:00:00',
    organiser_offset_secs: -8 * 3600,
    start: '2026-11-06T06:00:00+00:00',
  }
}

function recurringPayload(next: 'after-change' | 'before-change' | 'ended'): InvitePayload {
  return {
    attendees: [],
    dtend: { Zoned: { local: '2026-10-29T23:00:00', tz_name: 'Pacific Standard Time' } },
    dtend_utc: '2026-10-30T06:00:00+00:00',
    dtstart: { Zoned: { local: '2026-10-29T22:00:00', tz_name: 'Pacific Standard Time' } },
    dtstart_utc: '2026-10-30T05:00:00+00:00',
    join_url: null,
    method: 'REQUEST',
    next_occurrence: nextOccurrence(next),
    organiser_zone: 'America/Los_Angeles',
    organizer: { cn: 'Chair', email: 'chair@example.com' },
    recurrence: {
      by_day: ['TH'],
      by_month: [],
      by_month_day: [],
      count: null,
      freq: 'WEEKLY',
      interval: 1,
      plain: true,
      until: '2027-04-29',
    },
    recurrence_id: null,
    rrule: 'FREQ=WEEKLY;UNTIL=20270430T050000Z;INTERVAL=1;BYDAY=TH;WKST=SU',
    sequence: 0,
    status: null,
    summary: 'GOLIA sync',
    uid: 'golia-sync',
  }
}
