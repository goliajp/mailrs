import { z } from '@/lib/zod'

// Backend: crates/ops/src/engine/mod.rs — struct AlertView
const opsAlertSchema = z.object({
  object: z.string(),
  phase: z.string(),
  rule: z.string(),
  severity: z.string(),
  since: z.number(),
  step_en: z.string(),
  step_ja: z.string(),
  step_zh: z.string(),
  threshold: z.string().nullable(),
  title_en: z.string(),
  title_ja: z.string(),
  title_zh: z.string(),
  value: z.string().nullable(),
  why: z.string().nullable(),
})

export type WireOpsAlert = z.infer<typeof opsAlertSchema>

// Backend: crates/ops/src/store/control.rs — struct Silence
const opsSilenceSchema = z.object({
  by: z.string(),
  id: z.string(),
  object: z.string().nullable(),
  reason: z.string(),
  rule: z.string(),
  set_at: z.number(),
  until: z.number(),
})

export type WireOpsSilence = z.infer<typeof opsSilenceSchema>

// Backend: crates/ops/src/store/control.rs — struct Maintenance
const opsMaintenanceSchema = z.object({
  by: z.string(),
  reason: z.string(),
  started_at: z.number(),
  until: z.number(),
})

// Backend: crates/ops/src/app.rs — fn state
const opsDaemonStateSchema = z.object({
  alerts: z.array(opsAlertSchema),
  external_channel: z.boolean(),
  last_cycle: z.object({ duration_ms: z.number(), started_at: z.number() }).nullable(),
  maintenance: opsMaintenanceSchema.nullable(),
  notifications: z.object({
    held_for_maintenance: z.number(),
    own_path_awaiting_acceptance_250: z.array(z.string()),
  }),
  rules: z.array(
    z.object({ name: z.string(), title_en: z.string(), title_ja: z.string(), title_zh: z.string() })
  ),
  silences: z.array(opsSilenceSchema),
  unmonitored: z.boolean(),
})

// Backend: crates/webapi/src/handlers/admin_ops_daemon/link.rs — fn read_state
export const opsStateSchema = z.discriminatedUnion('reach', [
  z.object({
    reach: z.literal('ok'),
    silent_after_secs: z.number(),
    stale: z.boolean(),
    state: opsDaemonStateSchema,
  }),
  z.object({ reach: z.literal('not_configured') }),
  z.object({ detail: z.string(), reach: z.literal('unreachable') }),
  z.object({ reach: z.literal('refused') }),
])

export type WireOpsDaemonState = z.infer<typeof opsDaemonStateSchema>
export type WireOpsState = z.infer<typeof opsStateSchema>

// Backend: crates/ops/src/app.rs — fn history (store/alerts.rs Transition,
// store/log.rs NotificationRow, store/control.rs control_events_since)
export const opsHistorySchema = z.object({
  controls: z.array(
    z.object({
      action: z.string(),
      at: z.number(),
      by: z.string(),
      kind: z.string(),
      object: z.string().nullable().optional(),
      reason: z.string(),
      rule: z.string().optional(),
      until: z.number(),
    })
  ),
  notifications: z.array(
    z.object({
      at: z.number(),
      channel: z.string(),
      group_id: z.string(),
      result: z.string(),
      summary: z.string(),
    })
  ),
  transitions: z.array(
    z.object({
      at: z.number(),
      from: z.string(),
      object: z.string(),
      reason: z.string(),
      rule: z.string(),
      to: z.string(),
      value: z.string().nullable(),
    })
  ),
})

export type WireOpsHistory = z.infer<typeof opsHistorySchema>

// Backend: crates/ops/src/api/admin.rs — fn set_silence (the stored Silence)
export const opsSilenceSetSchema = opsSilenceSchema

// Backend: crates/ops/src/api/admin.rs — fn end_silence and fn maintenance
export const opsDoneSchema = z.object({}).passthrough()
