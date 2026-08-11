/**
 * The Send list, and the two things you can do to a send that failed.
 *
 * Backend: crates/webapi/src/handlers/sends.rs
 */

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import {
  redraftSchema,
  resendResultSchema,
  scheduledListSchema,
  sendsSchema,
  type WireRedraft,
  type WireResendResult,
  type WireSend,
} from '../schemas/sends'

export type ScheduledSend = {
  id: string
  recipient: string
  scheduledAt: number
  subject: string
}

export async function wireCancelScheduled(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'POST',
    path: `/scheduled/${encodeURIComponent(id)}/cancel`,
  })
}

/**
 * A failed send as compose fields. The attachments come back described
 * but not transferred — a later send names the ones to keep by index and
 * the server re-extracts the bytes it never sent to the browser.
 */
export function wireGetRedraft(sendId: string): Promise<WireRedraft> {
  return wireFetch(redraftSchema, {
    path: `/mail/sends/${encodeURIComponent(sendId)}/redraft`,
  })
}

// ── scheduled ────────────────────────────────────────────────────
//
// Mail that has been written and has not left yet. `POST
// /api/scheduled/{id}/cancel` existed since G13.3 with no caller on
// any platform, because nothing could list what there was to cancel —
// the listing was an MCP tool. `GET /api/scheduled` is that listing,
// added 2026-08-10.
//
// Backend: `crates/webapi/src/handlers/scheduled.rs` — `list_scheduled`
// answers `{items: [{id, scheduled_at, recipient, subject}]}`, soonest
// first, and only the caller's own.

export async function wireListScheduled(): Promise<readonly ScheduledSend[]> {
  const raw = await wireFetch(scheduledListSchema, { path: '/scheduled' })
  return raw.items
}

/**
 * `status` filters server-side. An unrecognised value is a 400 rather
 * than a silently unfiltered list, so pass only the known names.
 */
export function wireListSends(status?: null | string): Promise<readonly WireSend[]> {
  const suffix = status ? `&status=${encodeURIComponent(status)}` : ''
  return wireFetch(sendsSchema, { path: `/mail/sends?limit=100${suffix}` })
}

/**
 * Re-enqueue the stored envelope unchanged. Same message, same
 * Message-ID, new send id — the recipient never received it.
 */
export function wireResend(sendId: string): Promise<WireResendResult> {
  return wireFetch(resendResultSchema, {
    method: 'POST',
    path: `/mail/sends/${encodeURIComponent(sendId)}/resend`,
  })
}
