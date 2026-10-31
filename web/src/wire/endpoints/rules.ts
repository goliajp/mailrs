/**
 * The signed-in user's rules (`crates/webapi/src/handlers/rules.rs`,
 * `rules_run.rs`). A refusal comes back 400 or 409 with `{error}` naming
 * why: `empty_query`, `operator_not_for_rules`, `invalid_action`,
 * `invalid_name`, `name_taken`, `too_many`.
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  type RuleBody,
  rulePreviewSchema,
  ruleRunSchema,
  ruleSchema,
  rulesSchema,
  ruleUndoSchema,
  type WireRule,
} from '../schemas/rules'

export function wireApplyRule(id: string) {
  return wireFetch(ruleRunSchema, {
    method: 'POST',
    path: `/rules/${encodeURIComponent(id)}/apply`,
  })
}

export function wireCreateRule(body: RuleBody): Promise<WireRule> {
  return wireFetch(ruleSchema, { body, method: 'POST', path: '/rules' })
}

export async function wireDeleteRule(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/rules/${encodeURIComponent(id)}`,
  })
}

export function wireListRules(signal?: AbortSignal): Promise<WireRule[]> {
  return wireFetch(rulesSchema, { path: '/rules', signal })
}

export function wirePreviewRule(query: string) {
  return wireFetch(rulePreviewSchema, { body: { query }, method: 'POST', path: '/rules/preview' })
}

export function wireUndoRuleRun(runId: string) {
  return wireFetch(ruleUndoSchema, {
    method: 'POST',
    path: `/rules/runs/${encodeURIComponent(runId)}/undo`,
  })
}

export function wireUpdateRule(id: string, body: RuleBody): Promise<WireRule> {
  return wireFetch(ruleSchema, { body, method: 'POST', path: `/rules/${encodeURIComponent(id)}` })
}
