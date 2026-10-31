/**
 * Admin resource wire schemas — v2.1 §10.3 (2026-07-08).
 *
 * Strategy: admin pages already deal with permissive backend
 * shapes and error-tolerant rendering. Instead of pinning every
 * field for 12 resource types across ~40 endpoints, use a
 * uniform `passthrough()` pattern that catches gross type errors
 * (arrays vs objects, missing top-level envelope) but forwards
 * whatever fields the caller reads.
 *
 * Individual resources can graduate to tight schemas as needed —
 * the migration boundary is: any downstream `x.someField` access
 * that turns out wrong under Zod parse can tighten the schema
 * here without ripping wire boundary out of the codebase.
 */

import { z } from '@/lib/zod'

/**
 * The universal list-envelope shape. Every admin list endpoint
 * follows one of these — bare array or `{items: [...]}`. Content
 * types are permissive; caller reads specific fields.
 */
export const adminListSchema = z.union([
  z.object({ items: z.array(z.record(z.string(), z.unknown())) }),
  z.array(z.record(z.string(), z.unknown())).transform((items) => ({ items })),
])

/** A single object response. */
export const adminObjectSchema = z.record(z.string(), z.unknown())

/** Empty response for admin mutations that don't return content. */
export const adminEmptyResultSchema = z.union([
  z.undefined(),
  z.object({ success: z.boolean().optional() }).passthrough(),
])

/**
 * The `{success, message?, ...}` envelope some admin endpoints return
 * as a permissive object.
 */
export const adminResultSchema = z
  .object({
    error: z.string().optional(),
    message: z.string().optional(),
    success: z.boolean().optional(),
  })
  .passthrough()

export type WireAdminResult = z.infer<typeof adminResultSchema>

/**
 * A write's answer: the envelope above, or nothing at all. Most admin
 * writes answer 204, and checking that empty body against the envelope
 * failed every one of them after the write had succeeded.
 */
export const adminWriteResultSchema = z.union([z.undefined(), adminResultSchema])

/**
 * A list of strings, under whichever key the endpoint uses —
 * `{items}`, `{permissions}`, `{members}`, `{groups}` — or bare.
 */
export const adminStringListSchema = z.union([
  z.array(z.string()),
  z
    .record(z.string(), z.unknown())
    .transform((o) => Object.values(o).find((v) => Array.isArray(v)) ?? [])
    .pipe(z.array(z.string())),
])

// Backend: crates/webapi/src/handlers/admin_password_clear.rs — fn preview
export const passwordClearPreviewSchema = z.object({
  issuers_configured: z.boolean(),
  items: z.array(
    z.object({
      address: z.string(),
      agent_keys: z.array(
        z.object({
          name: z.string(),
          prefix: z.string(),
          scopes: z.array(z.string()),
        })
      ),
      cleared_at: z.number().nullable(),
      exists: z.boolean(),
      external_logins: z.number(),
      sessions: z.number(),
      smtp_password_logins_7d: z.number(),
      totp: z.boolean(),
    })
  ),
})

export type WirePasswordClearPreview = z.infer<typeof passwordClearPreviewSchema>

// Backend: crates/webapi/src/handlers/admin_password_clear.rs — fn clear
export const passwordClearResultSchema = z.object({
  items: z.array(
    z.object({
      address: z.string(),
      result: z.enum(['cleared', 'unknown']),
      sessions_revoked: z.number(),
    })
  ),
})

export type WirePasswordClearResult = z.infer<typeof passwordClearResultSchema>

// Backend: crates/webapi/src/handlers/admin_directory/password_reset.rs — struct KeyRow
export const accountAgentKeySchema = z.object({
  created_at: z.number().nullable(),
  id: z.number(),
  last_used_at: z.number().nullable(),
  name: z.string(),
  prefix: z.string(),
  scopes: z.array(z.string()),
})

export type WireAccountAgentKey = z.infer<typeof accountAgentKeySchema>

// Backend: crates/webapi/src/handlers/admin_directory/accounts.rs — fn list_account_agent_keys
export const accountAgentKeysSchema = z.object({
  items: z.array(accountAgentKeySchema),
  last_use_recorded_since: z.string(),
})

export type WireAccountAgentKeys = z.infer<typeof accountAgentKeysSchema>

// Backend: crates/webapi/src/handlers/admin_directory/accounts.rs — fn set_account_password
export const compromiseResetDoneSchema = z.object({
  agent_keys_revoked: z.number(),
  sessions_revoked: z.number(),
})

export const keyListChangedSchema = z.object({
  error: z.literal('key_list_changed'),
  keys: z.array(accountAgentKeySchema),
})

// Backend: crates/core-sidestate/src/families/outbound/pause.rs — struct Paused
export const outboundPausedSchema = z.object({
  items: z.array(
    z.object({
      address: z.string(),
      held: z.number(),
      reason: z.string(),
      since: z.number(),
    })
  ),
})

export type WireOutboundPaused = z.infer<typeof outboundPausedSchema>

// Backend: crates/webapi/src/handlers/admin_outbound_pause.rs — fn resume
export const outboundResumedSchema = z.object({
  discarded: z.number(),
  sent: z.number(),
})

export type WireOutboundResumed = z.infer<typeof outboundResumedSchema>
