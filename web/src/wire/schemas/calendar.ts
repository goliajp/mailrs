import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/calendar.rs — fn get_events (EventRow). The optional
// columns come back as null when empty; `partstat` is absent until the
// reader answers.
export const calendarEventSchema = z.object({
  dtend: z.string().nullable().default(null),
  dtstart: z.string().nullable().default(null),
  /** The Message-ID of the mail the invitation came in; absent for other events. */
  message_id: z.string().optional(),
  organizer: z.string().nullable().default(null),
  partstat: z.enum(['ACCEPTED', 'TENTATIVE', 'DECLINED']).optional(),
  source: z.string().nullable().default(null),
  status: z.string().nullable().default(null),
  summary: z.string().default(''),
  /** The conversation that mail is in now; absent when there is none to open. */
  thread_id: z.string().optional(),
  uid: z.string(),
})

export const calendarEventsSchema = z.array(calendarEventSchema)

export type WireCalendarEvent = z.infer<typeof calendarEventSchema>
