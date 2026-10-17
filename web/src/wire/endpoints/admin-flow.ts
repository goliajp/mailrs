/** Backend: crates/webapi/src/handlers/admin_flow.rs */

import type { AdminFlow } from '../schemas/admin-flow'

import { wireFetch } from '../client'
import { adminFlowSchema } from '../schemas/admin-flow'

/** `GET /api/admin/flow` — where mail is in the server right now. */
export function wireAdminFlow(signal?: AbortSignal): Promise<AdminFlow> {
  return wireFetch(adminFlowSchema, { path: '/admin/flow', signal })
}
