// Zod schemas for low-frequency / configuration endpoints where the cost
// of full runtime validation is negligible (called at most once per
// minute) AND drift would silently break a user-visible part of the app.
//
// Hot-path list endpoints (`/conversations`, `/mail/threads/:id`) use the
// cheaper assertObjectShape / assertArrayShape from lib/runtime-shape.ts
// instead — see that file for the perf rationale.

import { z } from 'zod'

// /api/health — read by app.tsx StatusBar (every 30s) and admin-overview
// (every 10s). Single object, ~10 fields. Cost: ~50μs per response.
export const HealthInfoSchema = z.object({
  account_cache_size: z.number().optional(),
  active_sessions: z.number().optional(),
  kevy: z.boolean(),
  level: z.number().optional(),
  pg: z.boolean(),
  status: z.string(),
  total_connections: z.number().optional(),
  total_messages: z.number().optional(),
  uptime_secs: z.number(),
  version: z.string(),
})
export type HealthInfo = z.infer<typeof HealthInfoSchema>

// /api/mail/stats — read by dashboard. Single object. Cost: ~30μs.
export const MailStatsSchema = z.object({
  categories: z
    .array(
      z.object({
        category: z.string(),
        count: z.number(),
      })
    )
    .default([]),
  storage_bytes: z.number(),
  total_messages: z.number(),
  unread_messages: z.number(),
})
export type MailStats = z.infer<typeof MailStatsSchema>

// /api/admin/system-config GET — read by the system-config admin page.
// Configuration data, low frequency, high value to verify shape (each
// entry powers a form control whose render-time field accesses would
// otherwise crash silently on drift).
export const ConfigEntrySchema = z.object({
  description: z.string(),
  group: z.string(),
  key: z.string(),
  source: z.string(),
  updated_at: z.string().nullable(),
  updated_by: z.string().nullable(),
  value: z.string(),
  value_type: z.string(),
})
export const SystemConfigResponseSchema = z.object({
  entries: z.array(ConfigEntrySchema).optional(),
  message: z.string().optional(),
  success: z.boolean(),
})
export type ConfigEntry = z.infer<typeof ConfigEntrySchema>
export type SystemConfigResponse = z.infer<typeof SystemConfigResponseSchema>
