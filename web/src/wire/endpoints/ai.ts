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

export const wireReplySuggest = (payload: {
  original_body?: string
  sender?: string
  subject?: string
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
