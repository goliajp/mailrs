/**
 * One reason a message was judged as it was.
 *
 * Backend: `mailrs_inbound::reason::Reason` (crates/inbound/src/reason.rs):
 * `code` is a fraud rule id or one of that module's codes; `params` is
 * left out when empty. `score` is set on a fraud verdict layer's lines:
 * what each added, so a layer's score is the sum of its lines; left out
 * on a line that adds nothing, on placement, and on older verdicts.
 */

import { z } from '@/lib/zod'

export const wireReasonSchema = z.object({
  code: z.string(),
  params: z.record(z.string(), z.string()).optional(),
  score: z.number().optional(),
})

export type WireReason = z.infer<typeof wireReasonSchema>
