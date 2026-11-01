import type { z } from '@/lib/zod'

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  reclassifyAnswerSchema,
  ruleAnswerSchema,
  type SenderBucket,
  senderRulesSchema,
  undoAnswerSchema,
  type WireSenderRule,
} from '../schemas/sender-rules'

export type SenderRuleBody = Pick<
  WireSenderRule,
  'bucket' | 'disposal' | 'keep_latest' | 'older_than_days'
>

export async function wireDeleteSenderRule(address: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/sender-rules/${encodeURIComponent(address)}`,
  })
}

export function wireListSenderRules(): Promise<WireSenderRule[]> {
  return wireFetch(senderRulesSchema, { path: '/sender-rules' })
}

/** Save a rule; a retention applies at once and the answer says how much it let go of. */
export function wirePutSenderRule(
  address: string,
  body: SenderRuleBody
): Promise<z.infer<typeof ruleAnswerSchema>> {
  return wireFetch(ruleAnswerSchema, {
    body,
    method: 'PUT',
    path: `/sender-rules/${encodeURIComponent(address)}`,
  })
}

/** Move a sender's conversations to `bucket` and remember it. */
export function wireReclassifySender(
  sender: string,
  bucket: SenderBucket
): Promise<z.infer<typeof reclassifyAnswerSchema>> {
  return wireFetch(reclassifyAnswerSchema, {
    body: { bucket, sender },
    method: 'POST',
    path: '/senders/reclassify',
  })
}

/** Put back what a sender correction moved. */
export function wireUndoCorrection(id: string): Promise<z.infer<typeof undoAnswerSchema>> {
  return wireFetch(undoAnswerSchema, {
    method: 'POST',
    path: `/senders/corrections/${encodeURIComponent(id)}/undo`,
  })
}
