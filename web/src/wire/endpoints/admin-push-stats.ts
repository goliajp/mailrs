/** Backend: crates/webapi/src/handlers/admin_push_stats.rs */

import type { AdminPushStats } from '../schemas/admin-push-stats'

import { wireFetch } from '../client'
import { adminPushStatsSchema } from '../schemas/admin-push-stats'

/** `GET /api/admin/push-stats` — pushes per channel per day, newest day first. */
export function wireAdminPushStats(days: number, signal?: AbortSignal): Promise<AdminPushStats> {
  return wireFetch(adminPushStatsSchema, { path: `/admin/push-stats?days=${days}`, signal })
}
