/**
 * The signed-in user's saved searches
 * (`crates/webapi/src/handlers/saved_searches.rs`). A refused name or query
 * comes back 400, a taken name 409, each with `{error}` saying why.
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  savedSearchesSchema,
  savedSearchSchema,
  type WireSavedSearch,
} from '../schemas/saved-searches'

export function wireCreateSavedSearch(name: string, query: string): Promise<WireSavedSearch> {
  return wireFetch(savedSearchSchema, {
    body: { name, query },
    method: 'POST',
    path: '/saved-searches',
  })
}

export async function wireDeleteSavedSearch(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/saved-searches/${encodeURIComponent(id)}`,
  })
}

export function wireListSavedSearches(signal?: AbortSignal): Promise<WireSavedSearch[]> {
  return wireFetch(savedSearchesSchema, { path: '/saved-searches', signal })
}
