import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/calendar.rs — fn get_events (EventRow). The optional
// columns come back as null when empty; `partstat` is absent until the
// reader answers.
export const calendarEventSchema = z.object({
  dtend: z.string().nullable().default(null),
  dtstart: z.string().nullable().default(null),
  organizer: z.string().nullable().default(null),
  partstat: z.enum(['ACCEPTED', 'TENTATIVE', 'DECLINED']).optional(),
  source: z.string().nullable().default(null),
  status: z.string().nullable().default(null),
  summary: z.string().default(''),
  uid: z.string(),
})

export const calendarEventsSchema = z.array(calendarEventSchema)

export type WireCalendarEvent = z.infer<typeof calendarEventSchema>
