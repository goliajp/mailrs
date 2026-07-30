/**
 * The Send list, and the two things you can do to a send that failed.
 *
 * Backend: crates/webapi/src/handlers/sends.rs
 */

import { wireFetch } from '../client'
import {
  redraftSchema,
  resendResultSchema,
  sendsSchema,
  type WireRedraft,
  type WireResendResult,
  type WireSend,
} from '../schemas/sends'

/**
 * A failed send as compose fields. The attachments come back described
 * but not transferred — a later send names the ones to keep by index and
 * the server re-extracts the bytes it never sent to the browser.
 */
export function wireGetRedraft(sendId: string): Promise<WireRedraft> {
  return wireFetch(redraftSchema, {
    path: `/mail/sends/${encodeURIComponent(sendId)}:redraft`,
  })
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
    path: `/mail/sends/${encodeURIComponent(sendId)}:resend`,
  })
}
