/**
 * The review screen's reads and its one write.
 */

import { z } from 'zod'

import { wireFetch } from '../client'
import { type WireFraudVerdict, wireFraudVerdictResponseSchema } from '../schemas/fraud'

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
