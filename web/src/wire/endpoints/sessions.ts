/**
 * The signed-in user's sessions (`crates/webapi/src/handlers/auth/sessions.rs`).
 * A session is named by the first characters of a hash of its token.
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import { endedSchema, sessionsSchema } from '../schemas/sessions'

export async function wireEndOtherSessions() {
  return wireFetch(endedSchema, { method: 'POST', path: '/auth/sessions/end-others' })
}

export async function wireEndSession(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/auth/sessions/${encodeURIComponent(id)}`,
  })
}

export function wireListSessions(signal?: AbortSignal) {
  return wireFetch(sessionsSchema, { path: '/auth/sessions', signal })
}
