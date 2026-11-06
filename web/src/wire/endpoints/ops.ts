import { wireFetch } from '../client'
import {
  opsDoneSchema,
  opsHistorySchema,
  opsSilenceSetSchema,
  opsStateSchema,
  type WireOpsHistory,
  type WireOpsSilence,
  type WireOpsState,
} from '../schemas/ops'

/** `GET /admin/ops/state` — the ops daemon's state, or why webapi cannot see it. */
export const wireOpsState = (signal?: AbortSignal): Promise<WireOpsState> =>
  wireFetch(opsStateSchema, { path: '/admin/ops/state', signal })

/** `GET /admin/ops/history` — the last day of transitions, notifications and controls. */
export const wireOpsHistory = (signal?: AbortSignal): Promise<WireOpsHistory> =>
  wireFetch(opsHistorySchema, { path: '/admin/ops/history', signal })

export type SilenceInput = {
  forSecs: number
  object: null | string
  reason: string
  rule: string
}

/** `POST /admin/ops/silences` — silence a rule, or one object of it, for a while. */
export const wireOpsSilence = (s: SilenceInput): Promise<WireOpsSilence> =>
  wireFetch(opsSilenceSetSchema, {
    body: { for_secs: s.forSecs, object: s.object, reason: s.reason, rule: s.rule },
    method: 'POST',
    path: '/admin/ops/silences',
  })

/** `POST /admin/ops/silences/{id}/end` — end a silence before its time. */
export const wireOpsEndSilence = (id: string, reason: string): Promise<unknown> =>
  wireFetch(opsDoneSchema, {
    body: { reason },
    method: 'POST',
    path: `/admin/ops/silences/${encodeURIComponent(id)}/end`,
  })

export type MaintenanceAction = 'end' | 'extend' | 'start'

/** `POST /admin/ops/maintenance` — start or extend (with an end), or end maintenance. */
export const wireOpsMaintenance = (
  action: MaintenanceAction,
  reason: string,
  forSecs: null | number
): Promise<unknown> =>
  wireFetch(opsDoneSchema, {
    body: maintenanceBody(action, reason, forSecs),
    method: 'POST',
    path: '/admin/ops/maintenance',
  })

function maintenanceBody(action: MaintenanceAction, reason: string, forSecs: null | number) {
  if (action === 'end' || forSecs === null) return { action, reason }
  return { action, for_secs: forSecs, reason }
}
