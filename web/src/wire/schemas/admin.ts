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

import { z } from 'zod'

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
