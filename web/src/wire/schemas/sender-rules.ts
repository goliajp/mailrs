import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/sender_rules.rs
export const senderRuleSchema = z.object({
  address: z.string(),
  bucket: z.enum(['inbox', 'notifications', 'promotions']).optional(),
  created_at: z.number().default(0),
  disposal: z.enum(['archive', 'trash']).default('archive'),
  keep_latest: z.number().optional(),
  older_than_days: z.number().optional(),
  source: z.string().default(''),
})

export const senderRulesSchema = z.array(senderRuleSchema)

export const reclassifyAnswerSchema = z.object({
  correction_id: z.string().optional(),
  moved: z.number(),
})

export const undoAnswerSchema = z.object({ restored: z.number() })

export const ruleAnswerSchema = z.object({ disposed: z.number() })

export type SenderBucket = NonNullable<WireSenderRule['bucket']>
export type WireSenderRule = z.infer<typeof senderRuleSchema>
