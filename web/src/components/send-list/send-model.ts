import type { WireSentMessage } from '@/wire/schemas/mail'
import type { WireSend, WireSendStatus } from '@/wire/schemas/sends'

/**
 * Joining the two things the Send view is made of.
 *
 * The message list comes from the sent axis, which holds every outbound
 * message ever. Delivery status comes from the Send projection, which
 * only holds sends made since it shipped (2026-07-30). Replacing the list
 * with the projection would have made the user's entire history vanish,
 * and backfilling the projection would have meant writing `delivered` on
 * mails whose outcome nobody recorded — some of which bounced.
 *
 * So status is an enrichment, and its absence is rendered as absence.
 * `null` is not `0`: a send with no row has an unknown outcome, and the
 * honest display of unknown is nothing at all
 * (`.claude/rules/common/coding-style.md` → Null vs Zero).
 *
 * The join key is `message_id`. Both sides store it bare, without angle
 * brackets — verified against a prod capture of each on 2026-07-30. If
 * one side ever gains brackets the join fails silently and every row
 * loses its badge, which is why `joinKey` is one function and not two
 * inline expressions.
 */
export type SendRow = {
  msg: WireSentMessage
  /** Absent for every send that predates the projection. */
  send: null | WireSend
}

/**
 * The recipients that did not make it, with what the remote said.
 *
 * Empty when everything landed — the detail panel exists to explain a
 * failure, and listing successes alongside buries the one line that
 * matters.
 */
export function failedRecipients(send: WireSend): WireSend['recipients'] {
  return send.recipients.filter((r) => !r.delivered && !r.pending)
}

/**
 * Index sends by the message they belong to, keeping the newest attempt
 * per message.
 *
 * A resend creates a second Send row for the same message. The row that
 * matters is the latest one — the question the view answers is "where
 * does this mail stand now", not "how did the first attempt go".
 */
export function indexSendsByMessage(sends: readonly WireSend[]): Map<string, WireSend> {
  const out = new Map<string, WireSend>()
  for (const send of sends) {
    // `resent_from` names the original, so its own `send_id` carries the
    // `#r<n>` suffix and cannot be the join key.
    const key = joinKey(send.resent_from ?? send.send_id)
    const seen = out.get(key)
    if (!seen || send.created_at > seen.created_at) {
      out.set(key, send)
    }
  }
  return out
}

/** Normalise a Message-ID for comparison: no brackets, no case. */
export function joinKey(raw: string): string {
  return raw.trim().replace(/^</, '').replace(/>$/, '').toLowerCase()
}

/** Attach each message's send record, where there is one. */
export function joinSends(
  messages: readonly WireSentMessage[],
  sends: readonly WireSend[]
): SendRow[] {
  const byMessage = indexSendsByMessage(sends)
  return messages.map((msg) => ({
    msg,
    send: byMessage.get(joinKey(msg.message_id)) ?? null,
  }))
}

/**
 * Whether a row needs the user's attention: a send that failed, or one
 * that reached some recipients and not others.
 */
export function needsAttention(row: SendRow): boolean {
  if (!row.send) return false
  return row.send.status === 'failed' || row.send.status === 'partial'
}

const STATUS_LABELS: Record<WireSendStatus, string> = {
  delivered: 'Delivered',
  failed: 'Failed',
  partial: 'Partly delivered',
  scheduled: 'Scheduled',
  sending: 'Sending',
}

/**
 * Filter rows to one status.
 *
 * `null` shows everything. Rows with no send record are excluded by any
 * status filter rather than swept into a default bucket — they have no
 * status, and putting them under one would be a claim about their
 * outcome.
 */
export function filterByStatus(rows: readonly SendRow[], status: null | WireSendStatus) {
  if (!status) return [...rows]
  return rows.filter((row) => row.send?.status === status)
}

export function statusLabel(status: WireSendStatus): string {
  return STATUS_LABELS[status]
}
