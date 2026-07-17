import { z } from 'zod'

import { wireFetch } from '../client'

// Backend: crates/webapi/src/handlers/prefs.rs:439 — pub async fn get_contacts
// Returns: Json<Vec<String>> — a BARE array of display strings
//   (e.g. ["Srinidhi Gundappa <sgundapp@qti.qualcomm.com>", ...]).
//
// The contact autocomplete previously fetched this via `adminListGet`,
// whose `adminListSchema` requires an array of OBJECTS
// (z.array(z.record(...))). A bare string array fails that Zod parse,
// wireFetch throws, and the autocomplete's catch swallows it → no
// suggestions ever appear. This schema tolerates both the bare array
// and an {items:[...]} envelope, of strings.
//
// Verified 2026-07-15 against a live prod response for
// GET /api/contacts?q=cheng&limit=5.
export const contactSuggestionsSchema = z.union([
  z.array(z.string()),
  z.object({ items: z.array(z.string()) }).transform((v) => v.items),
])

/**
 * GET /api/contacts?q=<query>&limit=<n> — recipient autocomplete.
 * Returns display strings ranked by the backend, deduped + truncated
 * to `limit`.
 */
export async function fetchContactSuggestions(
  query: string,
  limit = 5,
  signal?: AbortSignal
): Promise<string[]> {
  return wireFetch(contactSuggestionsSchema, {
    path: `/contacts?q=${encodeURIComponent(query)}&limit=${limit}`,
    signal,
  })
}
