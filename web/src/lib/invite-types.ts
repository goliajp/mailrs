// The shapes the invitation endpoints answer with, shared by the card
// and by the rules beside it.
//
// Backend: `crates/webapi/src/handlers/complete.rs` —
// `get_message_single`'s `invite_payload`, which is
// `mailrs_ical::ParsedInvite` plus the two resolved instants, and what
// `calendar_events::series::annotate` adds each time it is read.
import type { NextOccurrence, RecurrenceRule } from '@/lib/invite-recurrence'
import type { CalDateTime } from '@/lib/invite-time'

export type Attendee = {
  cn: null | string
  email: string
  partstat: string
  role: string
  rsvp: boolean
}

export type InvitePayload = {
  attendees?: Attendee[]
  description?: null | string
  dtend?: CalDateTime | null
  /// The instant, resolved on the server against the invitation's own
  /// VTIMEZONE. Absent for an all-day event, which has no instant, and
  /// for mail ingested before 2026-08-20.
  dtend_utc?: null | string
  dtstart?: CalDateTime
  dtstart_utc?: null | string
  /// The way into the meeting, resolved on the server. RFC 5545 has no
  /// field for it and producers put it wherever, so one implementation
  /// rather than three.
  join_url?: null | string
  location?: null | string
  method: string
  /// For a recurring invitation, the first occurrence that has not ended
  /// as of the read; `null` once the series is over.
  next_occurrence?: NextOccurrence | null
  /// The IANA zone the organiser's `TZID` names, Windows names resolved.
  organiser_zone?: null | string
  organizer?: null | Person
  /// For a recurring invitation, its rule read for showing in words.
  recurrence?: null | RecurrenceRule
  recurrence_id?: CalDateTime | null
  rrule?: null | string
  sequence: number
  status?: null | string
  summary: string
  uid: string
}

export type Person = { cn: null | string; email: string }
