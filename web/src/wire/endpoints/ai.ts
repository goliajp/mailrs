/**
 * AI feature wire endpoints — v2.1 §10.1 (2026-07-08).
 */

import { wireFetch } from '../client'
import {
  generateSubjectResultSchema,
  polishResultSchema,
  replySuggestResultSchema,
  type WireGenerateSubjectResult,
  type WirePolishResult,
  type WireReplySuggestResult,
} from '../schemas/ai'

export const wirePolishText = (text: string, tone?: string): Promise<WirePolishResult> =>
  wireFetch(polishResultSchema, {
    body: tone ? { text, tone } : { text },
    method: 'POST',
    path: '/mail/ai/polish',
  })

/**
 * Backend: crates/webapi/src/handlers/ai.rs — `ai_reply_suggest`, taking
 * `mailrs_intelligence::assist::ReplySuggestRequest`.
 *
 * The three `original_*` fields are required there. This function used to
 * send `sender` and `subject`, which serde dropped, leaving
 * `original_sender` missing — a 422 on every call, so the Suggest button
 * had never worked on the lane that had the route at all.
 */
export const wireReplySuggest = (payload: {
  original_body: string
  original_sender: string
  original_subject: string
  thread_context?: string
}): Promise<WireReplySuggestResult> =>
  wireFetch(replySuggestResultSchema, {
    body: payload,
    method: 'POST',
    path: '/mail/ai/reply-suggest',
  })

export const wireGenerateSubject = (payload: {
  body: string
  context?: string
}): Promise<WireGenerateSubjectResult> =>
  wireFetch(generateSubjectResultSchema, {
    body: payload,
    method: 'POST',
    path: '/mail/ai/generate-subject',
  })
