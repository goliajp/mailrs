/**
 * The server's own observations behind the reputation judgement and the
 * attachment scan (`crates/webapi/src/handlers/screening_admin.rs`), for
 * super administrators. A refusal comes back 400 with `{error}`.
 */

import { wireFetch } from '../client'
import {
  type HashBody,
  type ObservationBody,
  recordedSchema,
  screeningListSchema,
} from '../schemas/screening'

export function wireAddScreeningHash(body: HashBody) {
  return wireFetch(recordedSchema, { body, method: 'POST', path: '/admin/screening/hashes' })
}

export function wireAddScreeningObservation(body: ObservationBody) {
  return wireFetch(recordedSchema, { body, method: 'POST', path: '/admin/screening/observations' })
}

export function wireListScreening(signal?: AbortSignal) {
  return wireFetch(screeningListSchema, { path: '/admin/screening/observations', signal })
}
