import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/notify.rs and
// crates/core-sidestate/src/families/notify.rs
export const notifyPrefsSchema = z.object({
  hide_content: z.boolean().default(false),
  mode: z.enum(['default', 'all', 'important', 'off']).default('default'),
  quiet_end: z.number().optional(),
  quiet_start: z.number().optional(),
  utc_offset_minutes: z.number().default(0),
})

// Backend: crates/webapi/src/handlers/notify.rs — fn overrides
export const notifyOverridesSchema = z.object({
  senders: z.array(z.tuple([z.string(), z.string()])),
  // each conversation's choice with its subject; null when the reader no
  // longer has the conversation. Absent from a server older than the field.
  thread_details: z
    .array(z.object({ subject: z.string().nullable(), thread_id: z.string(), value: z.string() }))
    .default([]),
  threads: z.array(z.tuple([z.string(), z.string()])),
})

export const notifyStatsSchema = z.array(
  z.object({
    counts: z.record(z.string(), z.record(z.string(), z.number())),
    day: z.string(),
  })
)

export const importantSchema = z.array(z.string())

// Backend: crates/webapi/src/handlers/web_push.rs
export const webPushStateSchema = z.object({
  enabled: z.boolean(),
  public_key: z.string().optional(),
  removed: z.array(z.string()),
})

export type WireNotifyPrefs = z.infer<typeof notifyPrefsSchema>
export type WireNotifyStats = z.infer<typeof notifyStatsSchema>
