import { wireFetch } from '../client'
import { calendarEventsSchema, type WireCalendarEvent } from '../schemas/calendar'

/**
 * Every event overlapping `[start, end)`: the reader's own, invitations
 * filed from mail with their answer, and subscribed feeds
 * (`crates/webapi/src/handlers/calendar.rs` get_events).
 */
export function wireCalendarEvents(
  start: Date,
  end: Date,
  signal?: AbortSignal
): Promise<WireCalendarEvent[]> {
  const params = new URLSearchParams({ end: end.toISOString(), start: start.toISOString() })
  return wireFetch(calendarEventsSchema, { path: `/calendar/events?${params.toString()}`, signal })
}
