/**
 * The review screen's reads and its one write.
 */

import { z } from 'zod'

import { wireFetch } from '../client'
import {
  type WireFraudVerdict,
  wireFraudVerdictResponseSchema,
  wireQuarantineCountSchema,
} from '../schemas/fraud'

/**
 * `GET /api/messages/{messageId}/fraud-verdict`
 *
 * `null` when nothing was found — which is almost every message.
 */
export async function fetchFraudVerdict(
  messageId: string,
  signal?: AbortSignal
): Promise<null | WireFraudVerdict> {
  const raw = await wireFetch(wireFraudVerdictResponseSchema, {
    path: `/messages/${encodeURIComponent(messageId)}/fraud-verdict`,
    signal,
  })
  return raw.verdict
}

/**
 * `GET /api/quarantine?limit=0` — how many are held, and no rows.
 *
 * Backend: crates/webapi/src/handlers/quarantine.rs — `list_quarantine`,
 * returning `{items, total}`. `total` is an index count the walk
 * produces anyway, so a limit of zero costs a count and hydrates
 * nothing.
 *
 * It exists because the tab said `Review` and nothing else. The route
 * was capped at 200 while 439 were held, so a reader was shown a page
 * with no way to tell it from the whole — the same shape as every
 * other number this month that could not come out wrong.
 */
export async function fetchQuarantineCount(signal?: AbortSignal): Promise<number> {
  const raw = await wireFetch(wireQuarantineCountSchema, {
    path: '/quarantine?limit=0',
    signal,
  })
  return raw.total
}

/**
 * `POST /api/quarantine/{threadId}/release` — it was not fraud.
 *
 * Returns the conversation to whatever list it belonged to. The stored
 * verdict is left alone: it records what was decided then, and a
 * person disagreeing later does not change what the rules saw.
 */
export async function releaseQuarantined(threadId: string): Promise<void> {
  await wireFetch(z.void(), {
    allowEmpty: true,
    method: 'POST',
    path: `/quarantine/${encodeURIComponent(threadId)}/release`,
  })
}
