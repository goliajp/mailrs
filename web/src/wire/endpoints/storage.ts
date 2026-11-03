/** How much of the signed-in user's quota their mail takes (`crates/webapi/src/handlers/storage.rs`). */

import type { WireStorage } from '../schemas/storage'

import { wireFetch } from '../client'
import { storageSchema } from '../schemas/storage'

export function wireGetStorage(signal?: AbortSignal): Promise<WireStorage> {
  return wireFetch(storageSchema, { path: '/storage', signal })
}
