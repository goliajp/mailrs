import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/rules.rs — fn list / fn create / fn update
// (the type is crates/core-sidestate/src/families/rules.rs `Rule`)
export const ruleSchema = z.object({
  actions: z.array(z.string()),
  created_at: z.number(),
  enabled: z.boolean(),
  id: z.string(),
  name: z.string(),
  query: z.string(),
})

export type WireRule = z.infer<typeof ruleSchema>

export const rulesSchema = z.array(ruleSchema)

// Backend: crates/webapi/src/handlers/rules_run.rs — fn preview
export const rulePreviewSchema = z.object({
  count: z.number(),
  items: z.array(z.object({ subject: z.string(), thread_id: z.string() })),
})

// Backend: crates/webapi/src/handlers/rules_run.rs — fn run
export const ruleRunSchema = z.object({
  changed: z.number(),
  failed: z.number(),
  run_id: z.string(),
})

// Backend: crates/webapi/src/handlers/rules_run.rs — fn undo_run
export const ruleUndoSchema = z.object({ failed: z.number(), restored: z.number() })

/** What a rule is made of, as the server takes it. */
export type RuleBody = { actions: string[]; enabled: boolean; name: string; query: string }
