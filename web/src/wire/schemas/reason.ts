/**
 * One reason a message was judged as it was.
 *
 * Backend: `mailrs_inbound::reason::Reason` (crates/inbound/src/reason.rs):
 * `code` is a fraud rule id or one of that module's codes; `params` is
 * left out when empty.
 */

import { z } from '@/lib/zod'

export const wireReasonSchema = z.object({
  code: z.string(),
  params: z.record(z.string(), z.string()).optional(),
})

export type WireReason = z.infer<typeof wireReasonSchema>
