/**
 * The signed-in user's labels (`crates/webapi/src/handlers/labels.rs`).
 * A refused name or colour comes back 400, a taken name 409, each with
 * `{error}` saying why; `wireFetch` surfaces that text.
 */

import { wireFetch } from '../client'
import { labelSchema, labelsSchema, type WireLabel } from '../schemas/labels'
import { emptyResponseSchema } from '../schemas/mutations'

export function wireCreateLabel(name: string, color: string): Promise<WireLabel> {
  return wireFetch(labelSchema, { body: { color, name }, method: 'POST', path: '/labels' })
}

export async function wireDeleteLabel(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/labels/${encodeURIComponent(id)}`,
  })
}

export function wireListLabels(signal?: AbortSignal): Promise<WireLabel[]> {
  return wireFetch(labelsSchema, { path: '/labels', signal })
}

/** Put label `id` on the conversation, or take it off. */
export async function wireSetThreadLabel(threadId: string, id: string, on: boolean): Promise<void> {
  let method: 'DELETE' | 'POST' = 'DELETE'
  if (on) method = 'POST'
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method,
    path: `/conversations/${encodeURIComponent(threadId)}/labels/${encodeURIComponent(id)}`,
  })
}

export function wireUpdateLabel(
  id: string,
  change: { color?: string; name?: string }
): Promise<WireLabel> {
  return wireFetch(labelSchema, {
    body: change,
    method: 'POST',
    path: `/labels/${encodeURIComponent(id)}`,
  })
}
