/**
 * The batches re-judging took back, and the super administrator's
 * decisions on them. Each write needs a recent password confirmation,
 * which `http` asks for when the server wants it.
 */

import type { WireRecallBatch } from '../schemas/rejudge'

import { wireFetch } from '../client'
import {
  wireRecallBatchConfirmedSchema,
  wireRecallBatchDiscardedSchema,
  wireRecallBatchesResponseSchema,
  wireRecallBatchReleasedSchema,
} from '../schemas/rejudge'

const BASE = '/admin/rejudge/batches'

/** `GET /api/admin/rejudge/batches`, newest first */
export async function fetchRecallBatches(signal?: AbortSignal): Promise<WireRecallBatch[]> {
  const raw = await wireFetch(wireRecallBatchesResponseSchema, { path: BASE, signal })
  return raw.items
}

/** `POST /api/admin/rejudge/batches/{observation}/confirm`; how many were held */
export async function wireConfirmRecallBatch(observation: string): Promise<number> {
  const raw = await wireFetch(wireRecallBatchConfirmedSchema, {
    method: 'POST',
    path: `${BASE}/${encodeURIComponent(observation)}/confirm`,
  })
  return raw.held
}

/** `POST /api/admin/rejudge/batches/{observation}/discard`; how many it would have held */
export async function wireDiscardRecallBatch(observation: string): Promise<number> {
  const raw = await wireFetch(wireRecallBatchDiscardedSchema, {
    method: 'POST',
    path: `${BASE}/${encodeURIComponent(observation)}/discard`,
  })
  return raw.would_hold
}

/** `POST /api/admin/rejudge/batches/{observation}/release`; how many were put back */
export async function wireReleaseRecallBatch(observation: string): Promise<number> {
  const raw = await wireFetch(wireRecallBatchReleasedSchema, {
    method: 'POST',
    path: `${BASE}/${encodeURIComponent(observation)}/release`,
  })
  return raw.released
}
