/**
 * The admin overview's mail flow.
 *
 * Backend: crates/webapi/src/handlers/admin_flow.rs — fn flow
 * A stage is `null` when its owner could not be read, which is not zero.
 */

import { z } from 'zod'

export const adminFlowSchema = z.object({
  mock: z.object({ bytes: z.number(), max_bytes: z.number(), received: z.number() }).nullable(),
  outbound: z
    .object({
      failed: z.number(),
      queued: z.number(),
      retrying: z.number(),
      scheduled: z.number(),
    })
    .nullable(),
  receiving: z
    .object({
      active_connections: z.number(),
      connections: z.number(),
      spooled: z.number(),
      uptime_secs: z.number(),
    })
    .nullable(),
  spool_backlog: z.number().nullable(),
})

export type AdminFlow = z.infer<typeof adminFlowSchema>
