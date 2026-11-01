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

export const notifyOverridesSchema = z.object({
  senders: z.array(z.tuple([z.string(), z.string()])),
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
})

export type WireNotifyPrefs = z.infer<typeof notifyPrefsSchema>
export type WireNotifyStats = z.infer<typeof notifyStatsSchema>
